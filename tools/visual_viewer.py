import argparse
import hashlib
import json
import logging
import os
import re
import stat
import struct
import tempfile
import threading
import zlib
from collections import OrderedDict
from dataclasses import asdict, dataclass
from enum import StrEnum
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

if __package__:
    from .visual_protocol import (
        Capture,
        Command,
        NativeCapture,
        ProtocolError,
        RunOutcome,
        ScreenshotAck,
        acknowledgement_from_wire,
        object_value,
    )
else:
    from visual_protocol import (
        Capture,
        Command,
        NativeCapture,
        ProtocolError,
        RunOutcome,
        ScreenshotAck,
        acknowledgement_from_wire,
        object_value,
    )


DEFAULT_POINTER = Path(__file__).resolve().parents[1] / "target/visual-harness/latest.json"


class ViewerError(RuntimeError):
    pass


@dataclass(frozen=True)
class RunPointer:
    output: str
    version: int = 1

    def __post_init__(self):
        if type(self.version) is not int or self.version != 1:
            raise ViewerError("visual viewer: unsupported run pointer version")
        if not isinstance(self.output, str) or not self.output or "\0" in self.output or not Path(self.output).is_absolute():
            raise ViewerError("visual viewer: the run pointer requires an absolute output path")

    @classmethod
    def from_wire(cls, value):
        object_value(value)
        if set(value) != {"version", "output"}:
            raise ViewerError("visual viewer: invalid run pointer fields")
        return cls(output=value["output"], version=value["version"])

    def to_wire(self):
        return asdict(self)


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

    def to_wire(self):
        return asdict(self)


def publish_run(output: Path, pointer: Path = DEFAULT_POINTER) -> RunPointer:
    value = RunPointer(str(Path(output).expanduser().resolve()))
    destination = Path(pointer).expanduser().absolute()
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="wb", prefix=f".{destination.name}.", dir=destination.parent,
                                         delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(json.dumps(value.to_wire(), allow_nan=False).encode("utf-8"))
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, destination)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    return value


class ViewerState:
    def __init__(self, run: Path | None = None, pointer: Path = DEFAULT_POINTER):
        self.run = Path(run).expanduser().resolve() if run is not None else None
        self.pointer = Path(pointer).expanduser().absolute()
        self._lock = threading.Lock()
        self._images: OrderedDict[str, bytes] = OrderedDict()
        self._last_key = None
        self._last_preview = None
        self._active_run = None

    def snapshot(self) -> ViewerSnapshot:
        with self._lock:
            run = None
            report = None
            try:
                run = self.run or Path(RunPointer.from_wire(_read_json(self.pointer)).output).resolve()
                if run != self._active_run:
                    self._images.clear()
                    self._last_key = None
                    self._last_preview = None
                    self._active_run = run
                report = _run_summary(run)
                folder = run / "config" / "captures"
                if folder.resolve() != folder:
                    raise ViewerError("visual viewer: the capture directory contains a symbolic link")
                if not folder.exists():
                    return ViewerSnapshot(SnapshotState.WAITING, str(run), report=report)
                candidates = []
                last_error = None
                for _, capture_id, path in _acknowledgements(folder):
                    try:
                        wire = _read_json(path)
                        if wire.get("state") != "completed":
                            continue
                        ack = acknowledgement_from_wire(wire, Command.SCREENSHOT, capture_id)
                        if isinstance(ack, ScreenshotAck):
                            candidates.append((capture_id, path, wire, ack.capture))
                    except (OSError, ValueError, ProtocolError, ViewerError, RecursionError) as error:
                        last_error = str(error)
                candidates.sort(key=lambda item: (item[3].frame_ordinal, item[0]), reverse=True)
                for capture_id, path, wire, capture in candidates:
                    try:
                        png_path = folder / f"{capture_id}.capture.png"
                        key = (str(run), _file_key(path), _file_key(png_path))
                        if key == self._last_key:
                            return ViewerSnapshot(SnapshotState.READY, str(run), self._last_preview, report=report)
                        image = _read_bounded(png_path, MAX_PNG_BYTES)
                        dimensions = _png_dimensions(image)
                        _validate_capture(capture, capture_id, dimensions)
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
                        return ViewerSnapshot(SnapshotState.READY, str(run), preview, report=report)
                    except (OSError, ValueError, ProtocolError, ViewerError, RecursionError) as error:
                        last_error = str(error)
                if last_error:
                    return ViewerSnapshot(SnapshotState.ERROR, str(run), error=last_error, report=report)
                return ViewerSnapshot(SnapshotState.WAITING, str(run), report=report)
            except FileNotFoundError:
                return ViewerSnapshot(SnapshotState.WAITING, str(run) if run is not None else None)
            except (OSError, ValueError, ProtocolError, ViewerError, RecursionError) as error:
                return ViewerSnapshot(SnapshotState.ERROR, str(run) if run is not None else None, error=str(error), report=report)

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


LOG = logging.getLogger("visual_viewer")
MAX_JSON_BYTES = 256 * 1024
MAX_PNG_BYTES = 128 * 1024 * 1024
MAX_DECODED_BYTES = 256 * 1024 * 1024
MAX_CACHE_BYTES = 256 * 1024 * 1024
MAX_DIRECTORY_ENTRIES = 4096
ID_PATTERN = re.compile(r"[A-Za-z0-9_-]{1,128}\Z")
IMAGE_PATTERN = re.compile(r"/capture/([0-9a-f]{64})\.png\Z")


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
        return RunSummary(RunOutcome(status) if status is not None else None, smoke, error)
    except (OSError, ValueError, ProtocolError, ViewerError, RecursionError) as error:
        return RunSummary(error=f"visual viewer: run report unavailable: {error}")


def _read_bounded(path, limit):
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
            raise ViewerError("visual viewer: the artifact is not a bounded regular file")
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ViewerError("visual viewer: the artifact exceeds the size limit")
    return data


def _read_json(path):
    return object_value(json.loads(_read_bounded(path, MAX_JSON_BYTES)))


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
        elif self.path == "/api/status":
            body = json.dumps(self.server.viewer.snapshot().to_wire(), allow_nan=False).encode("utf-8")
            self._send(200, body, "application/json; charset=utf-8")
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
<p id="status">Waiting for a capture.</p><section id="preview" hidden>
<div id="labels"></div><img id="capture" alt="Completed XR capture"><p id="details"></p>
</section></body></html>"""

SCRIPT = """'use strict';
const status = document.getElementById('status');
const preview = document.getElementById('preview');
let image = document.getElementById('capture');
const labels = document.getElementById('labels');
const details = document.getElementById('details');
let revision = null;
let displayedRun = null;
async function poll() {
  try {
    const response = await fetch('/api/status', {cache: 'no-store'});
    if (!response.ok) throw new Error('Status request failed.');
    const snapshot = await response.json();
    if (snapshot.run !== displayedRun) {
      preview.hidden = true;
      revision = null;
      displayedRun = snapshot.run;
    }
    if (snapshot.state !== 'ready') {
      preview.hidden = true;
      revision = null;
      status.textContent = (snapshot.error || 'Waiting for a completed capture.') +
        ' | ' + (snapshot.report?.status || 'Run status unknown');
      return;
    }
    const next = snapshot.preview;
    if (next.revision !== revision) {
      const loaded = new Image();
      loaded.src = next.image_url;
      await loaded.decode();
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
    status.textContent = snapshot.run + ' | ' + (snapshot.report?.status || 'Run status unknown');
  } catch (error) {
    preview.hidden = true;
    revision = null;
    status.textContent = 'Preview unavailable: ' + error.message;
  } finally {
    setTimeout(poll, 1000);
  }
}
poll();
"""

STYLE = """body { background: #14171c; color: #eee; font: 16px system-ui, sans-serif; margin: 2rem; }
h1 { font-size: 1.5rem; } img { display: block; width: 100%; height: auto; }
#labels { display: grid; text-align: center; padding: .5rem; gap: 1rem; }
.stereo { grid-template-columns: 1fr 1fr; } .mono { grid-template-columns: 1fr; }
#status, #details { overflow-wrap: anywhere; } [hidden] { display: none !important; }
"""


if __name__ == "__main__":
    raise SystemExit(main())
