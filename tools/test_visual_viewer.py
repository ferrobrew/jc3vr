import http.client
import json
import os
import shutil
import subprocess
import tempfile
import threading
import unittest
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from pathlib import Path
from unittest.mock import patch

from tools.test_visual_suite import capture_wire, native_capture_wire, png
from tools.visual_protocol import Capture, Mode, RunOutcome
from tools.visual_viewer import (
    DEFAULT_POINTER,
    MAX_CAPTURE_JSON_BYTES,
    PAGE,
    SCRIPT,
    RunPointer,
    SnapshotState,
    ViewerError,
    ViewerState,
    _Handler,
    _read_json,
    create_server,
    main,
    publish_run,
    read_capture_json,
)
from tools.visual_viewer_history import (
    MAX_JSON_BYTES,
    RunId,
    history_path,
    read_history,
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
        with (patch("tools.visual_viewer_history.os.replace", side_effect=OSError("replacement refused")),
              self.assertRaises(OSError)):
            publish_run(self.root / "run-b", self.pointer)
        self.assertEqual(self.pointer.read_bytes(), old)
        self.assertEqual(sorted(path.name for path in self.root.iterdir()),
                         ["latest.history.json", "latest.history.lock", "latest.json", "run-a"])
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

    def test_large_completed_capture_acknowledgements_are_accepted(self):
        self.capture(Mode.RIGHT_ONLY, "old", ordinal=1)
        for ordinal, (mode, size) in enumerate(((Mode.MULTIPASS, 284 * 1024),
                                               (Mode.SINGLE_PASS, 347 * 1024),
                                               (Mode.MULTIPASS, MAX_CAPTURE_JSON_BYTES)), start=2):
            with self.subTest(mode=mode, size=size):
                wire = self.capture(mode, "large", ordinal=ordinal)
                wire["diagnostic"] = ""
                wire["diagnostic"] = "x" * (size - len(json.dumps(wire).encode("utf-8")))
                ack = self.folder / "large.json"
                ack.write_text(json.dumps(wire))
                self.assertGreater(ack.stat().st_size, MAX_JSON_BYTES)
                self.assertEqual(ack.stat().st_size, size)
                self.assertEqual(read_capture_json(ack), wire)
                snapshot = self.viewer.snapshot()
                self.assertIs(snapshot.state, SnapshotState.READY, snapshot.error)
                self.assertEqual(snapshot.preview.capture.id, "large")
                self.assertEqual(snapshot.preview.capture.mode, mode)
                self.assertEqual(snapshot.preview.capture.frame_ordinal, ordinal)

    def test_oversized_capture_acknowledgement_falls_back(self):
        self.capture(Mode.RIGHT_ONLY, "old", ordinal=1)
        previous = self.viewer.snapshot()
        wire = self.capture(Mode.MULTIPASS, "oversized", ordinal=2)
        wire["diagnostic"] = ""
        wire["diagnostic"] = "x" * (MAX_CAPTURE_JSON_BYTES + 1 - len(json.dumps(wire).encode("utf-8")))
        ack = self.folder / "oversized.json"
        ack.write_text(json.dumps(wire))
        self.assertEqual(ack.stat().st_size, MAX_CAPTURE_JSON_BYTES + 1)
        with self.assertRaises(ViewerError):
            read_capture_json(ack)
        snapshot = self.viewer.snapshot()
        self.assertIs(snapshot.state, SnapshotState.READY, snapshot.error)
        self.assertEqual(snapshot.preview, previous.preview)
        self.assertEqual(snapshot.preview.capture.id, "old")

    def test_capture_json_requires_an_object(self):
        ack = self.folder / "invalid.json"
        for wire in ([], None, "completed", 1, True):
            with self.subTest(wire=wire):
                ack.write_text(json.dumps(wire))
                with self.assertRaisesRegex(ViewerError, "expected a JSON object"):
                    read_capture_json(ack)

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
        self.assertIsNotNone(self.viewer.image(first.preview.revision))
        self.assertIsNone(self.viewer._last_key)
        self.assertIsNone(self.viewer._last_preview)
        other_folder.mkdir(parents=True)
        self.capture(Mode.RIGHT_ONLY, folder=other_folder)
        second = self.viewer.snapshot()
        self.assertEqual(second.run, str(other))
        self.assertNotEqual(first.preview.revision, second.preview.revision)
        self.assertIn("Physical right", second.preview.labels[0])
        self.assertIsNotNone(self.viewer.image(first.preview.revision))
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
            self.assertEqual(self.request(server, snapshot["preview"]["image_url"])[0], 200)
            self.assertEqual(self.request(server, next_snapshot["preview"]["image_url"])[0], 200)

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
const elements = Object.fromEntries(['status', 'preview', 'capture', 'labels', 'details', 'summary', 'runs', 'groups',
  'samples', 'follow', 'previous', 'next', 'previous-group', 'next-group'].map(
  name => [name, {hidden: name === 'preview', replaceWith() {}, replaceChildren() {}}]));
global.document = {
  getElementById: name => elements[name],
  createElement: () => ({}),
};
let scheduled;
global.setTimeout = callback => { scheduled = callback; };
global.clearTimeout = () => {};
global.localStorage = {getItem: () => null, setItem() {}};
let currentRun = 'first';
let rejectDecode = false;
let verifyHiddenBeforeDecode = false;
global.fetch = async url => ({ok: true, json: async () => url === '/api/runs' ?
  {runs: [], latest_run_id: null} : ({
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

    def test_history_order_timestamps_deduplication_and_atomic_failure(self):
        publish_run(self.run, self.pointer, started_at="2026-01-02T03:04:05+00:00")
        older = self.root / "older"
        publish_run(older, self.pointer, started_at="2025-01-01T00:00:00+00:00")
        history = read_history(self.pointer)
        self.assertEqual([entry.output for entry in history], [str(self.run), str(older)])
        self.assertEqual(history[0].run_id, RunId.for_output(self.run))
        publish_run(self.run, self.pointer)
        self.assertEqual(read_history(self.pointer), history)
        old_index, old_pointer = history_path(self.pointer).read_bytes(), self.pointer.read_bytes()
        with patch("tools.visual_viewer_history.os.replace", side_effect=OSError("refused")), self.assertRaises(OSError):
            publish_run(self.root / "never-visible", self.pointer)
        self.assertEqual(history_path(self.pointer).read_bytes(), old_index)
        self.assertEqual(self.pointer.read_bytes(), old_pointer)
        replace = os.replace

        def fail_pointer(source, target):
            if target == self.pointer:
                raise OSError("pointer refused")
            replace(source, target)

        with patch("tools.visual_viewer_history.os.replace", side_effect=fail_pointer), self.assertRaises(OSError):
            publish_run(self.root / "registered", self.pointer)
        self.assertEqual(self.pointer.read_bytes(), old_pointer)
        self.assertIn(str(self.root / "registered"), [entry.output for entry in read_history(self.pointer)])
        self.assertFalse(any(path.name.startswith(".") for path in self.root.iterdir()))
        for stamp in ("yesterday", "2026-01-01T00:00:00", "2026-01-01T00:00:00+01:00", 5):
            with self.subTest(stamp=stamp), self.assertRaises(ViewerError):
                publish_run(self.run, self.pointer, started_at=stamp)

    def test_concurrent_publication_does_not_lose_runs_or_expose_partial_json(self):
        def publish(index):
            publish_run(self.root / f"history-{index}", self.pointer)
            self.assertIsInstance(json.loads(history_path(self.pointer).read_bytes())["runs"], list)
            self.assertIn(RunPointer.from_wire(json.loads(self.pointer.read_bytes())).output,
                          [entry.output for entry in read_history(self.pointer)])

        with ThreadPoolExecutor(max_workers=4) as pool:
            list(pool.map(publish, range(12)))
        self.assertEqual(len(read_history(self.pointer)), 13)

    def test_sample_groups_and_pinned_selection_remain_stable_during_rollover(self):
        self.capture(Mode.LEFT_ONLY, "suite_left_only_1", ordinal=1)
        self.capture(Mode.MULTIPASS, "suite_multipass_1", ordinal=2)
        self.capture(Mode.MULTIPASS, "suite_multipass_2", ordinal=3)
        self.capture(Mode.MULTIPASS, "suite_multipass_return_1", ordinal=4)
        self.capture(None, "smoke", ordinal=5)
        run_id = RunId.for_output(self.run)
        selected = self.viewer.snapshot(run_id, "suite_multipass_1")
        self.assertEqual(selected.preview.capture.id, "suite_multipass_1")
        self.assertEqual([group["id"] for group in selected.groups],
                         ["left_only", "multipass", "multipass_return", "native"])
        self.assertEqual([sample["id"] for sample in selected.groups[1]["samples"]],
                         ["suite_multipass_1", "suite_multipass_2"])
        self.capture(Mode.MULTIPASS, "suite_multipass_3", ordinal=6)
        self.assertEqual(len(self.viewer.snapshot(run_id, "suite_multipass_1").groups[1]["samples"]), 3)
        other = self.root / "run-b"
        folder = other / "config/captures"
        folder.mkdir(parents=True)
        self.capture(Mode.RIGHT_ONLY, "new", folder, 7)
        publish_run(other, self.pointer)
        self.assertEqual(self.viewer.snapshot().preview.capture.id, "new")
        pinned = self.viewer.snapshot(run_id, "suite_multipass_1")
        self.assertEqual(pinned.run, str(self.run))
        self.assertEqual(pinned.preview.revision, selected.preview.revision)
        (self.folder / "suite_multipass_1.capture.png").write_bytes(b"broken")
        self.assertEqual(self.viewer.snapshot(run_id, "suite_multipass_1").state, "error")
        self.assertEqual(self.viewer.snapshot().preview.capture.id, "new")

    def test_known_id_api_rejects_paths_unknown_ids_and_extra_fields(self):
        self.capture(Mode.LEFT_ONLY, "first")
        run_id = RunId.for_output(self.run)
        with self.server() as server:
            history = json.loads(self.request(server, "/api/runs")[2])
            self.assertEqual(history["latest_run_id"], run_id)
            self.assertEqual(history["runs"][0]["run_id"], run_id)
            selected = f"/api/status?run_id={run_id}&capture_id=first"
            snapshot = json.loads(self.request(server, selected)[2])
            self.assertEqual(snapshot["preview"]["capture"]["id"], "first")
            for query in ("run_id=/etc", "run_id=" + "f" * 64,
                          f"run_id={run_id}&capture_id=../first", f"run_id={run_id}&capture_id=unknown",
                          f"run_id={run_id}&capture_id=%2Fetc%2Fpasswd", "capture_id=first",
                          f"run_id={run_id}&run_id={run_id}", "run_id=", "output=/etc", "path=/etc",
                          f"run_id={run_id}&capture_id=first&path=/etc"):
                with self.subTest(query=query):
                    self.assertEqual(self.request(server, "/api/status?" + query)[0], 404)
            self.assertEqual(self.request(server, "/api/runs", headers={"Origin": "null"})[0], 403)
            self.assertEqual(self.request(server, "/api/runs?output=/etc")[0], 404)
        with patch("tools.visual_viewer._read_json", side_effect=AssertionError("read outside registry")):
            self.assertEqual(self.viewer.snapshot("/etc/passwd").state, "error")
        (self.folder / "first.json").unlink()
        self.assertIn("unknown capture ID", self.viewer.snapshot(run_id, "first").error)

    def test_history_validation_and_explicit_run_without_registry(self):
        self.capture()
        history = history_path(self.pointer)
        original = history.read_bytes()
        for data in (b"{", b"[]", b"x" * (MAX_JSON_BYTES + 1), b'{"version":true,"runs":[]}',
                     b'{"version":1,"runs":[{"run_id":"bad","output":"/tmp","timestamp":"bad"}]}'):
            with self.subTest(data=data[:30]):
                history.write_bytes(data)
                self.assertEqual(self.viewer.snapshot().state, "error")
        history.write_bytes(original)
        history.unlink()
        self.pointer.unlink()
        viewer = ViewerState(run=self.run, pointer=self.pointer)
        self.assertEqual(viewer.snapshot().state, "ready")
        entries = viewer.runs()
        run_id = entries.runs[0].run_id
        self.assertEqual(viewer.snapshot(run_id, "sample").state, "ready")
        self.assertEqual(viewer.snapshot("f" * 64).state, "error")

    def test_summary_exposes_comparison_failure_without_hiding_capture(self):
        self.capture()
        (self.run / "result.json").write_text(json.dumps({"status": "completed", "smoke_succeeded": True,
                                                       "comparison": {"status": "fail"}}))
        folder = self.run / "comparison"
        folder.mkdir()
        (folder / "report.json").write_text(json.dumps({"status": "fail", "cases": [
            {"name": "gameplay", "status": "fail", "comparisons": [
                {"name": "multipass vs single_pass", "status": "fail", "errors": ["Geometry mismatch"]}]}]}))
        snapshot = self.viewer.snapshot()
        self.assertEqual(snapshot.state, "ready")
        self.assertEqual(snapshot.report.comparison, "fail")
        self.assertTrue(any("Geometry mismatch" in error for error in snapshot.report.comparison_failures))
        self.assertEqual(self.viewer.runs().runs[0].report.comparison, "fail")

    def test_registry_reads_pointer_before_history_during_publication(self):
        original = self.pointer.read_bytes()
        original_read = self.viewer.runs().latest_run_id
        read_json = _read_json
        other = self.root / "run-b"

        def read_and_publish(path):
            value = read_json(path)
            if path == self.pointer:
                publish_run(other, self.pointer)
            return value

        with patch("tools.visual_viewer._read_json", side_effect=read_and_publish):
            history = self.viewer.runs()
        self.assertEqual(history.latest_run_id, original_read)
        self.assertIn(history.latest_run_id, [entry.run_id for entry in history.runs])
        self.pointer.write_bytes(original)
        history_path(self.pointer).unlink()
        legacy = self.viewer.runs()
        self.assertEqual(legacy.latest_run_id, legacy.runs[0].run_id)
        self.assertEqual(self.viewer.snapshot(legacy.latest_run_id).state, "waiting")
        publish_run(other, self.pointer)
        self.pointer.write_bytes(original)
        with self.assertRaisesRegex(ViewerError, "absent from the history"):
            self.viewer.runs()

    def test_group_outcomes_include_repeatability_cross_metrics_and_reused_aggregate(self):
        self.capture(Mode.MULTIPASS, "suite_multipass_1", ordinal=1)
        self.capture(Mode.SINGLE_PASS, "suite_single_pass_1", ordinal=2)
        self.capture(Mode.MULTIPASS, "suite_multipass_return_1", ordinal=3)
        folder = self.run / "comparison"
        folder.mkdir()
        cross = {"name": "multipass vs single_pass, eye 0", "status": "fail",
                 "worst_metrics": {"mean": 1.25, "changed_fraction": 0.125}}
        (folder / "report.json").write_text(json.dumps({"cases": [{"modes": [
            {"mode": "multipass", "status": "pass", "repeatability": [
                {"name": "multipass eye 0 repeatability", "status": "pass"}]},
            {"mode": "single_pass", "status": "pass", "repeatability": []}],
            "comparisons": [cross, {"name": "left_only vs right_only, eye 0", "status": "fail"}]}]}))
        snapshot = self.viewer.snapshot()
        groups = {group["id"]: group for group in snapshot.groups}
        outcome = groups["multipass"]["outcome"]
        self.assertFalse(outcome["reused_aggregate"])
        self.assertEqual(outcome["repeatability"][0]["status"], "pass")
        self.assertEqual(len(outcome["comparisons"]), 1)
        self.assertEqual(outcome["comparisons"][0]["mean_absolute_error"], 1.25)
        self.assertEqual(outcome["comparisons"][0]["changed_fraction"], 0.125)
        reused = groups["multipass_return"]["outcome"]
        self.assertTrue(reused["reused_aggregate"])
        self.assertEqual(reused["mode"], "multipass")
        self.assertEqual(reused["comparisons"], outcome["comparisons"])
        self.assertEqual(groups["single_pass"]["outcome"]["comparisons"], outcome["comparisons"])
        (folder / "report.json").write_text("{")
        self.assertTrue(all(group["outcome"] is None for group in self.viewer.snapshot().groups))

    @unittest.skipUnless(shutil.which("chromium"), "Chromium is unavailable")
    def test_browser_history_navigation_pin_reload_and_follow_rollover(self):
        self.capture(Mode.LEFT_ONLY, "suite_left_only_1", ordinal=1)
        self.capture(Mode.MULTIPASS, "suite_multipass_1", ordinal=2)
        self.capture(Mode.MULTIPASS, "suite_multipass_2", ordinal=3)
        self.capture(Mode.MULTIPASS, "suite_multipass_return_1", ordinal=4)
        other = self.root / "run-b"
        folder = other / "config/captures"
        folder.mkdir(parents=True)
        self.capture(Mode.RIGHT_ONLY, "new_right", folder=folder, ordinal=5)
        browser_script = r"""
(async () => {
  const get = id => document.getElementById(id);
  const check = (value, message) => { if (!value) throw new Error(message); };
  const wait = async predicate => {
    for (let i = 0; i < 160; i++) {
      if (predicate()) return;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw new Error('Timed out: ' + get('status').textContent);
  };
  const capture = id => !get('preview').hidden && get('details').textContent.startsWith(id + ' |');
  const choose = (id, value) => {
    get(id).value = value;
    get(id).dispatchEvent(new Event('change'));
  };
  if (localStorage.getItem('browser-test-phase') === 'reload') {
    await wait(() => capture('suite_multipass_1'));
    check(!get('follow').checked, 'Reload loses pinned selection');
    get('follow').checked = true;
    get('follow').dispatchEvent(new Event('change'));
    await wait(() => capture('new_right'));
    check(get('labels').textContent.includes('Physical right'), 'Right label is lost');
    check(get('status').textContent.includes('Following latest'), 'Follow does not resume');
    localStorage.clear();
    document.body.textContent = 'BROWSER HISTORY PASS';
    return;
  }
  await wait(() => capture('suite_multipass_return_1'));
  check(get('follow').checked, 'Follow latest is not the default');
  choose('groups', 'left_only');
  await wait(() => capture('suite_left_only_1'));
  check(get('labels').textContent.includes('Physical left'), 'Left label is lost');
  get('next-group').click();
  await wait(() => capture('suite_multipass_1'));
  get('previous-group').click();
  await wait(() => capture('suite_left_only_1'));
  get('next-group').click();
  await wait(() => capture('suite_multipass_1'));
  get('next').click();
  await wait(() => capture('suite_multipass_2'));
  get('previous').click();
  await wait(() => capture('suite_multipass_1'));
  await fetch('/browser-test-rollover');
  await new Promise(resolve => setTimeout(resolve, 1200));
  check(capture('suite_multipass_1'), 'Polling loses pinned sample');
  check(get('runs').options.length === 2, 'New run is missing from history');
  choose('runs', get('runs').options[0].value);
  await wait(() => capture('new_right'));
  const oldRun = [...get('runs').options].find(option => option.textContent.includes('run-a'));
  choose('runs', oldRun.value);
  await wait(() => capture('suite_multipass_return_1'));
  choose('groups', 'multipass');
  await wait(() => capture('suite_multipass_1'));
  localStorage.setItem('browser-test-phase', 'reload');
  location.reload();
})().catch(error => { document.body.textContent = 'BROWSER HISTORY FAIL: ' + error.stack; });
"""
        original_get = _Handler.do_GET
        test = self

        def handle(handler):
            if handler.path == "/browser-test.js":
                handler._send(200, browser_script.encode(), "text/javascript")
            elif handler.path == "/browser-test-rollover":
                publish_run(other, test.pointer)
                handler._send(200, b"ok", "text/plain")
            elif handler.path == "/":
                page = PAGE.replace('</head>', '<script src="/browser-test.js" defer></script></head>')
                handler._send(200, page.encode(), "text/html")
            else:
                original_get(handler)

        with patch.object(_Handler, "do_GET", handle), self.server() as server:
            result = subprocess.run([
                shutil.which("chromium"), "--headless", "--no-sandbox", "--disable-gpu",
                "--disable-dev-shm-usage", "--no-proxy-server", "--no-first-run",
                "--user-data-dir=" + str(self.root / "browser-profile"), "--dump-dom",
                "--virtual-time-budget=20000", f"http://127.0.0.1:{server.server_port}/",
            ], text=True, capture_output=True, timeout=35, check=False)
        self.assertEqual(result.returncode, 0, result.stderr[-2000:])
        self.assertIn("BROWSER HISTORY PASS", result.stdout, result.stdout[-3000:] + result.stderr[-1500:])

    def test_cli_port_validation_without_launch(self):
        for port in ("0", "65536", "-1"):
            with self.subTest(port=port), patch("sys.stderr"), self.assertRaises(SystemExit):
                main(["--port", port])


if __name__ == "__main__":
    unittest.main()
