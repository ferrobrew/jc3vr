#!/usr/bin/env python3
"""The capture host's relay: record the game's mirror window with gpu-screen-recorder for each take
the payload starts, into the take's own folder, and go back to listening when the take ends.

    relay.py [--host 127.0.0.1] [--port 47172] [--fallback-dir captures]
             [--fps 60] [--quality high] [--audio default_output]
             [--window-id ID | --region WxH+X+Y] [-- EXTRA GSR ARGS...]

The payload connects over TCP and speaks the control protocol below. For each take the relay finds
the game's window in niri (`niri msg --json windows`), computes the window's region on screen from
its workspace and output, and runs gpu-screen-recorder on it with audio from the default
output, writing `video.mkv` into the take's folder beside the payload's `frames.jsonl`. It writes
`stream.json` there with the region, the output, the window, and the wall-clock time of the video's
first frame, so `video_time = unix_ms - video_start_unix_ms` lines the payload's sidecar up with the
video. When the take ends it sends gpu-screen-recorder SIGINT, waits for it to finalize the file,
and listens again, so one relay serves every take of a session.

## Control protocol, version 1

Each message is one line of UTF-8 JSON terminated by `\\n`, at most 64 KiB. The payload opens with

    {"protocol": "jc3vr-capture", "version": 1, "type": "start", "take": "<stamp>",
     "folder": "/abs/host/path" | null, "window": {"title": "...", "client_size": [w, h]}}

where `folder` is the take's folder as the host names it (null when the payload cannot name it, on
native Windows, in which case the relay uses a stamped folder under `--fallback-dir`) and `window`
describes the game window as the payload sees it (both fields optional). The relay answers with one
of

    {"type": "started", "file": "...", "video_start_unix_ms": <float | null>,
     "region": {"x": .., "y": .., "width": .., "height": ..}, "output": "<name> | null"}
    {"type": "error", "message": "..."}

and closes the connection after an error. While recording it may send an error (gpu-screen-recorder
exited on its own) and close. The payload ends the take with `{"type": "stop"}`, or by closing the
connection; after a stop line the relay answers `{"type": "stopped", "exit_code": <int | null>}`
once the file is finalized. A message of an unknown type is ignored by either side; a start with an
unknown protocol name or version is refused with an error.

## Finding the window

The window is the one whose title equals the title the payload sent (or `Just Cause 3` when it sent
none), falling back to the Proton app id (`steam_app_225540`). Several matches are narrowed by app
id, then by a window size equal to the payload's client size, then by focus. The region is the
window's tile position in its workspace view plus the window's offset in the tile, offset by the
output's logical position; this holds for floating and tiled windows alike, since niri reports the
tile position for both. The window's workspace must be the active one on its output and the window
must lie wholly inside the output, since gpu-screen-recorder records a fixed region and anything
covering it is recorded too. `--window-id` picks a niri window by id instead, and `--region` skips
niri entirely, for other compositors and for testing.

gpu-screen-recorder runs in its own session, so Ctrl+C reaches only the relay, which then stops the
take in progress and waits for the file to be finalized. Standard library only; run it with
`python3 -I`.
"""

import argparse
import json
import logging
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
from dataclasses import dataclass

DEFAULT_PORT = 47172
PROTOCOL = "jc3vr-capture"
PROTOCOL_VERSION = 1
STREAM_FORMAT = "jc3vr-capture-stream"
STREAM_VERSION = 1
LINE_LIMIT = 64 * 1024
DEFAULT_TITLE = "Just Cause 3"
DEFAULT_APP_IDS = ("steam_app_225540",)
GSR = "gpu-screen-recorder"

# How long the payload has to send its start line after connecting.
START_TIMEOUT_S = 10.0
# How long gpu-screen-recorder has to capture its first frame before the take is reported started
# without a start time.
FIRST_FRAME_TIMEOUT_S = 10.0
# How long gpu-screen-recorder has to finalize the file after SIGINT before it is killed.
FINALIZE_TIMEOUT_S = 30.0
POLL_S = 0.1
# GPU-selection overrides a launching terminal may carry (a PRIME offload wrapper pins its clients'
# rendering to another GPU this way). gpu-screen-recorder must use the GPU that drives the display,
# and with these set it finds no card to capture from.
GPU_OVERRIDES = ("__EGL_VENDOR_LIBRARY_FILENAMES", "DRI_PRIME", "VK_DRIVER_FILES", "VK_ICD_FILENAMES")

log = logging.getLogger("capture")


class ProtocolError(Exception):
    """A control message that is not valid for protocol version 1."""


class LocateError(Exception):
    """The game window could not be found or its region computed."""


@dataclass(frozen=True)
class WindowHint:
    """The game window as the payload describes it."""

    title: str | None
    client_size: tuple[int, int] | None


@dataclass(frozen=True)
class Start:
    take: str | None
    folder: str | None
    window: WindowHint


@dataclass(frozen=True)
class Stop:
    pass


@dataclass(frozen=True)
class Unknown:
    """A message of a type this version does not know, which is ignored."""

    type: str


@dataclass(frozen=True)
class Region:
    x: int
    y: int
    width: int
    height: int

    def gsr_arg(self) -> str:
        return f"{self.width}x{self.height}+{self.x}+{self.y}"

    def to_json(self) -> dict:
        return {"x": self.x, "y": self.y, "width": self.width, "height": self.height}


@dataclass(frozen=True)
class Located:
    """Where the take records from: the region, and the niri window and output when known."""

    region: Region
    output: dict | None
    window: dict | None


def parse_message(line: bytes) -> Start | Stop | Unknown:
    """Parse one control line (newline optional) from the payload."""
    try:
        message = json.loads(line.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ProtocolError(f"not a JSON line: {error}") from None
    if not isinstance(message, dict):
        raise ProtocolError("a message must be a JSON object")
    kind = message.get("type")
    if not isinstance(kind, str):
        raise ProtocolError("a message must have a string `type`")
    if kind == "stop":
        return Stop()
    if kind != "start":
        return Unknown(kind)
    if message.get("protocol") != PROTOCOL:
        raise ProtocolError(f"the start names protocol {message.get('protocol')!r}, not {PROTOCOL!r}")
    if message.get("version") != PROTOCOL_VERSION:
        raise ProtocolError(
            f"the start is for protocol version {message.get('version')!r}; this relay speaks "
            f"version {PROTOCOL_VERSION}"
        )
    folder = message.get("folder")
    if folder is not None:
        if not isinstance(folder, str) or not folder.startswith("/") or "\0" in folder:
            raise ProtocolError(f"the folder is not an absolute path: {folder!r}")
    take = message.get("take")
    if take is not None and (not isinstance(take, str) or not is_plain_name(take)):
        raise ProtocolError(f"the take is not a plain name: {take!r}")
    return Start(take=take, folder=folder, window=parse_window_hint(message.get("window")))


def parse_window_hint(value) -> WindowHint:
    if value is None:
        return WindowHint(title=None, client_size=None)
    if not isinstance(value, dict):
        raise ProtocolError("`window` must be an object")
    title = value.get("title")
    if title is not None and not isinstance(title, str):
        raise ProtocolError("`window.title` must be a string")
    size = value.get("client_size")
    if size is not None:
        if (
            not isinstance(size, list)
            or len(size) != 2
            or not all(isinstance(v, int) and not isinstance(v, bool) and v > 0 for v in size)
        ):
            raise ProtocolError("`window.client_size` must be two positive integers")
        size = (size[0], size[1])
    return WindowHint(title=title or None, client_size=size)


def is_plain_name(name: str) -> bool:
    return bool(name) and name not in (".", "..") and "/" not in name and "\0" not in name


def encode(message: dict) -> bytes:
    return json.dumps(message, separators=(",", ":")).encode("utf-8") + b"\n"


class LineReader:
    """Split a byte stream into lines, refusing a line longer than the limit."""

    def __init__(self, limit: int = LINE_LIMIT):
        self.limit = limit
        self.buffer = b""

    def feed(self, data: bytes) -> list[bytes]:
        self.buffer += data
        lines = []
        while (end := self.buffer.find(b"\n")) >= 0:
            lines.append(self.buffer[:end])
            self.buffer = self.buffer[end + 1 :]
        if len(self.buffer) > self.limit:
            raise ProtocolError(f"a control line is longer than {self.limit} bytes")
        return lines


def parse_region(text: str) -> Region:
    """Parse `WxH+X+Y`, the form gpu-screen-recorder and slurp use."""
    try:
        size, x, y = text.split("+")
        width, height = size.split("x")
        region = Region(x=int(x), y=int(y), width=int(width), height=int(height))
    except ValueError:
        raise ValueError(f"not a WxH+X+Y region: {text!r}") from None
    if region.width <= 0 or region.height <= 0:
        raise ValueError(f"the region is empty: {text!r}")
    return region


def find_window(
    windows: list[dict],
    hint: WindowHint,
    app_ids: tuple[str, ...] = DEFAULT_APP_IDS,
) -> dict:
    """Pick the game's window from niri's window list."""
    title = hint.title or DEFAULT_TITLE
    candidates = [w for w in windows if w.get("title") == title]
    if not candidates:
        candidates = [w for w in windows if w.get("app_id") in app_ids]
    if not candidates:
        seen = ", ".join(f"{w.get('app_id')!r}: {w.get('title')!r}" for w in windows) or "none"
        raise LocateError(
            f"no window titled {title!r} or with app id {' or '.join(app_ids)}; windows: {seen}"
        )
    narrowings = [
        lambda w: w.get("app_id") in app_ids,
        lambda w: hint.client_size is not None
        and tuple(w.get("layout", {}).get("window_size", ())) == hint.client_size,
        lambda w: bool(w.get("is_focused")),
    ]
    for keep in narrowings:
        if len(candidates) == 1:
            break
        narrowed = [w for w in candidates if keep(w)]
        if narrowed:
            candidates = narrowed
    if len(candidates) > 1:
        ids = ", ".join(str(w.get("id")) for w in candidates)
        raise LocateError(f"several windows match {title!r} (ids {ids}); pass --window-id")
    return candidates[0]


def window_region(window: dict, workspaces: list[dict], outputs: dict) -> tuple[Region, dict]:
    """The window's region in the compositor's logical layout, and its output."""
    describe = f"window {window.get('id')} ({window.get('title')!r})"
    workspace_id = window.get("workspace_id")
    workspace = next((w for w in workspaces if w.get("id") == workspace_id), None)
    if workspace is None:
        raise LocateError(f"{describe} is not on a workspace")
    if not workspace.get("is_active"):
        raise LocateError(
            f"{describe} is on workspace {workspace.get('idx')} of {workspace.get('output')}, "
            "which is not the active one there; switch to it"
        )
    output = outputs.get(workspace.get("output"))
    if output is None:
        output = next(
            (o for o in outputs.values() if o.get("name") == workspace.get("output")), None
        )
    if output is None or output.get("logical") is None:
        raise LocateError(f"{describe} is on output {workspace.get('output')!r}, which is off")
    logical = output["logical"]
    layout = window.get("layout") or {}
    tile = layout.get("tile_pos_in_workspace_view")
    if tile is None:
        raise LocateError(f"niri reports no on-screen position for {describe}")
    offset = layout.get("window_offset_in_tile") or [0.0, 0.0]
    width, height = layout.get("window_size") or (0, 0)
    if width <= 0 or height <= 0:
        raise LocateError(f"{describe} has no size")
    region = Region(
        x=int(logical["x"] + tile[0] + offset[0]),
        y=int(logical["y"] + tile[1] + offset[1]),
        width=int(width),
        height=int(height),
    )
    inside = (
        region.x >= logical["x"]
        and region.y >= logical["y"]
        and region.x + region.width <= logical["x"] + logical["width"]
        and region.y + region.height <= logical["y"] + logical["height"]
    )
    if not inside:
        raise LocateError(
            f"{describe} at {region.gsr_arg()} is not wholly on {output.get('name')} "
            f"({logical['width']}x{logical['height']}+{logical['x']}+{logical['y']}); "
            "scroll it fully into view"
        )
    return region, output


def take_paths(folder: str) -> tuple[str, str]:
    """The video and metadata paths for a take in `folder`: `video.mkv` and `stream.json`, or the
    first free `video_N.mkv` and `stream_N.json` when an earlier connection used those."""
    n = 0
    while True:
        suffix = "" if n == 0 else f"_{n}"
        video = os.path.join(folder, f"video{suffix}.mkv")
        stream = os.path.join(folder, f"stream{suffix}.json")
        if not os.path.exists(video) and not os.path.exists(stream):
            return video, stream
        n += 1


def build_command(
    gsr: str, region: Region, video: str, fps: int, quality: str, audio: str, extra: list[str]
) -> list[str]:
    command = [gsr, "-w", region.gsr_arg(), "-f", str(fps), "-q", quality]
    if audio != "none":
        command += ["-a", audio]
    return command + ["-write-first-frame-ts", "yes", *extra, "-o", video]


def read_first_frame_ts(path: str) -> float | None:
    """The first frame's wall-clock time in Unix milliseconds, from gpu-screen-recorder's `.ts`
    file (a header line, then `<monotonic_us> <realtime_us>`), or None while it is incomplete."""
    try:
        with open(path) as f:
            lines = f.read().split("\n")
    except FileNotFoundError:
        return None
    for line in lines[1:]:
        fields = line.split()
        if len(fields) == 2:
            try:
                return int(fields[1]) / 1000.0
            except ValueError:
                return None
    return None


def unix_ms() -> int:
    return time.time_ns() // 1_000_000


@dataclass
class Config:
    fallback_dir: str
    fps: int
    quality: str
    audio: str
    extra: list[str]
    window_id: int | None
    region: Region | None
    gsr: str = GSR


def niri_json(*args: str):
    try:
        result = subprocess.run(
            ["niri", "msg", "--json", *args], capture_output=True, check=True, timeout=5
        )
    except FileNotFoundError:
        raise LocateError("niri is not on PATH; pass --region to record a fixed region") from None
    except subprocess.CalledProcessError as error:
        stderr = error.stderr.decode(errors="replace").strip()
        raise LocateError(f"`niri msg {' '.join(args)}` failed: {stderr}") from None
    except subprocess.TimeoutExpired:
        raise LocateError(f"`niri msg {' '.join(args)}` timed out") from None
    return json.loads(result.stdout)


def locate(config: Config, hint: WindowHint) -> Located:
    if config.region is not None:
        return Located(region=config.region, output=None, window=None)
    windows = niri_json("windows")
    if config.window_id is not None:
        window = next((w for w in windows if w.get("id") == config.window_id), None)
        if window is None:
            raise LocateError(f"niri has no window with id {config.window_id}")
    else:
        window = find_window(windows, hint)
    region, output = window_region(window, niri_json("workspaces"), niri_json("outputs"))
    if hint.client_size is not None and (region.width, region.height) != hint.client_size:
        log.warning(
            "capture: the window is %dx%d in niri but %dx%d to the game",
            region.width,
            region.height,
            *hint.client_size,
        )
    if output.get("logical", {}).get("scale", 1.0) != 1.0:
        log.warning("capture: output %s is scaled; the region is in logical pixels", output["name"])
    return Located(
        region=region,
        output={
            "name": output.get("name"),
            "logical": output.get("logical"),
        },
        window={
            "id": window.get("id"),
            "title": window.get("title"),
            "app_id": window.get("app_id"),
            "is_floating": window.get("is_floating"),
        },
    )


class Connection:
    """The control connection: line reads with a deadline, and best-effort sends."""

    def __init__(self, sock: socket.socket):
        self.sock = sock
        self.reader = LineReader()
        self.pending: list[bytes] = []
        self.closed = False

    def poll(self, timeout: float) -> list[bytes]:
        """Lines that arrive within `timeout`; sets `closed` when the peer closes."""
        if self.pending or self.closed:
            lines, self.pending = self.pending, []
            return lines
        self.sock.settimeout(timeout)
        try:
            data = self.sock.recv(4096)
        except TimeoutError:
            return []
        except ConnectionError:
            data = b""
        if not data:
            self.closed = True
            return []
        return self.reader.feed(data)

    def read_line(self, timeout: float) -> bytes | None:
        deadline = time.monotonic() + timeout
        while not self.closed:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return None
            lines = self.poll(remaining)
            if lines:
                self.pending = lines[1:]
                return lines[0]
        return None

    def send(self, message: dict) -> None:
        try:
            self.sock.settimeout(5.0)
            self.sock.sendall(encode(message))
        except OSError as error:
            log.warning("capture: could not send %s to the payload: %s", message.get("type"), error)


def serve(server: socket.socket, config: Config) -> None:
    host, port = server.getsockname()[:2]
    with server:
        log.info("capture: listening on %s:%d", host, port)
        while True:
            sock, peer = server.accept()
            with sock:
                log.info("capture: connection from %s:%d", *peer[:2])
                run_take(Connection(sock), config)
            log.info("capture: listening on %s:%d", host, port)


def run_take(connection: Connection, config: Config) -> None:
    line = connection.read_line(START_TIMEOUT_S)
    if line is None:
        log.warning("capture: the connection sent no start line")
        return
    try:
        start = parse_message(line)
        if not isinstance(start, Start):
            raise ProtocolError("the first message must be a start")
    except ProtocolError as error:
        refuse(connection, f"capture: {error}")
        return
    folder = start.folder or os.path.join(
        config.fallback_dir, start.take or time.strftime("%Y-%m-%d_%H-%M-%S")
    )
    try:
        os.makedirs(folder, exist_ok=True)
    except OSError as error:
        refuse(connection, f"capture: could not create {folder}: {error}")
        return
    try:
        located = locate(config, start.window)
    except LocateError as error:
        refuse(connection, f"capture: the game window: {error}")
        return
    video, stream = take_paths(folder)
    command = build_command(
        config.gsr, located.region, video, config.fps, config.quality, config.audio, config.extra
    )
    log.info("capture: recording %s to %s", located.region.gsr_arg(), video)
    spawn_ms = unix_ms()
    try:
        recorder = subprocess.Popen(
            command, stdin=subprocess.DEVNULL, start_new_session=True, env=recorder_env()
        )
    except OSError as error:
        refuse(connection, f"capture: could not start {config.gsr}: {error}")
        return
    metadata = {
        "format": STREAM_FORMAT,
        "version": STREAM_VERSION,
        "take": start.take,
        "video": os.path.basename(video),
        "video_start_unix_ms": None,
        "spawn_unix_ms": spawn_ms,
        "stop_unix_ms": None,
        "exit_code": None,
        "region": located.region.to_json(),
        "output": located.output,
        "window": located.window,
        "command": command,
    }
    try:
        record(connection, recorder, video, stream, metadata)
    except KeyboardInterrupt:
        log.info("capture: interrupted; finalizing the take")
        finish(recorder, stream, metadata)
        raise


def recorder_env() -> dict:
    """The relay's environment without the GPU-selection overrides."""
    env = dict(os.environ)
    for name in GPU_OVERRIDES:
        if env.pop(name, None) is not None:
            log.info("capture: not passing %s to %s", name, GSR)
    return env


def refuse(connection: Connection, message: str) -> None:
    log.error("%s", message)
    connection.send({"type": "error", "message": message})


def record(
    connection: Connection, recorder: subprocess.Popen, video: str, stream: str, metadata: dict
) -> None:
    deadline = time.monotonic() + FIRST_FRAME_TIMEOUT_S
    start_ms = None
    while start_ms is None and time.monotonic() < deadline and recorder.poll() is None:
        time.sleep(POLL_S)
        start_ms = read_first_frame_ts(video + ".ts")
    if recorder.poll() is not None:
        refuse(connection, f"capture: {GSR} exited with status {recorder.returncode} at start")
        return
    if start_ms is None:
        log.warning("capture: no first-frame time yet; stream.json will be updated at the stop")
    metadata["video_start_unix_ms"] = start_ms
    write_metadata(stream, metadata)
    connection.send(
        {
            "type": "started",
            "file": video,
            "video_start_unix_ms": start_ms,
            "region": metadata["region"],
            "output": (metadata["output"] or {}).get("name"),
        }
    )
    stop_requested = False
    while not connection.closed:
        lines = connection.poll(POLL_S)
        if recorder.poll() is not None:
            refuse(connection, f"capture: {GSR} exited with status {recorder.returncode}")
            break
        for line in lines:
            try:
                message = parse_message(line)
            except ProtocolError as error:
                log.warning("capture: ignoring a control line: %s", error)
                continue
            if isinstance(message, Stop):
                stop_requested = True
        if stop_requested:
            break
    log.info("capture: the take ended (%s)", "stop" if stop_requested else "connection closed")
    code = finish(recorder, stream, metadata)
    if stop_requested:
        connection.send({"type": "stopped", "exit_code": code})


def finish(recorder: subprocess.Popen, stream: str, metadata: dict) -> int | None:
    """Stop gpu-screen-recorder with SIGINT, wait for it to finalize, and complete the metadata."""
    metadata["stop_unix_ms"] = unix_ms()
    if recorder.poll() is None:
        recorder.send_signal(signal.SIGINT)
        try:
            recorder.wait(FINALIZE_TIMEOUT_S)
        except subprocess.TimeoutExpired:
            log.error("capture: %s did not finalize in time; killing it", GSR)
            recorder.kill()
            recorder.wait()
    code = recorder.returncode
    if code != 0:
        log.error("capture: %s exited with status %d", GSR, code)
    metadata["exit_code"] = code
    if metadata["video_start_unix_ms"] is None:
        video = os.path.join(os.path.dirname(stream), metadata["video"])
        metadata["video_start_unix_ms"] = read_first_frame_ts(video + ".ts")
    write_metadata(stream, metadata)
    return code


def write_metadata(path: str, metadata: dict) -> None:
    temporary = path + ".tmp"
    try:
        with open(temporary, "w") as f:
            json.dump(metadata, f, indent=2)
            f.write("\n")
        os.replace(temporary, path)
    except OSError as error:
        log.error("capture: could not write %s: %s", path, error)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    parser.add_argument("--fallback-dir", default="captures")
    parser.add_argument("--fps", type=int, default=60)
    parser.add_argument("--quality", default="high")
    parser.add_argument("--audio", default="default_output", help='a gsr audio input, or "none"')
    target = parser.add_mutually_exclusive_group()
    target.add_argument("--window-id", type=int, help="record this niri window")
    target.add_argument("--region", help="record this WxH+X+Y region without asking niri")
    parser.add_argument("extra", nargs=argparse.REMAINDER, help="after --: extra gsr arguments")
    args = parser.parse_args()
    extra = args.extra[1:] if args.extra[:1] == ["--"] else args.extra
    try:
        region = parse_region(args.region) if args.region else None
    except ValueError as error:
        parser.error(str(error))
    config = Config(
        fallback_dir=os.path.abspath(args.fallback_dir),
        fps=args.fps,
        quality=args.quality,
        audio=args.audio,
        extra=extra,
        window_id=args.window_id,
        region=region,
    )

    logging.basicConfig(level=logging.INFO, format="%(message)s", stream=sys.stderr)
    signal.signal(signal.SIGTERM, _interrupt)
    if shutil.which(GSR) is None:
        log.error("capture: %s is not on PATH", GSR)
        return 1
    try:
        server = socket.create_server((args.host, args.port))
    except OSError as error:
        log.error("capture: could not listen on %s:%d: %s", args.host, args.port, error)
        return 1
    try:
        serve(server, config)
    except KeyboardInterrupt:
        log.info("capture: stopped")
    return 0


def _interrupt(signum, frame):
    raise KeyboardInterrupt


if __name__ == "__main__":
    sys.exit(main())
