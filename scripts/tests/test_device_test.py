import hashlib
import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location("device_test", Path(__file__).parents[1] / "device_test.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def uint(value, major=0):
    if value < 24:
        return bytes([major * 32 + value])
    if value < 256:
        return bytes([major * 32 + 24, value])
    return bytes([major * 32 + 25]) + value.to_bytes(2, "big")


def blob(value):
    return uint(len(value), 2) + value


def record(payload=b"message"):
    return b"\x85" + blob(b"a" * 32) + uint(1000) + uint(1) + blob(payload) + blob(b"s" * 64)


def journal(*records):
    return b"RACNET01" + b"".join(struct.pack("<I", len(r)) + r for r in records)


class JournalTests(unittest.TestCase):
    def test_exact_entry_id_and_payload(self):
        raw = record("hello 世界".encode())
        entry, = module.entries(journal(raw))
        self.assertEqual(entry["id"], hashlib.sha256(raw).hexdigest())
        self.assertEqual(entry["payload"], "hello 世界".encode())
        self.assertEqual(entry["kind"], 1)
        self.assertEqual(entry["timestamp_ms"], 1000)

    def test_incomplete_tail_never_becomes_delivery_evidence(self):
        first = journal(record(b"first"))
        second = journal(record(b"second"))[8:]
        for cut in range(len(second)):
            self.assertEqual(len(module.entries(first + second[:cut])), 1)
        self.assertEqual(len(module.entries(first + second)), 2)

    def test_invalid_complete_records_fail_instead_of_passing(self):
        for bad in (b"bad", b"RACNET01" + b"\xff" * 4,
                    journal(record() + b"\x00"), journal(b"\x84" + record()[1:]),
                    journal(record()[:-1]), journal(record().replace(b"\x19\x03\xe8", b"\x18\x01", 1))):
            with self.subTest(bad=bad[:20]):
                with self.assertRaises(ValueError):
                    module.entries(bad)

    def test_embedded_marker_in_wrong_kind_is_not_a_board_post(self):
        raw = record().replace(b"\x19\x03\xe8\x01", b"\x19\x03\xe8\x00", 1)
        entry, = module.entries(journal(raw))
        self.assertEqual(entry["kind"], 0)

    def test_empty_store(self):
        self.assertEqual(module.entries(b"RACNET01"), [])


class ReportingTests(unittest.TestCase):
    def test_metadata_before_first_install_does_not_require_an_apk_path(self):
        class UninstalledDevice(module.Device):
            def shell(self, *args, **kwargs):
                if args[:2] == ("pm", "path"):
                    raise AssertionError("pm path fails before installation")
                return ""
        metadata = UninstalledDevice("test").metadata()
        self.assertIsNone(metadata["installed_apk_sha256"])
        self.assertEqual(metadata["package_versions"], [])

    def test_metadata_does_not_treat_another_profiles_install_as_owner_install(self):
        device = module.Device("test")

        def shell(*args):
            if args == ("pm", "list", "packages", module.PACKAGE):
                return "package:" + module.PACKAGE
            if args[:2] == ("pm", "path"):
                raise AssertionError("APK path must not be queried when absent in the owner profile")
            return ""

        device.shell = Mock(side_effect=shell)
        self.assertIsNone(device.metadata()["installed_apk_sha256"])
        device.shell.assert_any_call("pm", "list", "packages", "--user", "0", module.PACKAGE)

    def test_event_counts_are_diagnostic_not_fabricated_measurements(self):
        self.assertEqual(module.event_counts("x MEAS event=established link=1\n"
                                           "x MEAS event=sync_done bytes_in=100\n"
                                           "unrelated event=sync_done\n"),
                         {"established": 1, "sync_done": 1})

    def test_step_returns_evidence_and_records_failure(self):
        with tempfile.TemporaryDirectory() as parent:
            session = module.Session(Path(parent) / "run", [], "offline", "test")
            self.assertEqual(session.step("good", lambda: {"id": "abc"}), {"id": "abc"})
            with self.assertRaises(ValueError):
                session.step("bad", lambda: int("bad"))
            self.assertEqual([step["status"] for step in session.report["steps"]], ["passed", "failed"])

    def test_capture_is_not_a_passing_hardware_test(self):
        with tempfile.TemporaryDirectory() as parent:
            session = module.Session(Path(parent) / "run", [], "capture", "test")
            session.report["status"] = "captured_not_evaluated"
            session.finish()
            self.assertIn("captured_not_evaluated", (session.folder / "SUMMARY.md").read_text())

    def test_capture_errors_prevent_a_complete_pass(self):
        with tempfile.TemporaryDirectory() as parent:
            session = module.Session(Path(parent) / "run", [], "offline", "test")
            session.report.update(status="passed", errors=["lost stream"])
            session.finish()
            self.assertEqual(session.report["status"], "incomplete")

    def test_failed_logcat_start_closes_capture_file(self):
        device = Mock(serial="test")
        device.metadata.return_value = {}
        with tempfile.TemporaryDirectory() as parent:
            session = module.Session(Path(parent) / "run", [device], "capture", "test")
            with patch.object(module.subprocess, "Popen", side_effect=OSError("cannot start")) as start:
                with self.assertRaisesRegex(OSError, "cannot start"):
                    session.start()
            self.assertTrue(start.call_args.kwargs["stdout"].closed)
            self.assertEqual(session.streams, [])

    def test_logcat_shutdown_failure_does_not_skip_other_streams_or_report(self):
        first, second = Mock(), Mock()
        first.poll.return_value = second.poll.return_value = None
        first.terminate.side_effect = OSError("cannot stop stream")
        first_output, second_output = Mock(), Mock()
        with tempfile.TemporaryDirectory() as parent:
            session = module.Session(Path(parent) / "run", [], "capture", "test")
            session.streams = [(first, first_output), (second, second_output)]
            session.report["status"] = "captured_not_evaluated"
            session.finish()
            second.terminate.assert_called_once()
            first_output.close.assert_called_once()
            second_output.close.assert_called_once()
            self.assertEqual(session.report["status"], "incomplete")
            self.assertIn("incomplete", (session.folder / "SUMMARY.md").read_text())

    def test_logcat_read_failure_does_not_skip_next_device_or_report(self):
        devices = [Mock(serial="first"), Mock(serial="second")]
        with tempfile.TemporaryDirectory() as parent:
            session = module.Session(Path(parent) / "run", devices, "capture", "test")
            (session.folder / "device-1" / "logcat.txt").mkdir(parents=True)
            session.report["status"] = "captured_not_evaluated"
            session.finish()
            devices[1].snapshot.assert_called_once()
            self.assertEqual(session.report["status"], "incomplete")
            self.assertTrue(any(error.startswith("Logcat summary:") for error in session.report["errors"]))


class PairOrchestrationTests(unittest.TestCase):
    class FakeDevice:
        def __init__(self, name, network):
            self.name, self.network, self.enabled, self.store = name, network, False, {}
            network.append(self)

        def mesh(self, enabled):
            self.enabled = enabled
            self.sync()

        def sync(self):
            live = [device for device in self.network if device.enabled]
            shared = {key: value for device in live for key, value in device.store.items()}
            for device in live:
                device.store.update(shared)

        def post(self, marker):
            entry = {"id": hashlib.sha256(marker.encode()).hexdigest(), "author": self.name}
            self.store[entry["id"]] = entry
            self.sync()
            return entry

        def wait_entry(self, *, entry_id, timeout=60):
            if entry_id not in self.store:
                raise RuntimeError("missing exact entry")
            return self.store[entry_id]

        def restart(self):
            self.enabled = False

    def test_pair_checks_both_directions_and_catch_up_after_restart(self):
        network = []
        a, b = self.FakeDevice("a", network), self.FakeDevice("b", network)
        with tempfile.TemporaryDirectory() as parent:
            session = module.Session(Path(parent) / "run", [a, b], "pair", "test")
            module.smoke(session, 1)
            self.assertEqual(a.store, b.store)
            self.assertEqual(len(a.store), 3)
            self.assertTrue(all(row["status"] == "passed" for row in session.report["steps"]))

    def test_missing_delivery_fails_the_pair_run(self):
        a, b = self.FakeDevice("a", []), self.FakeDevice("b", [])
        with tempfile.TemporaryDirectory() as parent:
            session = module.Session(Path(parent) / "run", [a, b], "pair", "test")
            with self.assertRaisesRegex(RuntimeError, "missing exact entry"):
                module.smoke(session, 1)
            self.assertEqual(session.report["steps"][-1]["status"], "failed")


class ScenarioTests(unittest.TestCase):
    class Device(PairOrchestrationTests.FakeDevice):
        def __init__(self, name, network):
            super().__init__(name, network)
            self.radio = "ON"
            self.text = ""
            self.restored = False

        def require_empty_draft(self):
            if self.text:
                raise RuntimeError("existing draft")

        def bluetooth_state(self):
            return self.radio

        def bluetooth(self, enabled, control):
            self.radio = "ON" if enabled else "OFF"
            if not enabled:
                self.enabled = False
            return {"state": self.radio}

        def radio_off_error(self):
            if self.radio != "OFF" or self.enabled:
                raise RuntimeError("missing radio error")
            return {"mesh": False, "bluetooth": "OFF"}

        def rotation_settings(self):
            return {"user_rotation": "null", "accelerometer_rotation": "1"}

        def rotate(self, value):
            return {"observed_rotation": value}

        def enter_draft(self, marker):
            self.require_empty_draft()
            self.text = marker

        def draft(self):
            return {"text": self.text}

        def restore_rotation(self, settings):
            self.restored = True

        def discard_draft(self, marker):
            if self.text and self.text != marker:
                raise RuntimeError("changed draft preserved")
            self.text = ""

    def session(self, devices, command):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        return module.Session(Path(temporary.name) / "run", devices, command, "test")

    def test_bluetooth_each_device_recovers_and_restores(self):
        network = []
        a, b = self.Device("a", network), self.Device("b", network)
        session = self.session([a, b], "bluetooth-recovery")
        module.bluetooth_recovery(session, 1, "shell")
        self.assertEqual(len(a.store), 4)
        self.assertEqual(a.store, b.store)
        self.assertEqual((a.radio, b.radio), ("ON", "ON"))
        self.assertEqual(sum(row["name"] == "Verify radio-off error and stopped mesh"
                             for row in session.report["steps"]), 2)

    def test_bluetooth_delivery_failure_and_interrupt_restore_both_radios(self):
        for error in (RuntimeError("missing exact entry"), KeyboardInterrupt()):
            with self.subTest(error=type(error).__name__):
                network = []
                a, b = self.Device("a", network), self.Device("b", network)
                b.post = Mock(side_effect=error)
                session = self.session([a, b], "bluetooth-recovery")
                with self.assertRaises(type(error)):
                    module.bluetooth_recovery(session, 1, "shell")
                self.assertEqual((a.radio, b.radio), ("ON", "ON"))

    def test_bluetooth_missing_delivery_fails_and_restores(self):
        a, b = self.Device("a", []), self.Device("b", [])
        session = self.session([a, b], "bluetooth-recovery")
        with self.assertRaisesRegex(RuntimeError, "missing exact entry"):
            module.bluetooth_recovery(session, 1, "shell")
        self.assertEqual((a.radio, b.radio), ("ON", "ON"))

    def test_rotation_interruption_restores_settings(self):
        device = self.Device("a", [])
        device.rotate = Mock(side_effect=[{}, KeyboardInterrupt()])
        with self.assertRaises(KeyboardInterrupt):
            module.draft_rotation(self.session([device], "draft-rotation"))
        self.assertTrue(device.restored)
        self.assertEqual(device.text, "")

    def test_bluetooth_requires_initial_on_without_mutation(self):
        device = self.Device("a", [])
        device.radio = "OFF"
        device.bluetooth = Mock()
        with self.assertRaisesRegex(RuntimeError, "initially"):
            module.bluetooth_recovery(self.session([device], "bluetooth-recovery"), 1, "shell")
        device.bluetooth.assert_not_called()

    def test_rotation_preserves_existing_draft(self):
        device = self.Device("a", [])
        device.text = "existing test draft"
        with self.assertRaisesRegex(RuntimeError, "existing draft"):
            module.draft_rotation(self.session([device], "draft-rotation"))
        self.assertEqual(device.text, "existing test draft")
        self.assertFalse(device.restored)

    def test_rotation_restores_settings_and_clears_only_synthetic_draft(self):
        device = self.Device("a", [])
        module.draft_rotation(self.session([device], "draft-rotation"))
        self.assertTrue(device.restored)
        self.assertEqual(device.text, "")

    def test_rotation_changed_draft_preserved_and_cleanup_error_reported(self):
        device = self.Device("a", [])
        def rotate(value):
            if value == 1:
                device.text = "changed test draft"
        device.rotate = rotate
        session = self.session([device], "draft-rotation")
        with self.assertRaisesRegex(RuntimeError, "not retained"):
            module.draft_rotation(session)
        self.assertTrue(device.restored)
        self.assertEqual(device.text, "changed test draft")
        self.assertTrue(session.report["errors"])

    def test_batch_posts_before_remote_waits_and_checks_every_id_after_restart(self):
        network = []
        a, b = self.Device("a", network), self.Device("b", network)
        for device in (a, b):
            device.wait_entry = Mock(wraps=device.wait_entry)
            device.restart = Mock(wraps=device.restart)
        session = self.session([a, b], "batch")
        module.batch(session, 1, 5)
        self.assertEqual(len(a.store), 10)
        for device in (a, b):
            self.assertEqual(device.wait_entry.call_count, 20)
            device.restart.assert_called_once()
        names = [row["name"] for row in session.report["steps"]]
        self.assertLess(max(i for i, name in enumerate(names) if name.startswith("Post ")),
                        min(i for i, name in enumerate(names) if name.startswith("Verify ")))

    def test_batch_missing_entry_fails(self):
        a, b = self.Device("a", []), self.Device("b", [])
        with self.assertRaisesRegex(RuntimeError, "missing exact entry"):
            module.batch(self.session([a, b], "batch"), 1, 2)


class ControlTests(unittest.TestCase):
    def test_post_preserves_changed_draft_without_pressing_post(self):
        for draft in ("changed draft", "", "synthetic-and-more"):
            with self.subTest(draft=draft):
                device = module.Device("test")
                device.draft = Mock(return_value=module.ET.Element("node", text=""))
                device.shell = Mock()
                root = module.ET.Element("hierarchy")
                module.ET.SubElement(root, "node", {"class": "android.widget.EditText", "text": draft})
                module.ET.SubElement(root, "node", {"text": "Post to board"})
                device.ui = Mock(return_value=(root, ""))
                device.tap = Mock()
                with self.assertRaisesRegex(RuntimeError, "Draft changed"):
                    device.post("synthetic")
                device.tap.assert_called_once()

    def test_post_accepts_only_exact_synthetic_draft(self):
        device = module.Device("test")
        device.draft = Mock(return_value=module.ET.Element("node", text=""))
        device.shell = Mock()
        root = module.ET.fromstring('<hierarchy><node class="android.widget.EditText" text="synthetic"/><node text="Post to board"/></hierarchy>')
        device.ui = Mock(return_value=(root, ""))
        device.tap = Mock()
        device.wait_entry = Mock(return_value={"id": "entry"})
        self.assertEqual(device.post("synthetic"), {"id": "entry"})
        self.assertEqual(device.tap.call_count, 2)

    def test_restart_preserves_unsent_draft(self):
        device = module.Device("test")
        device.draft = Mock(return_value=module.ET.Element("node", text="unsent draft"))
        device.shell = Mock()
        with self.assertRaisesRegex(RuntimeError, "Unsent draft"):
            device.restart()
        device.shell.assert_not_called()

    def test_launch_rejects_non_owner_or_unknown_user_before_mutation(self):
        device = module.Device("test")
        for user in ("10", "", "unknown"):
            with self.subTest(user=user):
                device.shell = Mock(return_value=user)
                with self.assertRaisesRegex(RuntimeError, "owner profile"):
                    device.launch()
                device.shell.assert_called_once_with("am", "get-current-user")

    def test_prepare_pins_install_and_permissions_to_owner(self):
        device = module.Device("test")
        device.shell = Mock(side_effect=lambda *args: "35" if args[:1] == ("getprop",) else "0")
        device.adb = Mock()
        device.prepare(Path("test.apk"))
        device.adb.assert_called_once_with("install", "--user", "0", "-r", "test.apk", timeout=120)
        grants = [call.args for call in device.shell.call_args_list if call.args[:2] == ("pm", "grant")]
        self.assertEqual(len(grants), 4)
        self.assertTrue(all(args[2:5] == ("--user", "0", module.PACKAGE) for args in grants))

    def test_bluetooth_state_accepts_oem_label_capitalization(self):
        device = module.Device("test")
        for text, expected in (("Bluetooth Status\n  State:         ON\n", "ON"),
                               ("  state: OFF\n", "OFF")):
            device.shell = Mock(return_value=text)
            self.assertEqual(device.bluetooth_state(), expected)

    def test_bluetooth_state_rejects_missing_and_ambiguous_states(self):
        device = module.Device("test")
        for text in ("enabled: true", "State: ON\nState: OFF"):
            device.shell = Mock(return_value=text)
            with self.assertRaisesRegex(RuntimeError, "actual Bluetooth"):
                device.bluetooth_state()

    def test_shell_exit_success_does_not_prove_bluetooth_state(self):
        device = module.Device("test")
        device.bluetooth_state = Mock(return_value="ON")
        device.shell = Mock(return_value="")
        with patch.object(module.time, "monotonic", side_effect=[0, 16]):
            with self.assertRaisesRegex(RuntimeError, "did not reach"):
                device.bluetooth(False, "shell")
        device.shell.assert_called_once_with("svc", "bluetooth", "disable")

    def test_settings_ambiguous_switches_are_not_tapped(self):
        device = module.Device("test")
        device.bluetooth_state = Mock(return_value="ON")
        device.shell = Mock()
        node = '<node package="com.android.settings" resource-id="com.android.settings:id/switch_widget" checkable="true" />'
        device.raw_ui = Mock(return_value=(module.ET.fromstring('<hierarchy>' + node * 2 + '</hierarchy>'), ""))
        device.tap = Mock()
        with self.assertRaisesRegex(RuntimeError, "uniquely"):
            device.bluetooth(False, "settings")
        device.tap.assert_not_called()

    def test_settings_explicit_control_uses_switch_and_observes_state(self):
        device = module.Device("test")
        device.bluetooth_state = Mock(side_effect=["ON", "OFF"])
        device.shell = Mock()
        device.launch = Mock()
        node = '<hierarchy><node package="com.android.settings" resource-id="com.android.settings:id/switch_widget" checkable="true" checked="true" /></hierarchy>'
        device.raw_ui = Mock(return_value=(module.ET.fromstring(node), node))
        device.tap = Mock()
        self.assertEqual(device.bluetooth(False, "settings")["state"], "OFF")
        device.tap.assert_called_once()
        self.assertFalse(any(call.args[:2] == ("svc", "bluetooth") for call in device.shell.call_args_list))

    def test_rotation_settings_without_observed_rotation_fail(self):
        device = module.Device("test")
        device.shell = Mock()
        device.ui = Mock(return_value=(module.ET.fromstring('<hierarchy rotation="0" />'), ""))
        with self.assertRaisesRegex(RuntimeError, "Display did not"):
            device.rotate(1)

    def test_rotation_restores_absent_settings_with_delete(self):
        device = module.Device("test")
        device.shell = Mock(side_effect=["", "null", "", "1"])
        device.restore_rotation({"user_rotation": "null", "accelerometer_rotation": "1"})
        self.assertIn(unittest.mock.call("settings", "delete", "system", "user_rotation"), device.shell.call_args_list)

    def test_cleanup_failure_prevents_pass(self):
        with tempfile.TemporaryDirectory() as parent:
            session = module.Session(Path(parent) / "run", [], "draft-rotation", "test")
            module.cleanup(session, "restore", Mock(side_effect=RuntimeError("failed")))
            session.report["status"] = "passed"
            session.finish()
            self.assertEqual(session.report["status"], "incomplete")

    def test_cli_rejects_invalid_selection_and_flags_before_adb(self):
        commands = [["batch"], ["bluetooth-recovery", "--serial", "a"],
                    ["batch", "--serial", "a", "--serial", "a"],
                    ["batch", "--count", "0"], ["batch", "--count", "21"],
                    ["draft-rotation", "--count", "2"],
                    ["pair", "--bluetooth-control", "settings"]]
        with patch.object(module, "connected") as connected:
            for argv in commands:
                with self.subTest(argv=argv), self.assertRaises(SystemExit):
                    module.main(argv)
            connected.assert_not_called()

    def test_cli_rejects_other_profile_before_creating_or_starting_session(self):
        with patch.object(module, "connected", return_value={"test": "device"}), \
                patch.object(module.Device, "require_owner_user", side_effect=RuntimeError("owner profile required")), \
                patch.object(module, "Session") as session:
            with self.assertRaises(SystemExit):
                module.main(["offline", "--serial", "test"])
            session.assert_not_called()

    def test_output_guard_rejects_unignored_repository_path(self):
        with self.assertRaisesRegex(ValueError, "git-ignored"):
            module.validate_output(module.ROOT / "docs" / "device-evidence")
        module.validate_output(module.ROOT / "test-runs" / "unit-test-evidence")


if __name__ == "__main__":
    unittest.main()
