import hashlib
import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest

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


if __name__ == "__main__":
    unittest.main()
