#!/usr/bin/env python3
"""Racnet debug-APK tests and evidence capture. Standard library + adb only."""
import argparse
import hashlib
import json
import re
import shlex
import subprocess
import sys
import time
import uuid
import xml.etree.ElementTree as ET
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path

PACKAGE = "org.racnet.android"
ROOT = Path(__file__).resolve().parents[1]
MAX_JOURNAL = 64 * 1024 * 1024


def run(args, *, binary=False, timeout=30):
    result = subprocess.run(args, capture_output=True, timeout=timeout)
    if result.returncode:
        error = result.stderr.decode(errors="replace") or result.stdout.decode(errors="replace")
        raise RuntimeError(f"{shlex.join(args[:5])}: {error.strip()}")
    return result.stdout if binary else result.stdout.decode(errors="replace").strip()


def connected():
    return dict(line.split()[:2] for line in run(["adb", "devices"]).splitlines()[1:] if line.strip())


def utc():
    return datetime.now(timezone.utc).isoformat()


def entries(data):
    """Read complete canonical journal records; tolerate only an incomplete tail.

    Rust verifies signatures before persisting. This reader extracts evidence;
    it does not independently certify signatures or trust arbitrary imports.
    """
    if len(data) > MAX_JOURNAL or not data.startswith(b"RACNET01"):
        raise ValueError("invalid or oversized entry journal")
    offset = 8
    result = []
    while offset + 4 <= len(data):
        length = int.from_bytes(data[offset:offset + 4], "little")
        if not 0 < length <= 1_048_576:
            raise ValueError("invalid journal record length")
        if offset + 4 + length > len(data):
            break
        record = data[offset + 4:offset + 4 + length]
        pos = 0

        def value(major):
            nonlocal pos
            if pos >= len(record):
                raise ValueError("truncated CBOR")
            head = record[pos]
            pos += 1
            if head >> 5 != major:
                raise ValueError("unexpected CBOR type")
            size = head & 31
            if size >= 24:
                widths = {24: 1, 25: 2, 26: 4, 27: 8}
                width = widths.get(size)
                if width is None or pos + width > len(record):
                    raise ValueError("invalid CBOR length")
                size = int.from_bytes(record[pos:pos + width], "big")
                pos += width
                if size < {1: 24, 2: 256, 4: 65536, 8: 4294967296}[width]:
                    raise ValueError("noncanonical CBOR integer")
            if major == 2:
                if pos + size > len(record):
                    raise ValueError("truncated CBOR bytes")
                output = record[pos:pos + size]
                pos += size
                return output
            return size

        if value(4) != 5:
            raise ValueError("entry must have five fields")
        author, timestamp, kind, payload, signature = value(2), value(0), value(0), value(2), value(2)
        if len(author) != 32 or len(signature) != 64 or pos != len(record) or timestamp == 2**64 - 1:
            raise ValueError("invalid entry fields")
        result.append({"id": hashlib.sha256(record).hexdigest(), "author": author.hex(),
                       "timestamp_ms": timestamp, "kind": kind, "payload": payload})
        offset += 4 + length
    return result


def event_counts(log):
    return dict(Counter(re.findall(r"\bMEAS event=([a-z_]+)\b", log)))


class Device:
    def __init__(self, serial):
        self.serial = serial

    def adb(self, *args, **kwargs):
        return run(["adb", "-s", self.serial, *args], **kwargs)

    def shell(self, *args, **kwargs):
        return self.adb("shell", shlex.join(args), **kwargs)

    def require_owner_user(self):
        if self.shell("am", "get-current-user") != "0":
            raise RuntimeError("Automated tests require the Android owner profile; switch to it before testing")

    def metadata(self):
        props = {key: self.shell("getprop", key) for key in (
            "ro.product.manufacturer", "ro.product.model", "ro.build.version.release",
            "ro.build.version.sdk", "ro.build.fingerprint", "ro.kernel.qemu")}
        package = self.shell("dumpsys", "package", PACKAGE)
        installed = "package:" + PACKAGE in self.shell("pm", "list", "packages", "--user", "0", PACKAGE).splitlines()
        apk_paths = self.shell("pm", "path", "--user", "0", PACKAGE).splitlines() if installed else []
        apk_hash = None
        if apk_paths and apk_paths[0].startswith("package:"):
            apk_hash = self.shell("sha256sum", apk_paths[0].removeprefix("package:")).split()[0]
            if not re.fullmatch(r"[0-9a-f]{64}", apk_hash):
                raise RuntimeError("Cannot identify installed APK hash")
        return {"serial": self.serial, "properties": props,
                "emulator": props["ro.kernel.qemu"] == "1", "installed_apk_sha256": apk_hash,
                "device_time": self.shell("date", "+%Y-%m-%dT%H:%M:%S%z"),
                "bluetooth_on": self.shell("settings", "get", "global", "bluetooth_on"),
                "location_mode": self.shell("settings", "get", "secure", "location_mode"),
                "package_versions": [line.strip() for line in package.splitlines()
                                     if "versionName=" in line or "versionCode=" in line]}

    def prepare(self, apk):
        self.require_owner_user()
        self.adb("install", "--user", "0", "-r", str(apk), timeout=120)
        sdk = int(self.shell("getprop", "ro.build.version.sdk"))
        if sdk < 29:
            raise RuntimeError("Android 10 / API 29 or later required")
        permissions = (["BLUETOOTH_SCAN", "BLUETOOTH_CONNECT", "BLUETOOTH_ADVERTISE"] if sdk >= 31
                       else ["ACCESS_COARSE_LOCATION", "ACCESS_FINE_LOCATION"])
        if sdk >= 33:
            permissions.append("POST_NOTIFICATIONS")
        for permission in permissions:
            self.shell("pm", "grant", "--user", "0", PACKAGE, "android.permission." + permission)
        self.launch()

    def launch(self):
        self.require_owner_user()
        self.shell("am", "start", "--user", "0", "-W", "-n", PACKAGE + "/.MainActivity")

    def raw_ui(self):
        self.shell("uiautomator", "dump", "/data/local/tmp/racnet-test-ui.xml", timeout=20)
        xml = self.shell("cat", "/data/local/tmp/racnet-test-ui.xml")
        root = ET.fromstring(xml)
        return root, xml

    def ui(self):
        root, xml = self.raw_ui()
        if not any(n.get("package") == PACKAGE for n in root.iter("node")):
            raise RuntimeError("Racnet is not visible. Unlock the phone and dismiss system prompts.")
        return root, xml

    def tap(self, node):
        coordinates = list(map(int, re.findall(r"\d+", node.get("bounds", ""))))
        if len(coordinates) != 4 or node.get("enabled") == "false":
            raise RuntimeError("UI control is unavailable")
        x1, y1, x2, y2 = coordinates
        if x2 <= x1 or y2 <= y1:
            raise RuntimeError("UI control is outside the visible screen")
        self.shell("input", "tap", str((x1 + x2) // 2), str((y1 + y2) // 2))

    def board(self):
        self.launch()
        for _ in range(5):
            root, _ = self.ui()
            nodes = list(root.iter("node"))
            if any(n.get("class") == "android.widget.EditText" for n in nodes):
                return root
            button = next((n for n in nodes if n.get("text") in ("Continue", "Continue offline", "← Back")), None)
            if button is None:
                if any(n.get("text") == "Opening local messages…" for n in nodes):
                    time.sleep(1)
                    continue
                error = " ".join(n.get("text", "") for n in nodes)
                raise RuntimeError("Cannot open board: " + error[:500])
            self.tap(button)
        raise RuntimeError("Board did not open")

    def mesh(self, enabled):
        root = self.board()
        toggle = next((n for n in root.iter("node") if n.get("checkable") == "true"), None)
        if toggle is None:
            raise RuntimeError("Cannot locate mesh switch")
        if (toggle.get("checked") == "true") != enabled:
            self.tap(toggle)
            for _ in range(5):
                root, _ = self.ui()
                toggle = next((n for n in root.iter("node") if n.get("checkable") == "true"), None)
                if toggle is not None and (toggle.get("checked") == "true") == enabled:
                    return
            raise RuntimeError("Mesh did not reach requested state; check Bluetooth and permissions")

    def draft(self):
        root = self.board()
        fields = [n for n in root.iter("node") if n.get("class") == "android.widget.EditText"]
        if len(fields) != 1:
            raise RuntimeError("Cannot identify board draft field")
        return fields[0]

    def require_empty_draft(self):
        if self.draft().get("text"):
            raise RuntimeError("Unsent draft found; save or discard it on the phone before testing")

    def enter_draft(self, marker):
        self.require_empty_draft()
        self.tap(self.draft())
        self.shell("input", "text", marker)
        self.shell("input", "keyevent", "KEYCODE_BACK")

    def discard_draft(self, marker):
        field = self.draft()
        if field.get("text", "") == "":
            return
        if field.get("text") != marker:
            raise RuntimeError("Draft changed; preserving it instead of clearing it")
        self.tap(field)
        self.shell("input", "keyevent", "KEYCODE_MOVE_END")
        for _ in marker:
            self.shell("input", "keyevent", "KEYCODE_DEL")
        self.shell("input", "keyevent", "KEYCODE_BACK")
        if self.draft().get("text"):
            raise RuntimeError("Synthetic draft cleanup did not clear the field")

    def bluetooth_state(self):
        output = self.shell("dumpsys", "bluetooth_manager")
        states = re.findall(r"^\s*state:\s*(ON|OFF|TURNING_ON|TURNING_OFF)\s*$", output, re.MULTILINE | re.IGNORECASE)
        if len(states) != 1:
            raise RuntimeError("Cannot identify actual Bluetooth adapter state")
        return states[0].upper()

    def bluetooth(self, enabled, control):
        target = "ON" if enabled else "OFF"
        if self.bluetooth_state() == target:
            return {"state": target, "control": control}
        if control == "shell":
            self.shell("svc", "bluetooth", "enable" if enabled else "disable")
        else:
            self.shell("am", "start", "-W", "-a", "android.settings.BLUETOOTH_SETTINGS")
            root, _ = self.raw_ui()
            # Only recognized Settings main-switch IDs; never guess from paired-device rows.
            ids = {"com.android.settings:id/switch_widget", "com.android.settings:id/switch_bar",
                   "com.android.settings:id/switch_main", "com.android.settings:id/switch_compat"}
            switches = [n for n in root.iter("node") if n.get("resource-id") in ids
                        and n.get("checkable") == "true" and n.get("package") == "com.android.settings"]
            if len(switches) != 1:
                raise RuntimeError("Cannot uniquely identify Bluetooth Settings switch; handle the OEM layout manually")
            if (switches[0].get("checked") == "true") == enabled:
                raise RuntimeError("Bluetooth Settings switch disagrees with adapter state")
            self.tap(switches[0])
        deadline = time.monotonic() + 15
        while self.bluetooth_state() != target:
            if time.monotonic() >= deadline:
                raise RuntimeError("Bluetooth did not reach requested state; shell control may be unsupported; try --bluetooth-control settings")
            time.sleep(0.5)
        self.launch()
        return {"state": target, "control": control}

    def radio_off_error(self):
        expected = "Bluetooth is off. Enable it, then turn the mesh on again."
        for _ in range(5):
            root = self.board()
            switches = [n for n in root.iter("node") if n.get("checkable") == "true"]
            if (len(switches) == 1 and switches[0].get("checked") == "false"
                    and any(n.get("text") == expected for n in root.iter("node"))
                    and self.bluetooth_state() == "OFF"):
                return {"bluetooth": "OFF", "mesh": False, "error": expected}
        raise RuntimeError("Expected radio-off error and stopped mesh were not observed")

    def rotation_settings(self):
        return {key: self.shell("settings", "get", "system", key)
                for key in ("accelerometer_rotation", "user_rotation")}

    def restore_rotation(self, settings):
        errors = []
        for key, value in settings.items():
            try:
                if value == "null":
                    self.shell("settings", "delete", "system", key)
                else:
                    self.shell("settings", "put", "system", key, value)
                if self.shell("settings", "get", "system", key) != value:
                    raise RuntimeError("Rotation setting restoration was not observed")
            except Exception as error:
                errors.append(str(error))
        if errors:
            raise RuntimeError("; ".join(errors))

    def rotate(self, rotation):
        self.shell("settings", "put", "system", "accelerometer_rotation", "0")
        self.shell("settings", "put", "system", "user_rotation", str(rotation))
        for _ in range(5):
            root, _ = self.ui()
            if root.get("rotation") == str(rotation):
                return {"observed_rotation": rotation}
        raise RuntimeError("Display did not reach requested rotation")

    def stored(self):
        return entries(self.adb("exec-out", "run-as", PACKAGE, "cat", "files/entries.racnet", binary=True))

    def wait_entry(self, *, marker=None, entry_id=None, timeout=60):
        start = time.monotonic()
        while True:
            found = next((entry for entry in self.stored() if
                          (entry_id is not None and entry["id"] == entry_id) or
                          (marker is not None and entry["kind"] == 1 and entry["payload"] == marker.encode())), None)
            if found:
                return {key: value for key, value in found.items() if key != "payload"}
            if time.monotonic() - start >= timeout:
                raise RuntimeError(f"Entry not received within {timeout}s on {self.serial}")
            time.sleep(0.5)

    def post(self, marker):
        field = self.draft()
        if field.get("text"):
            raise RuntimeError("Unsent draft found; save or discard it on the phone before testing")
        self.tap(field)
        self.shell("input", "text", marker)
        self.shell("input", "keyevent", "KEYCODE_BACK")
        root, _ = self.ui()  # Wait for keyboard dismissal before finding the button.
        fields = [n for n in root.iter("node") if n.get("class") == "android.widget.EditText"]
        if len(fields) != 1 or fields[0].get("text") != marker:
            raise RuntimeError("Draft changed; preserving it instead of posting it")
        button = next((n for n in root.iter("node") if n.get("text") == "Post to board"), None)
        if button is None:
            raise RuntimeError("Post button unavailable")
        self.tap(button)
        return self.wait_entry(marker=marker, timeout=15)

    def restart(self):
        self.require_empty_draft()
        self.shell("am", "force-stop", "--user", "0", PACKAGE)
        root = self.board()
        if not any(n.get("class") == "android.widget.EditText" for n in root.iter("node")):
            raise RuntimeError("App failed to reopen")

    def snapshot(self, directory):
        directory.mkdir(parents=True, exist_ok=True)
        root, xml = self.ui()
        (directory / "ui.xml").write_text(xml)
        (directory / "screen.png").write_bytes(self.adb("exec-out", "screencap", "-p", binary=True))
        return root


class Session:
    def __init__(self, folder, devices, command, note):
        self.folder, self.devices, self.streams = folder, devices, []
        folder.mkdir(parents=True, exist_ok=False)
        self.report = {"schema": 1, "started_utc": utc(), "command": command, "note": note,
                       "status": "running", "steps": [], "devices": [], "errors": [],
                       "git_commit": run(["git", "-C", str(ROOT), "rev-parse", "HEAD"]),
                       "git_dirty": bool(run(["git", "-C", str(ROOT), "status", "--porcelain"])),
                       "scope": "Automated functional checks; no range or isolated radio throughput claims."}
        self.save()

    def save(self):
        temporary = self.folder / "report.json.tmp"
        temporary.write_text(json.dumps(self.report, indent=2) + "\n")
        temporary.replace(self.folder / "report.json")

    def start(self):
        for index, device in enumerate(self.devices):
            directory = self.folder / f"device-{index + 1}"
            directory.mkdir()
            metadata = device.metadata()
            self.report["devices"].append(metadata)
            (directory / "device.json").write_text(json.dumps(metadata, indent=2) + "\n")
            since = device.shell("date", "+%m-%d %H:%M:%S.000")
            output = (directory / "logcat.txt").open("wb")
            try:
                process = subprocess.Popen(["adb", "-s", device.serial, "logcat", "-v", "epoch", "-T", since,
                                            "RacnetMeas:I", "RacnetCentral:I", "RacnetPeripheral:I", "*:S"],
                                           stdout=output, stderr=subprocess.STDOUT)
            except BaseException:
                output.close()
                raise
            self.streams.append((process, output))
            self.save()

    def step(self, name, operation):
        start = time.monotonic()
        row = {"name": name, "started_utc": utc(), "status": "running"}
        self.report["steps"].append(row)
        self.save()
        print(name, flush=True)
        try:
            row["evidence"] = operation()
            row["status"] = "passed"
            return row["evidence"]
        except KeyboardInterrupt:
            row["status"] = "interrupted"
            raise
        except Exception as error:
            row.update(status="failed", error=str(error))
            raise
        finally:
            row["host_elapsed_s"] = round(time.monotonic() - start, 3)
            self.save()

    def finish(self):
        for process, output in self.streams:
            try:
                if process.poll() is not None:
                    self.report["errors"].append("Logcat stream exited before capture finished")
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            except Exception as error:
                self.report["errors"].append(f"Logcat shutdown: {error}")
            finally:
                try:
                    output.close()
                except Exception as error:
                    self.report["errors"].append(f"Logcat file closure: {error}")
        for index, device in enumerate(self.devices):
            directory = self.folder / f"device-{index + 1}"
            try:
                device.snapshot(directory)
            except Exception as error:
                self.report["errors"].append(f"Snapshot {device.serial}: {error}")
            log = directory / "logcat.txt"
            try:
                if log.exists():
                    (directory / "events.json").write_text(json.dumps(event_counts(log.read_text(errors="replace")), indent=2) + "\n")
            except Exception as error:
                self.report["errors"].append(f"Logcat summary: {error}")
        self.report["evidence_complete"] = not self.report["errors"]
        if self.report["errors"] and self.report["status"] in ("passed", "prepared", "captured_not_evaluated"):
            self.report["status"] = "incomplete"
        self.report["ended_utc"] = utc()
        self.save()
        lines = ["# Racnet device test", "", f"Result: **{self.report['status']}**", "",
                 f"Scenario: {self.report['command']}", f"Started: {self.report['started_utc']}",
                 f"Commit: {self.report['git_commit']} (dirty: {self.report['git_dirty']})", "",
                 "| Device | Model | Android | Emulator |", "|---|---|---|---|"]
        for index, metadata in enumerate(self.report["devices"]):
            props = metadata["properties"]
            lines.append(f"| {index + 1} | {props['ro.product.manufacturer']} {props['ro.product.model']} | {props['ro.build.version.release']} | {metadata['emulator']} |")
        lines += ["", "| Check | Result |", "|---|---|"]
        lines += [f"| {row['name']} | {row['status']} |" for row in self.report["steps"]]
        lines += ["", "Timing is host-observed automation duration, not radio throughput or range.",
                  "Exact entry IDs and errors are in report.json. Log counts alone do not prove sync.",
                  "Screenshots/XML can contain board messages. Keep this bundle private until reviewed."]
        (self.folder / "SUMMARY.md").write_text("\n".join(lines) + "\n")


def smoke(session, timeout):
    devices = session.devices
    markers = ["racnet-test-" + uuid.uuid4().hex for _ in devices]
    if len(devices) == 1:
        device = devices[0]
        session.step("Turn mesh off", lambda: device.mesh(False))
        entry = session.step("Post offline", lambda: device.post(markers[0]))
        session.step("Restart app", device.restart)
        session.step("Verify retained entry ID and author", lambda: device.wait_entry(entry_id=entry["id"]))
    else:
        a, b = devices
        for index, device in enumerate(devices):
            session.step(f"Enable mesh on device {index + 1}", lambda d=device: d.mesh(True))
        entry_a = session.step("Post on A", lambda: a.post(markers[0]))
        session.step("Verify exact signed entry on B", lambda: b.wait_entry(entry_id=entry_a["id"], timeout=timeout))
        entry_b = session.step("Post on B", lambda: b.post(markers[1]))
        session.step("Verify exact signed entry on A", lambda: a.wait_entry(entry_id=entry_b["id"], timeout=timeout))
        session.step("Restart B", b.restart)
        session.step("Verify received entry survives restart", lambda: b.wait_entry(entry_id=entry_a["id"]))
        queued = session.step("Post on A while B is disconnected", lambda: a.post("racnet-test-" + uuid.uuid4().hex))
        session.step("Reconnect B", lambda: b.mesh(True))
        session.step("Verify queued entry reaches B", lambda: b.wait_entry(entry_id=queued["id"], timeout=timeout))


def cleanup(session, name, operation):
    try:
        session.step(name, operation)
    except Exception as error:
        session.report["errors"].append(f"{name}: {error}")


def bluetooth_recovery(session, timeout, control):
    devices = session.devices
    for device in devices:
        device.require_empty_draft()
        if device.bluetooth_state() != "ON":
            raise RuntimeError("Bluetooth must initially be on for both devices")
    try:
        for index, device in enumerate(devices):
            session.step(f"Enable mesh on device {index + 1}", lambda d=device: d.mesh(True))
        for index, target in enumerate(devices):
            peer = devices[1 - index]
            session.step(f"Disable Bluetooth on device {index + 1}", lambda: target.bluetooth(False, control))
            session.step("Verify radio-off error and stopped mesh", target.radio_off_error)
            queued = session.step("Queue entry on peer", lambda: peer.post("racnet-test-" + uuid.uuid4().hex))
            session.step("Restore Bluetooth", lambda: target.bluetooth(True, control))
            session.step("Explicitly restart target mesh", lambda: target.mesh(True))
            session.step("Verify queued entry on target", lambda: target.wait_entry(entry_id=queued["id"], timeout=timeout))
            reply = session.step("Post reply on target", lambda: target.post("racnet-test-" + uuid.uuid4().hex))
            session.step("Verify exact reply on peer", lambda: peer.wait_entry(entry_id=reply["id"], timeout=timeout))
    finally:
        for index, device in enumerate(devices):
            cleanup(session, f"Restore Bluetooth on device {index + 1}", lambda d=device: d.bluetooth(True, control))


def draft_rotation(session):
    for device in session.devices:
        device.require_empty_draft()
    for index, device in enumerate(session.devices):
        settings = device.rotation_settings()
        marker = "racnet-draft-" + uuid.uuid4().hex
        try:
            session.step(f"Set portrait on device {index + 1}", lambda: device.rotate(0))
            session.step("Enter synthetic draft", lambda: device.enter_draft(marker))
            session.step("Observe landscape rotation", lambda: device.rotate(1))

            def retained():
                if device.draft().get("text") != marker:
                    raise RuntimeError("Exact synthetic draft was not retained")
                return {"exact_draft_retained": True}

            session.step("Verify exact retained draft", retained)
        finally:
            cleanup(session, "Restore rotation settings", lambda: device.restore_rotation(settings))
            cleanup(session, "Remove only synthetic draft", lambda: device.discard_draft(marker))


def batch(session, timeout, count):
    for device in session.devices:
        device.require_empty_draft()
    for index, device in enumerate(session.devices):
        session.step(f"Enable mesh on device {index + 1}", lambda d=device: d.mesh(True))
    posted = []
    for index, device in enumerate(session.devices):
        for number in range(count):
            posted.append(session.step(f"Post {number + 1} on device {index + 1}",
                                       lambda d=device: d.post("racnet-test-" + uuid.uuid4().hex)))
    for phase in ("before restart", "after restart"):
        if phase == "after restart":
            for index, device in enumerate(session.devices):
                session.step(f"Restart device {index + 1}", device.restart)
        for index, device in enumerate(session.devices):
            for number, entry in enumerate(posted):
                session.step(f"Verify entry {number + 1} on device {index + 1} {phase}",
                             lambda d=device, e=entry: d.wait_entry(entry_id=e["id"], timeout=timeout))


def validate_output(folder):
    resolved = folder.resolve()
    try:
        relative = resolved.relative_to(ROOT.resolve())
    except ValueError:
        return
    # Check an evidence filename as well as the directory, without creating either.
    for path in (relative, relative / "report.json"):
        result = subprocess.run(["git", "-C", str(ROOT), "check-ignore", "--quiet", "--", str(path)],
                                capture_output=True)
        if result.returncode != 0:
            raise ValueError("Evidence inside the repository must be git-ignored; use test-runs/")
    if run(["git", "-C", str(ROOT), "ls-files", "--", str(relative)]):
        raise ValueError("Evidence destination contains tracked files")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["inventory", "prepare", "offline", "pair", "capture", "bluetooth-recovery", "draft-rotation", "batch"])
    parser.add_argument("--serial", action="append", default=[], help="Explicit adb serial; repeat for two phones")
    parser.add_argument("--apk", type=Path, help="prepare only: APK to install in place, never uninstall")
    parser.add_argument("--output", type=Path, help="New evidence directory (must not already exist)")
    parser.add_argument("--seconds", type=int, default=60, help="capture duration")
    parser.add_argument("--timeout", type=int, default=90, help="sync deadline per transfer")
    parser.add_argument("--count", type=int, help="batch only: posts per phone, 1–20 (default 5)")
    parser.add_argument("--bluetooth-control", choices=["shell", "settings"],
                        help="bluetooth-recovery only: radio control (default shell); settings for unsupported OEM shells")
    parser.add_argument("--note", default="", help="Environment/distance/battery configuration as observed")
    args = parser.parse_args(argv)
    if args.seconds < 1 or args.timeout < 1:
        parser.error("durations must be positive")
    if args.count is not None and (args.command != "batch" or not 1 <= args.count <= 20):
        parser.error("--count is batch-only and must be between 1 and 20")
    if args.bluetooth_control is not None and args.command != "bluetooth-recovery":
        parser.error("--bluetooth-control is only used by bluetooth-recovery")
    if args.command == "inventory":
        print(json.dumps(connected(), indent=2))
        return 0
    if len(set(args.serial)) != len(args.serial):
        parser.error("serials must be distinct")
    expected = 2 if args.command in ("pair", "batch", "bluetooth-recovery") else 1 if args.command == "offline" else None
    if not args.serial or (expected is not None and len(args.serial) != expected):
        parser.error(f"select {'exactly ' + str(expected) if expected else 'one or more'} device(s) with --serial")
    if args.command == "prepare" and (args.apk is None or not args.apk.is_file()):
        parser.error("prepare requires an existing --apk")
    if args.command != "prepare" and args.apk is not None:
        parser.error("--apk is only used by prepare")
    available = connected()
    for serial in args.serial:
        if available.get(serial) != "device":
            parser.error(f"{serial}: {available.get(serial, 'not connected')}; connect/unlock and accept USB debugging")
    devices = [Device(serial) for serial in args.serial]
    try:
        for device in devices:
            device.require_owner_user()
    except RuntimeError as error:
        parser.error(str(error))
    folder = args.output or ROOT / "test-runs" / (datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:8])
    try:
        validate_output(folder)
    except ValueError as error:
        parser.error(str(error))
    session = Session(folder, devices, args.command, args.note)
    exit_code = 1
    try:
        session.start()
        if args.command == "prepare":
            session.report["apk_sha256"] = hashlib.sha256(args.apk.read_bytes()).hexdigest()
            for index, device in enumerate(devices):
                session.step(f"Install and grant permissions on device {index + 1}", lambda d=device: d.prepare(args.apk))
                session.report["devices"][index] = device.metadata()
                (folder / f"device-{index + 1}" / "device.json").write_text(
                    json.dumps(session.report["devices"][index], indent=2) + "\n")
            session.report["status"] = "prepared"
        elif args.command == "capture":
            print(f"Capturing for {args.seconds}s in {folder}", flush=True)
            time.sleep(args.seconds)
            session.report["status"] = "captured_not_evaluated"
        else:
            if args.command == "bluetooth-recovery":
                bluetooth_recovery(session, args.timeout, args.bluetooth_control or "shell")
            elif args.command == "draft-rotation":
                draft_rotation(session)
            elif args.command == "batch":
                batch(session, args.timeout, args.count or 5)
            else:
                smoke(session, args.timeout)
            session.report["status"] = "passed"
        exit_code = 0
    except KeyboardInterrupt:
        session.report["status"] = "interrupted"
        exit_code = 130
    except Exception as error:
        session.report.update(status="failed", failure=str(error))
        print(str(error), file=sys.stderr)
    finally:
        if args.command in ("offline", "pair", "batch", "bluetooth-recovery", "draft-rotation"):
            for device in devices:
                try:
                    device.mesh(False)
                except Exception as error:
                    session.report["errors"].append(f"Mesh cleanup: {error}")
        session.finish()
        if session.report["status"] == "incomplete" or session.report["errors"]:
            exit_code = exit_code or 1
        print(f"Report: {folder / 'SUMMARY.md'}", flush=True)
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
