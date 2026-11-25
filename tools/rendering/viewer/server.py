import argparse
import hashlib
import json
import logging
import math
import os
import re
import stat
import struct
import threading
import zlib
from collections import OrderedDict
from dataclasses import asdict, dataclass, field
from datetime import UTC, datetime
from enum import StrEnum
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

from tools.rendering.protocol import (
    Capture,
    Command,
    NativeCapture,
    Outcome,
    ProtocolError,
    RunOutcome,
    ScreenshotAck,
    acknowledgement_from_wire,
)
from tools.rendering.viewer.history import (
    RunEntry,
    RunId,
    RunPointer,
    ViewerError,
    read_history,
)
from tools.rendering.viewer.history import (
    publish_run as _publish_run,
)
from tools.rendering.viewer.history import (
    read_bounded as _read_bounded,
)
from tools.rendering.viewer.history import (
    read_json as _read_json,
)

DEFAULT_POINTER = Path(__file__).resolve().parents[3] / "target/visual-harness/latest.json"


@dataclass(frozen=True)
class CapturePreview:
    revision: str
    image_url: str
    labels: tuple[str, ...]
    capture: NativeCapture | Capture


@dataclass(frozen=True)
class RunSummary:
    status: RunOutcome | None = None
    smoke_succeeded: bool | None = None
    error: str | None = None
    comparison: Outcome | None = None
    comparison_failures: tuple[str, ...] = ()


@dataclass(frozen=True)
class GroupComparison:
    name: str
    status: Outcome
    mean_absolute_error: float | None = None
    changed_fraction: float | None = None


@dataclass(frozen=True)
class GroupOutcome:
    mode: str
    status: Outcome
    reused_aggregate: bool
    repeatability: tuple[GroupComparison, ...]
    comparisons: tuple[GroupComparison, ...]


@dataclass(frozen=True)
class RunListing:
    run_id: RunId
    output: str
    timestamp: str
    report: RunSummary


@dataclass(frozen=True)
class HistorySnapshot:
    runs: tuple[RunListing, ...]
    latest_run_id: RunId | None
    version: int = 1

    def to_wire(self):
        return asdict(self)


class SnapshotState(StrEnum):
    WAITING = "waiting"
    READY = "ready"
    ERROR = "error"


@dataclass(frozen=True)
class ViewerSnapshot:
    state: SnapshotState
    run: str | None = None
    preview: CapturePreview | None = None
    error: str | None = None
    version: int = 1
    report: RunSummary | None = None
    run_id: RunId | None = None
    timestamp: str | None = None
    groups: list[dict] = field(default_factory=list)

    def to_wire(self):
        return asdict(self)


def publish_run(output: Path, pointer: Path = DEFAULT_POINTER, *,
                started_at: datetime | str | None = None) -> RunPointer:
    return _publish_run(output, pointer, started_at=started_at)


def read_capture_json(path: Path) -> dict[str, object]:
    wire = json.loads(_read_bounded(path, MAX_CAPTURE_JSON_BYTES))
    if not isinstance(wire, dict):
        raise ViewerError("visual viewer: expected a JSON object")
    return wire


class ViewerState:
    def __init__(self, run: Path | None = None, pointer: Path = DEFAULT_POINTER):
        self.run = Path(run).expanduser().resolve() if run is not None else None
        self.pointer = Path(pointer).expanduser().absolute()
        self._lock = threading.Lock()
        self._images: OrderedDict[str, bytes] = OrderedDict()
        self._last_key = None
        self._last_preview = None
        self._active_run = None
        self._explicit_entry = None
        if self.run is not None:
            try:
                stamp = self.run.stat().st_mtime
            except FileNotFoundError:
                stamp = datetime.now(UTC).timestamp()
            self._explicit_entry = RunEntry(RunId.for_output(self.run), str(self.run),
                                            datetime.fromtimestamp(stamp, UTC).isoformat())

    def runs(self) -> HistorySnapshot:
        with self._lock:
            entries, latest = self._registry()
            return HistorySnapshot(tuple(RunListing(entry.run_id, entry.output, entry.timestamp,
                                                     _run_summary(Path(entry.output))) for entry in entries), latest)

    def snapshot(self, run_id: str | None = None, capture_id: str | None = None) -> ViewerSnapshot:
        with self._lock:
            run = None
            report = None
            metadata = {}
            try:
                if capture_id is not None and (not ID_PATTERN.fullmatch(capture_id) or run_id is None):
                    raise ViewerError("visual viewer: a capture selection requires a known run and capture ID")
                if run_id is None:
                    entries, latest_id = self._registry()
                    if latest_id is None:
                        return ViewerSnapshot(SnapshotState.WAITING)
                    entry = next(item for item in entries if item.run_id == latest_id)
                    run = Path(entry.output).resolve()
                else:
                    requested = RunId(run_id)
                    entries, _ = self._registry()
                    entry = next((item for item in entries if item.run_id == requested), None)
                    if entry is None:
                        raise ViewerError("visual viewer: unknown run ID")
                    run = Path(entry.output)
                    if run.resolve() != run:
                        raise ViewerError("visual viewer: the run directory contains a symbolic link")
                metadata = {"run_id": RunId.for_output(run), "timestamp": entry.timestamp if entry else None}
                if run != self._active_run:
                    self._last_key = None
                    self._last_preview = None
                    self._active_run = run
                report = _run_summary(run)
                folder = run / "config" / "captures"
                if folder.resolve() != folder:
                    raise ViewerError("visual viewer: the capture directory contains a symbolic link")
                if not folder.exists():
                    if capture_id is not None:
                        raise ViewerError("visual viewer: unknown capture ID")
                    return ViewerSnapshot(SnapshotState.WAITING, str(run), report=report, **metadata)
                candidates = []
                last_error = None
                for _, candidate_id, path in _acknowledgements(folder):
                    try:
                        wire = read_capture_json(path)
                        if wire.get("state") != "completed":
                            continue
                        ack = acknowledgement_from_wire(wire, Command.SCREENSHOT, candidate_id)
                        if isinstance(ack, ScreenshotAck):
                            candidates.append((candidate_id, path, wire, ack.capture))
                    except (OSError, ValueError, ProtocolError, ViewerError, RecursionError) as error:
                        last_error = str(error)
                candidates.sort(key=lambda item: (item[3].frame_ordinal, item[0]), reverse=True)
                groups = {}
                for candidate_id, _, _, capture in reversed(candidates):
                    group = ("multipass_return" if isinstance(capture, Capture) and
                             capture.mode == "multipass" and re.fullmatch(r"suite_multipass_return_[0-9]+", candidate_id)
                             else capture.mode if isinstance(capture, Capture) else "native")
                    groups.setdefault(group, []).append({"id": candidate_id, "frame_ordinal": capture.frame_ordinal,
                                                         "pair": _pair_key(candidate_id, group)})
                outcomes = _group_outcomes(run, groups)
                metadata["groups"] = [{"id": group, "samples": samples,
                                       "outcome": asdict(outcomes[group]) if group in outcomes else None}
                                      for group, samples in groups.items()]
                if capture_id is not None:
                    candidates = [item for item in candidates if item[0] == capture_id]
                    if not candidates:
                        raise ViewerError("visual viewer: unknown capture ID")
                for candidate_id, path, wire, capture in candidates:
                    try:
                        png_path = folder / f"{candidate_id}.capture.png"
                        key = (str(run), _file_key(path), _file_key(png_path))
                        if key == self._last_key:
                            return ViewerSnapshot(SnapshotState.READY, str(run), self._last_preview, report=report, **metadata)
                        image = _read_bounded(png_path, MAX_PNG_BYTES)
                        dimensions = _png_dimensions(image)
                        _validate_capture(capture, candidate_id, dimensions)
                        revision = hashlib.sha256(str(run).encode("utf-8") + b"\0" +
                                                  json.dumps(wire, sort_keys=True, allow_nan=False).encode("utf-8") +
                                                  b"\0" + image).hexdigest()
                        selected = capture.selected_eye if isinstance(capture, Capture) else None
                        labels = (("Physical left eye (L)" if selected == 0 else "Physical right eye (R)",)
                                  if selected is not None else ("L - left eye", "R - right eye"))
                        preview = CapturePreview(revision, f"/capture/{revision}.png", labels, capture)
                        self._images[revision] = image
                        self._images.move_to_end(revision)
                        while len(self._images) > 3 or sum(map(len, self._images.values())) > MAX_CACHE_BYTES:
                            self._images.popitem(last=False)
                        self._last_key, self._last_preview = key, preview
                        return ViewerSnapshot(SnapshotState.READY, str(run), preview, report=report, **metadata)
                    except (OSError, ValueError, ProtocolError, ViewerError, RecursionError) as error:
                        last_error = str(error)
                if last_error:
                    return ViewerSnapshot(SnapshotState.ERROR, str(run), error=last_error, report=report, **metadata)
                return ViewerSnapshot(SnapshotState.WAITING, str(run), report=report, **metadata)
            except FileNotFoundError:
                return ViewerSnapshot(SnapshotState.WAITING, str(run) if run is not None else None, **metadata)
            except (OSError, ValueError, ProtocolError, ViewerError, RecursionError) as error:
                return ViewerSnapshot(SnapshotState.ERROR, str(run) if run is not None else None,
                                      error=str(error), report=report, **metadata)

    def _registry(self) -> tuple[list[RunEntry], RunId | None]:
        if self._explicit_entry is not None:
            return [self._explicit_entry], self._explicit_entry.run_id
        try:
            latest = RunPointer.from_wire(_read_json(self.pointer))
        except FileNotFoundError:
            latest = None
        entries = read_history(self.pointer)
        if latest is None:
            return entries, None
        latest_id = RunId.for_output(latest.output)
        if not any(entry.run_id == latest_id for entry in entries):
            if entries:
                raise ViewerError("visual viewer: the latest run is absent from the history index")
            try:
                stamp = self.pointer.stat().st_mtime
            except FileNotFoundError:
                stamp = datetime.now(UTC).timestamp()
            entries = [RunEntry(latest_id, latest.output, datetime.fromtimestamp(stamp, UTC).isoformat())]
        return entries, latest_id

    def image(self, revision: str) -> bytes | None:
        with self._lock:
            return self._images.get(revision)


def create_server(run: Path | None = None, *, port: int = 8765,
                  pointer: Path = DEFAULT_POINTER) -> ThreadingHTTPServer:
    server = _ViewerServer(("127.0.0.1", port), _Handler)
    server.viewer = ViewerState(run, pointer)
    return server


def main(argv=None):
    parser = argparse.ArgumentParser(description="Local capture preview (not video).")
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--run", type=Path)
    args = parser.parse_args(argv)
    if not 1 <= args.port <= 65535:
        parser.error("--port must be between 1 and 65535")
    logging.basicConfig(level=logging.INFO, format="%(levelname)s: %(message)s")
    try:
        with create_server(args.run, port=args.port) as server:
            LOG.info("Capture preview: http://127.0.0.1:%s", server.server_port)
            try:
                server.serve_forever()
            except KeyboardInterrupt:
                pass
    except OSError as error:
        LOG.error("visual viewer: %s", error)
        return 1
    return 0


LOG = logging.getLogger("tools.rendering.viewer.server")
MAX_CAPTURE_JSON_BYTES = 2 * 1024 * 1024
MAX_PNG_BYTES = 128 * 1024 * 1024
MAX_DECODED_BYTES = 256 * 1024 * 1024
MAX_CACHE_BYTES = 256 * 1024 * 1024
MAX_DIRECTORY_ENTRIES = 4096
ID_PATTERN = re.compile(r"[A-Za-z0-9_-]{1,128}\Z")
IMAGE_PATTERN = re.compile(r"/capture/([0-9a-f]{64})\.png\Z")
PAIR_SEGMENTS = {
    "single_pass": ("single_pass", "single"),
    "multipass": ("multipass", "multi"),
    "multipass_return": ("multipass_return",),
    "reverse_multipass": ("reverse_multipass", "reverse_multi"),
    "left_only": ("left_only", "left"),
    "right_only": ("right_only", "right"),
}


def _run_summary(run):
    try:
        wire = _read_json(run / "result.json")
        status = wire.get("status")
        smoke = wire.get("smoke_succeeded")
        error = wire.get("error")
        if smoke is not None and type(smoke) is not bool:
            raise ViewerError("visual viewer: invalid run smoke result")
        if error is not None and not isinstance(error, str):
            raise ViewerError("visual viewer: invalid run error")
        comparison = wire.get("comparison")
        comparison_status = comparison.get("status") if isinstance(comparison, dict) else None
        if comparison_status is not None and comparison_status not in ("pass", "fail", "inconclusive"):
            raise ViewerError("visual viewer: invalid comparison status")
        failures = []
        if comparison_status in ("fail", "inconclusive"):
            try:
                details = _read_json(run / "comparison" / "report.json")
                failures = _comparison_failures(details)
            except (OSError, ValueError, ViewerError, RecursionError):
                failures = ["Detailed comparison report unavailable"]
        return RunSummary(RunOutcome(status) if status is not None else None, smoke, error,
                          Outcome(comparison_status) if comparison_status is not None else None, tuple(failures))
    except (OSError, ValueError, ProtocolError, ViewerError, RecursionError) as error:
        return RunSummary(error=f"visual viewer: run report unavailable: {error}")


def _group_outcomes(run, groups):
    try:
        wire = _read_json(run / "comparison" / "report.json")
        outcomes = {}
        cases = wire.get("cases", [])
        if not isinstance(cases, list):
            return outcomes
        for case in cases:
            if not isinstance(case, dict) or not isinstance(case.get("modes"), list):
                continue
            for mode in case["modes"]:
                if not isinstance(mode, dict) or mode.get("status") not in Outcome:
                    continue
                mode_id = mode.get("mode")
                if not isinstance(mode_id, str):
                    continue
                repeatability = _group_checks(mode.get("repeatability", []))
                comparisons = _group_checks(case.get("comparisons", []), mode_id)
                for group in groups:
                    reused = group == "multipass_return"
                    if ("multipass" if reused else group) == mode_id:
                        outcomes[group] = GroupOutcome(mode_id, Outcome(mode["status"]), reused,
                                                       repeatability, comparisons)
        return outcomes
    except (OSError, ValueError, ViewerError, RecursionError):
        return {}


def _group_checks(checks, mode=None):
    if not isinstance(checks, list):
        return ()
    result = []
    for check in checks[:64]:
        if not isinstance(check, dict) or check.get("status") not in Outcome:
            continue
        name = check.get("name")
        if not isinstance(name, str):
            continue
        if mode is not None:
            pair = re.fullmatch(r"([a-z_]+) vs ([a-z_]+), eye [01]", name)
            if pair is None or mode not in pair.groups():
                continue
        metrics = check.get("worst_metrics")
        mean = metrics.get("mean") if isinstance(metrics, dict) else None
        changed = metrics.get("changed_fraction") if isinstance(metrics, dict) else None
        mean = float(mean) if type(mean) in (int, float) and math.isfinite(mean) and 0 <= mean <= 255 else None
        changed = (float(changed) if type(changed) in (int, float) and math.isfinite(changed)
                   and 0 <= changed <= 1 else None)
        result.append(GroupComparison(name[:160], Outcome(check["status"]), mean, changed))
    return tuple(result)


def _comparison_failures(wire):
    failures = []
    pending = [(wire, "comparison")]
    while pending and len(failures) < 12:
        item, context = pending.pop()
        if not isinstance(item, dict):
            continue
        name = str(item.get("name", item.get("mode", item.get("label", context))))[:160]
        errors = item.get("errors", [])
        if isinstance(errors, list):
            failures.extend(f"{name}: {message}"[:400] for message in errors[:12 - len(failures)]
                            if isinstance(message, str))
        if item.get("status") in ("fail", "inconclusive") and not errors:
            failures.append(f"{name}: {item['status']}")
        for key in ("cases", "modes", "comparisons", "repeatability"):
            children = item.get(key, [])
            if isinstance(children, list):
                pending.extend((child, name) for child in reversed(children))
    return failures[:12]


def _pair_key(capture_id, group):
    segments = PAIR_SEGMENTS.get(group, ())
    if segments:
        pattern = rf"_(?:{'|'.join(map(re.escape, segments))})_([0-9]+)\Z"
        stripped = re.sub(pattern, r"_\1", capture_id, count=1)
        if stripped != capture_id:
            return stripped
    return capture_id


def _file_key(path):
    info = path.stat(follow_symlinks=False)
    if not stat.S_ISREG(info.st_mode):
        raise ViewerError("visual viewer: the artifact is not a regular file")
    return (path.name, info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def _acknowledgements(folder):
    candidates = []
    with os.scandir(folder) as entries:
        for count, entry in enumerate(entries):
            if count >= MAX_DIRECTORY_ENTRIES:
                raise ViewerError("visual viewer: the capture directory exceeds the entry limit")
            if entry.name.endswith(".json") and ID_PATTERN.fullmatch(entry.name[:-5]):
                info = entry.stat(follow_symlinks=False)
                if stat.S_ISREG(info.st_mode):
                    candidates.append((info.st_mtime_ns, entry.name[:-5], Path(entry.path)))
    return sorted(candidates, reverse=True)


def _png_dimensions(data):
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ViewerError("visual viewer: invalid PNG signature")
    offset, header, ended, idat_closed = 8, None, False, False
    compressed = bytearray()
    while offset < len(data):
        if offset + 12 > len(data):
            raise ViewerError("visual viewer: truncated PNG chunk")
        size, kind = struct.unpack_from(">I4s", data, offset)
        end = offset + size + 12
        if end > len(data):
            raise ViewerError("visual viewer: truncated PNG payload")
        payload = data[offset + 8:end - 4]
        if zlib.crc32(kind + payload) & 0xffffffff != struct.unpack_from(">I", data, end - 4)[0]:
            raise ViewerError("visual viewer: PNG chunk CRC mismatch")
        if header is None and kind != b"IHDR":
            raise ViewerError("visual viewer: PNG does not begin with IHDR")
        if kind == b"IHDR":
            if header is not None or size != 13:
                raise ViewerError("visual viewer: invalid PNG header")
            header = struct.unpack(">IIBBBBB", payload)
        elif kind == b"IDAT":
            if idat_closed:
                raise ViewerError("visual viewer: nonconsecutive PNG image chunks")
            compressed.extend(payload)
        elif kind == b"IEND":
            if size or end != len(data):
                raise ViewerError("visual viewer: invalid PNG end")
            ended = True
            break
        else:
            if compressed:
                idat_closed = True
            if kind[:1].isupper():
                raise ViewerError("visual viewer: unsupported critical PNG chunk")
        offset = end
    if header is None or not ended or not compressed:
        raise ViewerError("visual viewer: incomplete PNG")
    width, height, depth, color, compression, filtering, interlace = header
    if (not (0 < width <= 16384 and 0 < height <= 16384) or depth != 8 or color not in (2, 6) or
            (compression, filtering, interlace) != (0, 0, 0)):
        raise ViewerError("visual viewer: unsupported PNG layout")
    stride = width * (3 if color == 2 else 4) + 1
    expected = stride * height
    if expected > MAX_DECODED_BYTES:
        raise ViewerError("visual viewer: the decoded PNG exceeds the size limit")
    decoder = zlib.decompressobj()
    try:
        pixels = decoder.decompress(compressed, expected + 1)
    except zlib.error as error:
        raise ViewerError(f"visual viewer: invalid PNG compression: {error}") from error
    if len(pixels) != expected or not decoder.eof or decoder.unused_data or decoder.unconsumed_tail:
        raise ViewerError("visual viewer: invalid PNG decoded length or stream")
    if any(pixels[row * stride] > 4 for row in range(height)):
        raise ViewerError("visual viewer: invalid PNG row filter")
    return width, height


def _validate_capture(capture, capture_id, dimensions):
    if isinstance(capture, Capture):
        capture.validate(capture_id, capture.mode, capture.visual.generation, dimensions)
    elif (capture.id != capture_id or not capture.gameplay_ready or not capture.xr_session_running or
          capture.fresh_eyes != (True, True) or capture.source not in ("eye_textures", "collapsed_back_buffer") or
          dimensions != (capture.width, capture.height) or
          dimensions != (capture.eye_width * 2, capture.eye_height)):
        raise ViewerError("visual viewer: the native capture is not a fresh XR pair with matching dimensions")


class _ViewerServer(ThreadingHTTPServer):
    daemon_threads = True
    block_on_close = False

    def get_request(self):
        connection, address = super().get_request()
        connection.settimeout(5)
        return connection, address


class _Handler(BaseHTTPRequestHandler):
    server_version = "VisualViewer"
    sys_version = ""

    def do_GET(self):
        if not self._allowed():
            self._send(403, b"Forbidden", "text/plain; charset=utf-8")
            return
        if self.path == "/":
            self._send(200, PAGE.encode(), "text/html; charset=utf-8")
        elif self.path == "/viewer.js":
            self._send(200, SCRIPT.encode(), "text/javascript; charset=utf-8")
        elif self.path == "/viewer.css":
            self._send(200, STYLE.encode(), "text/css; charset=utf-8")
        elif self.path == "/api/runs":
            try:
                body = json.dumps(self.server.viewer.runs().to_wire(), allow_nan=False).encode("utf-8")
                self._send(200, body, "application/json; charset=utf-8")
            except (OSError, ValueError, ViewerError, RecursionError) as error:
                self._send(503, str(error).encode(), "text/plain; charset=utf-8")
        elif urlsplit(self.path).path == "/api/status":
            try:
                query = parse_qs(urlsplit(self.path).query, keep_blank_values=True, strict_parsing=True,
                                 max_num_fields=2)
                if set(query) - {"run_id", "capture_id"} or any(len(values) != 1 for values in query.values()):
                    raise ValueError("invalid selection fields")
                selection = {key: values[0] for key, values in query.items()}
                if any(not value for value in selection.values()):
                    raise ValueError("empty selection")
                snapshot = self.server.viewer.snapshot(**selection)
            except ValueError:
                self._send(404, b"Unknown selection", "text/plain; charset=utf-8")
                return
            body = json.dumps(snapshot.to_wire(), allow_nan=False).encode("utf-8")
            self._send(404 if selection and snapshot.state == SnapshotState.ERROR else 200,
                       body, "application/json; charset=utf-8")
        elif match := IMAGE_PATTERN.fullmatch(self.path):
            image = self.server.viewer.image(match[1])
            if image is None:
                self._send(404, b"Unknown capture revision", "text/plain; charset=utf-8")
            else:
                self._send(200, image, "image/png")
        else:
            self._send(404, b"Not found", "text/plain; charset=utf-8")

    def do_POST(self):
        self._send(405 if self._allowed() else 403, b"Read-only viewer", "text/plain; charset=utf-8")

    do_PUT = do_POST
    do_DELETE = do_POST
    do_PATCH = do_POST
    do_OPTIONS = do_POST
    do_HEAD = do_POST

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Security-Policy", "default-src 'none'; script-src 'self'; style-src 'self'; "
                         "img-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("X-Frame-Options", "DENY")
        self.send_header("Referrer-Policy", "no-referrer")
        super().end_headers()

    def log_message(self, format, *args):
        LOG.debug("HTTP: " + format, *args)

    def _allowed(self):
        hosts = self.headers.get_all("Host", [])
        origins = self.headers.get_all("Origin", [])
        allowed = {f"127.0.0.1:{self.server.server_port}", f"localhost:{self.server.server_port}"}
        return (self.client_address[0] == "127.0.0.1" and len(hosts) == 1 and hosts[0].lower() in allowed and
                (not origins or (len(origins) == 1 and origins[0] in {f"http://{host}" for host in allowed})) and
                self.headers.get("Sec-Fetch-Site") != "cross-site")

    def _send(self, status, body, content_type):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            try:
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError, TimeoutError):
                pass


PAGE = """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">
<title>Capture preview</title><link rel="stylesheet" href="/viewer.css">
<script src="/viewer.js" defer></script></head><body>
<h1>Capture preview</h1><p>Completed captures, refreshed every second. This is not video.</p>
<nav><label><input id="follow" type="checkbox" checked> Follow latest</label>
<label>Benchmark run <select id="runs"></select></label>
<button id="previous-group" type="button">Previous group</button>
<label>Mode / group <select id="groups"></select></label>
<button id="next-group" type="button">Next group</button>
<button id="previous" type="button">Previous sample</button>
<label>Sample <select id="samples"></select></label>
<button id="next" type="button">Next sample</button></nav>
<p id="status">Waiting for a capture.</p><p id="summary"></p><section id="preview" hidden>
<div id="labels"></div><img id="capture" alt="Completed XR capture"><p id="details"></p>
</section></body></html>"""

SCRIPT = """'use strict';
const status = document.getElementById('status');
const preview = document.getElementById('preview');
let image = document.getElementById('capture');
const labels = document.getElementById('labels');
const details = document.getElementById('details');
const summary = document.getElementById('summary');
const runs = document.getElementById('runs');
const groups = document.getElementById('groups');
const samples = document.getElementById('samples');
const follow = document.getElementById('follow');
const previousGroup = document.getElementById('previous-group');
const nextGroup = document.getElementById('next-group');
const previous = document.getElementById('previous');
const nextButton = document.getElementById('next');
const storageKey = 'visual-viewer-selection-v1';
let selection = {follow: true, run_id: null, capture_id: null};
try {
  const saved = JSON.parse(localStorage.getItem(storageKey));
  if (saved && saved.follow === false && /^[0-9a-f]{64}$/.test(saved.run_id) &&
      (saved.capture_id === null || /^[A-Za-z0-9_-]{1,128}$/.test(saved.capture_id))) {
    selection = {follow: false, run_id: saved.run_id, capture_id: saved.capture_id};
  }
} catch (_) {}
follow.checked = selection.follow;
let revision = null;
let notice = null;
let displayedRun = null;
let displayedCapture = null;
let current = null;
let generation = 0;
let timer = null;
let busy = false;
let refreshRequested = false;
function options(element, values, selected) {
  element.replaceChildren(...values.map(value => {
    const option = document.createElement('option');
    option.value = value.id;
    option.textContent = value.label;
    return option;
  }));
  element.value = selected || '';
}
function save() {
  try { localStorage.setItem(storageKey, JSON.stringify(selection)); } catch (_) {}
}
function changed() {
  generation++;
  notice = null;
  follow.checked = selection.follow;
  save();
  preview.hidden = true;
  revision = null;
  refreshRequested = true;
  if (!busy) {
    clearTimeout(timer);
    poll();
  }
}
function pin(captureId, runId = current?.run_id) {
  if (!runId) return;
  selection = {follow: false, run_id: runId, capture_id: captureId};
  changed();
}
function selectedSample() {
  const captureId = selection.capture_id || current?.preview?.capture.id;
  const allGroups = current?.groups || [];
  return allGroups.flatMap(group => group.samples).find(sample => sample.id === captureId);
}
function pinPair(group) {
  if (!group) return;
  const sample = selectedSample();
  const pair = sample?.pair;
  const target = pair ? group.samples.find(item => item.pair === pair) : null;
  if (target) {
    pin(target.id);
    return;
  }
  notice = 'No corresponding sample: ' + (sample ? sample.id : 'none') + (pair ? ' (' + pair + ')' : '') +
    ' is not in ' + group.id + '.';
  const allGroups = current?.groups || [];
  groups.value = allGroups.find(item => item.samples.some(entry => entry.id === sample?.id))?.id || '';
  status.textContent = notice;
}
runs.onchange = () => pin(null, runs.value);
groups.onchange = () => pinPair(current?.groups.find(group => group.id === groups.value));
samples.onchange = () => pin(samples.value);
function move(delta) {
  const group = current?.groups.find(group => group.id === groups.value);
  if (!group) return;
  const index = group.samples.findIndex(sample => sample.id === samples.value) + delta;
  if (index >= 0 && index < group.samples.length) pin(group.samples[index].id);
}
function moveGroup(delta) {
  const allGroups = current?.groups || [];
  const index = allGroups.findIndex(group => group.id === groups.value) + delta;
  if (index >= 0 && index < allGroups.length) pinPair(allGroups[index]);
}
previousGroup.onclick = () => moveGroup(-1);
nextGroup.onclick = () => moveGroup(1);
previous.onclick = () => move(-1);
nextButton.onclick = () => move(1);
follow.onchange = () => {
  if (follow.checked) {
    selection = {follow: true, run_id: null, capture_id: null};
    changed();
  } else if (current?.run_id) {
    pin(current.preview?.capture.id || null);
  } else {
    follow.checked = true;
  }
};
function navigation(snapshot) {
  const allGroups = snapshot.groups || [];
  const captureId = snapshot.preview?.capture.id || selection.capture_id;
  const group = allGroups.find(group => group.samples.some(sample => sample.id === captureId));
  options(groups, allGroups.map(group => ({id: group.id,
    label: group.id + (group.outcome ? ' | ' +
      (group.outcome.reused_aggregate ? 'reused multipass aggregate: ' : 'mode aggregate: ') +
      group.outcome.status : ' | not measured'),
  })), group?.id);
  options(samples, (group?.samples || []).map((sample, index) => ({
    id: sample.id, label: (index + 1) + ': ' + sample.id + ' | frame ' + sample.frame_ordinal,
  })), captureId);
  const groupIndex = allGroups.findIndex(item => item.id === group?.id);
  previousGroup.disabled = groupIndex <= 0;
  nextGroup.disabled = groupIndex < 0 || groupIndex >= allGroups.length - 1;
  const index = group?.samples.findIndex(sample => sample.id === captureId) ?? -1;
  previous.disabled = index <= 0;
  nextButton.disabled = index < 0 || index >= (group?.samples.length || 0) - 1;
}
async function poll() {
  if (busy) return;
  busy = true;
  refreshRequested = false;
  const requestGeneration = generation;
  try {
    const historyResponse = await fetch('/api/runs', {cache: 'no-store'});
    if (!historyResponse.ok) throw new Error('Run history request failed.');
    const history = await historyResponse.json();
    if (requestGeneration !== generation) return;
    const runId = selection.follow ? history.latest_run_id : selection.run_id;
    options(runs, history.runs.map(run => ({id: run.run_id,
      label: run.timestamp + ' | ' + run.output.split(/[\\/]/).pop() +
        ' | ' + (run.report?.status || 'unknown') +
        (run.report?.comparison ? ' | comparison ' + run.report.comparison : ''),
    })), runId);
    if (!selection.follow && !history.runs.some(run => run.run_id === runId)) {
      throw new Error('The pinned run is unavailable. Select another run or follow latest.');
    }
    let url = '/api/status';
    if (!selection.follow) {
      url += '?run_id=' + encodeURIComponent(selection.run_id);
      if (selection.capture_id) url += '&capture_id=' + encodeURIComponent(selection.capture_id);
    }
    const response = await fetch(url, {cache: 'no-store'});
    const snapshot = await response.json();
    if (requestGeneration !== generation) return;
    if (!response.ok) throw new Error(snapshot.error || 'Status request failed.');
    current = snapshot;
    if (!selection.follow && !selection.capture_id && snapshot.preview) {
      selection.capture_id = snapshot.preview.capture.id;
      save();
    }
    const captureId = snapshot.preview?.capture.id || null;
    if (snapshot.run !== displayedRun || captureId !== displayedCapture) {
      preview.hidden = true;
      revision = null;
      displayedRun = snapshot.run;
      displayedCapture = captureId;
    }
    navigation(snapshot);
    const report = snapshot.report;
    summary.textContent = (snapshot.timestamp || 'Timestamp unknown') + ' | ' +
      (report?.status || 'Run status unknown') +
      (report?.comparison ? ' | comparison: ' + report.comparison : '') +
      (report?.error ? ' | ' + report.error : '') +
      (report?.comparison_failures?.length ? ' | ' + report.comparison_failures.join(' | ') : '');
    const group = snapshot.groups?.find(group => group.id === groups.value);
    const outcome = group?.outcome;
    if (outcome) {
      summary.textContent += ' | ' + (outcome.reused_aggregate ? 'Reused multipass aggregate' : 'Mode aggregate') +
        ': ' + outcome.status;
      for (const check of [...outcome.repeatability, ...outcome.comparisons]) {
        summary.textContent += ' | ' + check.name + ': ' + check.status +
          (check.mean_absolute_error !== null ? ' MAE ' + check.mean_absolute_error.toFixed(3) : '') +
          (check.changed_fraction !== null ? ' changed ' + (100 * check.changed_fraction).toFixed(3) + '%' : '');
      }
    }
    if (snapshot.state !== 'ready') {
      preview.hidden = true;
      revision = null;
      status.textContent = snapshot.error || 'Waiting for a completed capture.';
      return;
    }
    const next = snapshot.preview;
    if (next.revision !== revision) {
      preview.hidden = true;
      const loaded = new Image();
      loaded.src = next.image_url;
      await loaded.decode();
      if (requestGeneration !== generation) return;
      const names = next.labels.map(text => {
        const label = document.createElement('span');
        label.textContent = text;
        return label;
      });
      image.replaceWith(loaded);
      image = loaded;
      loaded.id = 'capture';
      loaded.alt = 'Completed XR capture';
      labels.replaceChildren(...names);
      labels.className = names.length === 1 ? 'mono' : 'stereo';
      details.textContent = next.capture.id + ' | ' + (next.capture.mode || 'native') +
        ' | frame ' + next.capture.frame_ordinal;
      revision = next.revision;
    }
    preview.hidden = false;
    status.textContent = (selection.follow ? 'Following latest: ' : 'Pinned: ') + snapshot.run +
      (notice ? ' | ' + notice : '');
  } catch (error) {
    if (requestGeneration !== generation) return;
    preview.hidden = true;
    revision = null;
    status.textContent = 'Preview unavailable: ' + error.message;
  } finally {
    busy = false;
    if (refreshRequested) timer = setTimeout(poll, 0);
    else timer = setTimeout(poll, 1000);
  }
}
poll();
"""

STYLE = """body { background: #14171c; color: #eee; font: 16px system-ui, sans-serif; margin: 2rem; }
h1 { font-size: 1.5rem; } img { display: block; width: 100%; height: auto; }
nav { display: flex; align-items: center; flex-wrap: wrap; gap: .8rem; }
select { max-width: min(90vw, 42rem); } button, select { padding: .4rem; }
#labels { display: grid; text-align: center; padding: .5rem; gap: 1rem; }
.stereo { grid-template-columns: 1fr 1fr; } .mono { grid-template-columns: 1fr; }
#status, #summary, #details { overflow-wrap: anywhere; } [hidden] { display: none !important; }
"""
