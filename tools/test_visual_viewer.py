import http.client
import json
import os
import shutil
import subprocess
import tempfile
import threading
import unittest
from contextlib import contextmanager
from pathlib import Path
from unittest.mock import patch

from tools.test_visual_suite import capture_wire, native_capture_wire, png
from tools.visual_protocol import Capture, Mode, RunOutcome
from tools.visual_viewer import (
    DEFAULT_POINTER,
    MAX_JSON_BYTES,
    SCRIPT,
    RunPointer,
    SnapshotState,
    ViewerError,
    ViewerState,
    create_server,
    main,
    publish_run,
)


class ViewerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.pointer = self.root / "latest.json"
        self.run = self.root / "run-a"
        self.folder = self.run / "config/captures"
        self.folder.mkdir(parents=True)
        publish_run(self.run, self.pointer)
        self.viewer = ViewerState(pointer=self.pointer)

    def capture(self, mode=Mode.MULTIPASS, capture_id="sample", folder=None, ordinal=1):
        folder = folder or self.folder
        wire = capture_wire(mode, ordinal=ordinal) if mode is not None else native_capture_wire(ordinal)
        wire.update(id=capture_id, state="completed", png="/etc/passwd")
        (folder / f"{capture_id}.capture.png").write_bytes(png(wire["width"], wire["height"]))
        ack = folder / f"{capture_id}.json"
        ack.write_text(json.dumps(wire))
        os.utime(ack, ns=(ordinal * 1_000_000_000, ordinal * 1_000_000_000))
        return wire

    @contextmanager
    def server(self):
        with create_server(pointer=self.pointer, port=0) as server:
            thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.01})
            thread.start()
            try:
                yield server
            finally:
                server.shutdown()
                thread.join(timeout=5)
                self.assertFalse(thread.is_alive())

    def request(self, server, path="/api/status", headers=None, method="GET"):
        connection = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=5)
        try:
            connection.request(method, path, headers=headers or {})
            response = connection.getresponse()
            return response.status, dict(response.getheaders()), response.read()
        finally:
            connection.close()

    def test_pointer_is_atomic_versioned_and_has_resolved_identity(self):
        self.assertEqual(DEFAULT_POINTER.name, "latest.json")
        pointer = publish_run(self.run / ".." / "run-a", self.pointer)
        self.assertEqual(pointer, RunPointer(str(self.run.resolve())))
        self.assertEqual(RunPointer.from_wire(json.loads(self.pointer.read_bytes())), pointer)
        old = self.pointer.read_bytes()
        with (patch("tools.visual_viewer.os.replace", side_effect=OSError("replacement refused")),
              self.assertRaises(OSError)):
            publish_run(self.root / "run-b", self.pointer)
        self.assertEqual(self.pointer.read_bytes(), old)
        self.assertEqual(sorted(path.name for path in self.root.iterdir()), ["latest.json", "run-a"])
        for wire in ({"version": 2, "output": str(self.run)}, {"version": True, "output": str(self.run)},
                     {"version": 1, "output": "relative"}, {"version": 1, "output": []}):
            with self.subTest(wire=wire), self.assertRaises(ViewerError):
                RunPointer.from_wire(wire)

    def test_mono_physical_labels_and_stereo_modes(self):
        for ordinal, mode in enumerate((*Mode, None), start=1):
            with self.subTest(mode=mode):
                self.capture(mode, f"capture_{ordinal}", ordinal=ordinal)
                snapshot = self.viewer.snapshot()
                self.assertEqual(snapshot.state, "ready", snapshot.error)
                if mode is not None:
                    self.assertEqual(snapshot.preview.capture.mode, mode)
                selected = mode.selected_eye if mode else None
                if selected is None:
                    self.assertEqual(snapshot.preview.labels, ("L - left eye", "R - right eye"))
                else:
                    self.assertEqual(len(snapshot.preview.labels), 1)
                    self.assertIn("Physical left" if selected == 0 else "Physical right", snapshot.preview.labels[0])
                self.assertEqual(len(self.viewer.image(snapshot.preview.revision)),
                                 len(png(snapshot.preview.capture.width, 1)))
                if mode:
                    self.assertIsInstance(snapshot.preview.capture, Capture)

    def test_latest_completed_ack_only_and_png_derived_from_id(self):
        self.capture(Mode.RIGHT_ONLY, "first", ordinal=1)
        (self.folder / "incomplete.json").write_text('{"id":"incomplete","state":"pending"}')
        (self.folder / "failed.json").write_text('{"id":"failed","state":"failed"}')
        (self.folder / "ignored.capture.json").write_text("not an acknowledgement")
        first = self.viewer.snapshot()
        self.assertEqual(first.preview.capture.id, "first")
        self.capture(Mode.MULTIPASS, "next", ordinal=2)
        self.assertEqual(self.viewer.snapshot().preview.capture.id, "next")

    def test_frame_ordinal_wins_over_mtime_and_invalid_newest_falls_back(self):
        self.capture(Mode.LEFT_ONLY, "old", ordinal=1)
        self.capture(Mode.RIGHT_ONLY, "new", ordinal=2)
        self.assertEqual(self.viewer.snapshot().preview.capture.id, "new")
        os.utime(self.folder / "old.json", ns=(100_000_000_000, 100_000_000_000))
        self.assertEqual(self.viewer.snapshot().preview.capture.id, "new")
        self.capture(Mode.MULTIPASS, "broken", ordinal=3)
        (self.folder / "broken.capture.png").write_bytes(b"invalid PNG")
        os.utime(self.folder / "broken.json", ns=(0, 0))
        self.assertEqual(self.viewer.snapshot().preview.capture.id, "new")
        (self.folder / "broken.capture.png").write_bytes(png(2, 1))
        self.assertEqual(self.viewer.snapshot().preview.capture.id, "broken")

    def test_ordinal_selection_reads_beyond_former_mtime_window(self):
        self.capture(Mode.RIGHT_ONLY, "newest_frame", ordinal=2)
        for index in range(130):
            self.capture(Mode.LEFT_ONLY, f"older_frame_{index}", ordinal=1)
            os.utime(self.folder / f"older_frame_{index}.json", ns=(10_000_000_000, 10_000_000_000))
        self.assertEqual(self.viewer.snapshot().preview.capture.id, "newest_frame")

    def test_report_status_unknown_not_started_started_and_final(self):
        self.assertIsNone(self.viewer.snapshot().report.status)
        for outcome in RunOutcome:
            with self.subTest(outcome=outcome):
                (self.run / "result.json").write_text(json.dumps({"status": outcome, "smoke_succeeded": False}))
                snapshot = self.viewer.snapshot()
                self.assertEqual(snapshot.state, "waiting")
                self.assertEqual(snapshot.report.status, outcome)
        self.capture()
        self.assertEqual(self.viewer.snapshot().report.status, RunOutcome.FAILED)
        self.assertEqual(self.viewer.snapshot().state, "ready")

    def test_report_errors_do_not_hide_valid_capture(self):
        self.capture()
        report = self.run / "result.json"
        for data in (b"x" * (MAX_JSON_BYTES + 1), b"{", b"[]", b"\xff",
                     b'{"status":"unknown"}', b'{"smoke_succeeded":"yes"}'):
            with self.subTest(data=data[:30]):
                report.write_bytes(data)
                snapshot = self.viewer.snapshot()
                self.assertIs(snapshot.state, SnapshotState.READY)
                self.assertEqual(snapshot.preview.capture.id, "sample")
                self.assertIsNone(snapshot.report.status)
                self.assertIn("visual viewer: run report unavailable:", snapshot.report.error)
                self.assertEqual(snapshot.to_wire()["state"], "ready")

    def test_run_change_and_cached_revision_never_relabel_old_bytes(self):
        self.capture(Mode.LEFT_ONLY)
        first = self.viewer.snapshot()
        self.assertIsNotNone(self.viewer.image(first.preview.revision))
        other = self.root / "run-b"
        other_folder = other / "config/captures"
        publish_run(other, self.pointer)
        self.assertEqual(self.viewer.snapshot().state, "waiting")
        self.assertIsNone(self.viewer.image(first.preview.revision))
        self.assertIsNone(self.viewer._last_key)
        self.assertIsNone(self.viewer._last_preview)
        other_folder.mkdir(parents=True)
        self.capture(Mode.RIGHT_ONLY, folder=other_folder)
        second = self.viewer.snapshot()
        self.assertEqual(second.run, str(other))
        self.assertNotEqual(first.preview.revision, second.preview.revision)
        self.assertIn("Physical right", second.preview.labels[0])
        self.assertIsNone(self.viewer.image(first.preview.revision))
        fixed = ViewerState(run=self.run, pointer=self.pointer).snapshot()
        self.assertIn("Physical left", fixed.preview.labels[0])
        self.assertIsNone(self.viewer.image("f" * 64))

    def test_identical_capture_in_new_run_changes_revision(self):
        self.capture()
        first = self.viewer.snapshot()
        other = self.root / "run-b"
        other_folder = other / "config/captures"
        other_folder.mkdir(parents=True)
        self.capture(folder=other_folder)
        publish_run(other, self.pointer)
        self.assertNotEqual(first.preview.revision, self.viewer.snapshot().preview.revision)

    def test_cache_is_bounded_and_stable_capture_skips_png_reread(self):
        self.capture(Mode.LEFT_ONLY)
        first = self.viewer.snapshot()
        with patch("tools.visual_viewer._png_dimensions", side_effect=AssertionError("unexpected decode")):
            self.assertEqual(self.viewer.snapshot(), first)
        for ordinal in range(2, 6):
            self.capture(Mode.MULTIPASS, f"capture_{ordinal}", ordinal=ordinal)
            self.assertEqual(self.viewer.snapshot().state, "ready")
        self.assertIsNone(self.viewer.image(first.preview.revision))

    def test_malformed_artifacts_are_bounded_and_do_not_crash(self):
        self.pointer.write_bytes(b"{" * (MAX_JSON_BYTES + 1))
        self.assertEqual(self.viewer.snapshot().state, "error")
        self.pointer.write_text("[]")
        self.assertEqual(self.viewer.snapshot().state, "error")
        self.pointer.write_text('{"version":1,"output":null}')
        self.assertEqual(self.viewer.snapshot().state, "error")
        publish_run(self.run, self.pointer)
        ack = self.folder / "sample.json"
        for data in (b"{", b"[]", b"\xff", b"{}", b"[" * 2000):
            with self.subTest(data=data[:10]):
                ack.write_bytes(data)
                self.assertIn(self.viewer.snapshot().state, ("waiting", "error"))
        wire = self.capture()
        for change in ({"id": "wrong"}, {"selected_eye": 0}, {"fresh_eyes": [False, False]},
                       {"width": 7}, {"mode": "unknown"}):
            with self.subTest(change=change):
                ack.write_text(json.dumps(wire | change))
                self.assertEqual(self.viewer.snapshot().state, "error")
        ack.write_text(json.dumps(wire))
        image_path = self.folder / "sample.capture.png"
        valid = image_path.read_bytes()
        for data in (b"not PNG", valid[:-1], valid + b"trailing", png(1, 1),
                     valid[:40] + bytes([valid[40] ^ 1]) + valid[41:]):
            with self.subTest(data=data[:10]):
                image_path.write_bytes(data)
                self.assertEqual(self.viewer.snapshot().state, "error")

    def test_symlink_files_and_directory_escape_rejected(self):
        self.capture()
        image = self.folder / "sample.capture.png"
        outside = self.root / "outside.png"
        image.replace(outside)
        image.symlink_to(outside)
        self.assertEqual(self.viewer.snapshot().state, "error")
        image.unlink()
        image.write_bytes(outside.read_bytes())
        target = self.root / "external-captures"
        self.folder.rename(target)
        self.folder.symlink_to(target, target_is_directory=True)
        self.assertEqual(self.viewer.snapshot().state, "error")
        self.folder.unlink()
        self.folder.mkdir()
        (self.folder / "sample.json").symlink_to(target / "sample.json")
        self.assertEqual(self.viewer.snapshot().state, "waiting")

    def test_http_snapshot_revision_cache_and_headers(self):
        self.capture(Mode.RIGHT_ONLY)
        with self.server() as server:
            self.assertEqual(server.server_address[0], "127.0.0.1")
            status, headers, body = self.request(server)
            self.assertEqual(status, 200)
            snapshot = json.loads(body)
            self.assertEqual(snapshot["preview"]["capture"]["selected_eye"], 1)
            self.assertIn("Physical right", snapshot["preview"]["labels"][0])
            self.assertEqual(headers["Cache-Control"], "no-store")
            self.assertIn("frame-ancestors 'none'", headers["Content-Security-Policy"])
            self.assertEqual(headers["X-Content-Type-Options"], "nosniff")
            status, headers, image = self.request(server, snapshot["preview"]["image_url"])
            self.assertEqual(status, 200)
            self.assertEqual(headers["Content-Type"], "image/png")
            self.assertEqual(image, png(1, 1))
            self.capture(Mode.MULTIPASS, "next", ordinal=2)
            next_snapshot = json.loads(self.request(server)[2])
            self.assertNotEqual(snapshot["preview"]["revision"], next_snapshot["preview"]["revision"])
            self.assertEqual(self.request(server, snapshot["preview"]["image_url"])[2], image)
            self.assertEqual(self.request(server, "/capture/" + "0" * 64 + ".png")[0], 404)
            for path in ("/", "/viewer.js", "/viewer.css"):
                self.assertEqual(self.request(server, path)[0], 200)
            self.assertIn(b"setTimeout(poll, 1000)", self.request(server, "/viewer.js")[2])
            self.assertIn(b"not video", self.request(server, "/")[2])
            publish_run(self.root / "run-without-captures", self.pointer)
            self.assertEqual(json.loads(self.request(server)[2])["state"], "waiting")
            self.assertEqual(self.request(server, snapshot["preview"]["image_url"])[0], 404)
            self.assertEqual(self.request(server, next_snapshot["preview"]["image_url"])[0], 404)

    def test_http_host_origin_and_no_filesystem_routes(self):
        with self.server() as server:
            local = f"127.0.0.1:{server.server_port}"
            for headers in ({"Host": "attacker.invalid"}, {"Host": "127.0.0.1:1"},
                            {"Origin": "https://attacker.invalid"}, {"Origin": "null"},
                            {"Origin": f"http://{local}.attacker.invalid"}, {"Sec-Fetch-Site": "cross-site"}):
                with self.subTest(headers=headers):
                    status, response_headers, _ = self.request(server, headers=headers)
                    self.assertEqual(status, 403)
                    self.assertNotIn("Access-Control-Allow-Origin", response_headers)
            self.assertEqual(self.request(server, headers={"Host": local, "Origin": f"http://{local}"})[0], 200)
            self.assertEqual(self.request(server, headers={"Host": f"localhost:{server.server_port}"})[0], 200)
            for path in ("/etc/passwd", "/../latest.json", "/%2e%2e/latest.json", "/api/status?run=/etc",
                         "/capture/sample.png", "/latest.json"):
                with self.subTest(path=path):
                    self.assertEqual(self.request(server, path)[0], 404)
            self.assertEqual(self.request(server, method="POST")[0], 405)
            connection = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=5)
            try:
                connection.putrequest("GET", "/api/status", skip_host=True)
                connection.putheader("Host", local)
                connection.putheader("Host", "attacker.invalid")
                connection.endheaders()
                response = connection.getresponse()
                self.assertEqual(response.status, 403)
                response.read()
            finally:
                connection.close()

    @unittest.skipUnless(shutil.which("node"), "Node is unavailable")
    def test_browser_hides_previous_run_before_decode_and_after_failure(self):
        fixture = r"""
const assert = require('node:assert/strict');
const elements = Object.fromEntries(['status', 'preview', 'capture', 'labels', 'details'].map(
  name => [name, {hidden: name === 'preview', replaceWith() {}, replaceChildren() {}}]));
global.document = {
  getElementById: name => elements[name],
  createElement: () => ({}),
};
let scheduled;
global.setTimeout = callback => { scheduled = callback; };
let currentRun = 'first';
let rejectDecode = false;
let verifyHiddenBeforeDecode = false;
global.fetch = async () => ({ok: true, json: async () => ({
  state: 'ready', run: currentRun, preview: {
    revision: currentRun, image_url: '/capture/' + currentRun + '.png', labels: ['L', 'R'],
    capture: {id: currentRun, frame_ordinal: 1},
  },
})});
global.Image = class {
  replaceWith() {}
  decode() {
    if (verifyHiddenBeforeDecode) assert.equal(elements.preview.hidden, true);
    return rejectDecode ? Promise.reject(new Error('decode rejected')) : Promise.resolve();
  }
};
"""
        checks = r"""
(async () => {
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(elements.preview.hidden, false);
  currentRun = 'second';
  rejectDecode = true;
  verifyHiddenBeforeDecode = true;
  await scheduled();
  assert.equal(elements.preview.hidden, true);
  assert.match(elements.status.textContent, /decode rejected/);
  assert.equal(revision, null);
  assert.equal(displayedRun, 'second');
  rejectDecode = false;
  await scheduled();
  assert.equal(elements.preview.hidden, false);
  global.fetch = async () => { throw new Error('offline'); };
  await scheduled();
  assert.equal(elements.preview.hidden, true);
  assert.equal(revision, null);
})().catch(error => { console.error(error); process.exitCode = 1; });
"""
        result = subprocess.run([shutil.which("node"), "-"], input=fixture + SCRIPT + checks,
                                text=True, capture_output=True, timeout=10, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_cli_port_validation_without_launch(self):
        for port in ("0", "65536", "-1"):
            with self.subTest(port=port), patch("sys.stderr"), self.assertRaises(SystemExit):
                main(["--port", port])


if __name__ == "__main__":
    unittest.main()
