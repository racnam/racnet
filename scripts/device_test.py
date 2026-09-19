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

    def metadata(self):
        props = {key: self.shell("getprop", key) for key in (
            "ro.product.manufacturer", "ro.product.model", "ro.build.version.release",
            "ro.build.version.sdk", "ro.build.fingerprint", "ro.kernel.qemu")}
        package = self.shell("dumpsys", "package", PACKAGE)
        installed = "package:" + PACKAGE in self.shell("pm", "list", "packages", PACKAGE).splitlines()
        apk_paths = self.shell("pm", "path", PACKAGE).splitlines() if installed else []
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
        self.adb("install", "-r", str(apk), timeout=120)
        sdk = int(self.shell("getprop", "ro.build.version.sdk"))
        if sdk < 29:
            raise RuntimeError("Android 10 / API 29 or later required")
        permissions = (["BLUETOOTH_SCAN", "BLUETOOTH_CONNECT", "BLUETOOTH_ADVERTISE"] if sdk >= 31
                       else ["ACCESS_COARSE_LOCATION", "ACCESS_FINE_LOCATION"])
        if sdk >= 33:
            permissions.append("POST_NOTIFICATIONS")
        for permission in permissions:
            self.shell("pm", "grant", PACKAGE, "android.permission." + permission)
        self.launch()

    def launch(self):
        self.shell("am", "start", "-W", "-n", PACKAGE + "/.MainActivity")

    def ui(self):
        self.shell("uiautomator", "dump", "/data/local/tmp/racnet-test-ui.xml", timeout=20)
        xml = self.shell("cat", "/data/local/tmp/racnet-test-ui.xml")
        root = ET.fromstring(xml)
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
        root = self.board()
        field = next(n for n in root.iter("node") if n.get("class") == "android.widget.EditText")
        if field.get("text"):
            raise RuntimeError("Unsent draft found; save or discard it on the phone before testing")
        self.tap(field)
        self.shell("input", "text", marker)
        self.shell("input", "keyevent", "KEYCODE_BACK")
        root, _ = self.ui()  # Wait for keyboard dismissal before finding the button.
        button = next((n for n in root.iter("node") if n.get("text") == "Post to board"), None)
        if button is None:
            raise RuntimeError("Post button unavailable")
        self.tap(button)
        return self.wait_entry(marker=marker, timeout=15)

    def restart(self):
        self.shell("am", "force-stop", PACKAGE)
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
            process = subprocess.Popen(["adb", "-s", device.serial, "logcat", "-v", "epoch", "-T", since,
                                        "RacnetMeas:I", "RacnetCentral:I", "RacnetPeripheral:I", "*:S"],
                                       stdout=output, stderr=subprocess.STDOUT)
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
            if process.poll() is not None:
                self.report["errors"].append("Logcat stream exited before capture finished")
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            output.close()
        for index, device in enumerate(self.devices):
            directory = self.folder / f"device-{index + 1}"
            try:
                device.snapshot(directory)
            except Exception as error:
                self.report["errors"].append(f"Snapshot {device.serial}: {error}")
            log = directory / "logcat.txt"
            if log.exists():
                (directory / "events.json").write_text(json.dumps(event_counts(log.read_text(errors="replace")), indent=2) + "\n")
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


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["inventory", "prepare", "offline", "pair", "capture"])
    parser.add_argument("--serial", action="append", default=[], help="Explicit adb serial; repeat for two phones")
    parser.add_argument("--apk", type=Path, help="prepare only: APK to install in place, never uninstall")
    parser.add_argument("--output", type=Path, help="New evidence directory (must not already exist)")
    parser.add_argument("--seconds", type=int, default=60, help="capture duration")
    parser.add_argument("--timeout", type=int, default=90, help="pair sync deadline per transfer")
    parser.add_argument("--note", default="", help="Environment/distance/battery configuration as observed")
    args = parser.parse_args(argv)
    if args.seconds < 1 or args.timeout < 1:
        parser.error("durations must be positive")
    if args.command == "inventory":
        print(json.dumps(connected(), indent=2))
        return 0
    if len(set(args.serial)) != len(args.serial):
        parser.error("serials must be distinct")
    expected = 2 if args.command == "pair" else 1 if args.command == "offline" else None
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
    folder = args.output or ROOT / "test-runs" / (datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:8])
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
        if args.command in ("offline", "pair"):
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
