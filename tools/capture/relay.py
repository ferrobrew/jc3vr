#!/usr/bin/env python3
"""The capture host's relay: listen for the payload's yuv4mpegpipe stream, start ffmpeg for each
connection with the recording's own folder as its output, forward the stream to it, and go back to
listening when the recording ends.

    relay.py [--host 127.0.0.1] [--port 47172] [--fallback-dir captures] -- FFMPEG ARGS...

The ffmpeg command line after `--` reads the stream from `pipe:0` and holds the token `@OUTPUT@`
exactly once where the output belongs; the relay replaces it with a segment start number and
`<folder>/video_%03d.mkv`. `scripts/capture_host.sh` builds that command (encoder, quality, colour
tags, audio, segmenting) and runs this.

The folder comes from the stream header: the payload adds an `XJC3VR_DIR=<path>` parameter naming
the recording's folder as the host sees it, percent-encoded because header parameters are separated
by spaces. The relay removes the parameter before ffmpeg sees the header, since ffmpeg's y4m
demuxer refuses a header longer than about a hundred bytes. A stream without the parameter (the
payload on Windows, or an older build) goes to a stamped folder under the fallback directory.
Segment numbers continue from the highest `video_NNN.mkv` already in the folder, so a second
connection for the same recording does not overwrite the first's segments.

ffmpeg runs in its own session, so Ctrl+C reaches only the relay, which then closes ffmpeg's input
and waits for it to finalize the current segment; a second Ctrl+C asks ffmpeg to stop at once.
Standard library only; run it with `python3 -I`.
"""

import argparse
import logging
import os
import re
import signal
import socket
import subprocess
import sys
import time
import urllib.parse
from dataclasses import dataclass

DEFAULT_PORT = 47172
OUTPUT_TOKEN = "@OUTPUT@"
FOLDER_TAG = b"XJC3VR_DIR="
Y4M_MAGIC = b"YUV4MPEG2 "
SEGMENT_NAME = "video_%03d.mkv"
# Generous: a percent-encoded path at the longest Linux allows, plus the plain parameters.
HEADER_LIMIT = 16 * 1024
CHUNK = 1 << 20

log = logging.getLogger("capture")


class HeaderError(Exception):
    """The connection did not open with a y4m stream header."""


@dataclass(frozen=True)
class Header:
    """A parsed stream header."""

    # The header line to hand to ffmpeg: the original without the folder parameter.
    forward: bytes
    # The recording folder the header named, or None without the parameter.
    folder: str | None


def parse_header(line: bytes) -> Header:
    """Split the folder parameter out of a y4m header line (newline included)."""
    if not line.startswith(Y4M_MAGIC) or not line.endswith(b"\n"):
        raise HeaderError(f"not a y4m stream header: {line[:40]!r}")
    kept = []
    folder = None
    for token in line[:-1].split(b" "):
        if token.startswith(FOLDER_TAG):
            folder = decode_folder(token[len(FOLDER_TAG) :])
        else:
            kept.append(token)
    return Header(forward=b" ".join(kept) + b"\n", folder=folder)


def decode_folder(value: bytes) -> str:
    """Decode a percent-encoded absolute Unix path from the header."""
    path = os.fsdecode(urllib.parse.unquote_to_bytes(value))
    if not path.startswith("/"):
        raise HeaderError(f"the folder parameter is not an absolute path: {path!r}")
    if "\0" in path:
        raise HeaderError("the folder parameter holds a NUL")
    return path


def next_segment_number(folder: str) -> int:
    """One past the highest `video_NNN.mkv` in `folder`, or 0."""
    pattern = re.compile(r"video_(\d+)\.mkv")
    try:
        names = os.listdir(folder)
    except FileNotFoundError:
        return 0
    numbers = [int(m.group(1)) for name in names if (m := pattern.fullmatch(name))]
    return max(numbers, default=-1) + 1


def build_command(template: list[str], folder: str, start_number: int) -> list[str]:
    """The ffmpeg command for one connection: the template with the output token replaced."""
    if template.count(OUTPUT_TOKEN) != 1:
        raise ValueError(f"the ffmpeg command must hold {OUTPUT_TOKEN} exactly once")
    command = []
    for arg in template:
        if arg == OUTPUT_TOKEN:
            command += [
                "-segment_start_number",
                str(start_number),
                os.path.join(folder, SEGMENT_NAME),
            ]
        else:
            command.append(arg)
    return command


def fallback_folder(fallback_dir: str) -> str:
    return os.path.join(fallback_dir, time.strftime("%Y-%m-%d_%H-%M-%S"))


def serve(server: socket.socket, fallback_dir: str, template: list[str]) -> None:
    host, port = server.getsockname()[:2]
    with server:
        log.info("capture: listening on %s:%d", host, port)
        while True:
            connection, peer = server.accept()
            with connection:
                log.info("capture: connection from %s:%d", *peer[:2])
                relay(connection, fallback_dir, template)
            log.info("capture: listening on %s:%d", host, port)


def relay(connection: socket.socket, fallback_dir: str, template: list[str]) -> None:
    reader = connection.makefile("rb", buffering=CHUNK)
    line = reader.readline(HEADER_LIMIT)
    if not line:
        log.warning("capture: the connection closed before the stream header")
        return
    try:
        header = parse_header(line)
    except HeaderError as error:
        log.error("capture: dropping the connection: %s", error)
        return
    if header.folder is None:
        folder = fallback_folder(fallback_dir)
        log.info("capture: the stream names no folder; writing to %s", folder)
    else:
        folder = header.folder
        log.info("capture: writing to %s", folder)
    try:
        os.makedirs(folder, exist_ok=True)
    except OSError as error:
        log.error("capture: dropping the connection: could not create %s: %s", folder, error)
        return
    command = build_command(template, folder, next_segment_number(folder))
    try:
        ffmpeg = subprocess.Popen(command, stdin=subprocess.PIPE, start_new_session=True)
    except OSError as error:
        log.error("capture: dropping the connection: could not start %s: %s", command[0], error)
        return
    try:
        ffmpeg.stdin.write(header.forward)
        while chunk := reader.read1(CHUNK):
            ffmpeg.stdin.write(chunk)
        log.info("capture: the stream ended")
    except BrokenPipeError:
        log.error("capture: ffmpeg stopped reading the stream")
    except ConnectionError as error:
        log.warning("capture: the connection dropped: %s", error)
    finally:
        finish(ffmpeg)


def finish(ffmpeg: subprocess.Popen) -> None:
    """Close ffmpeg's input so it finalizes the segment, and wait for it; on a second interrupt
    while waiting, ask it to stop at once (it still finalizes on SIGTERM)."""
    try:
        ffmpeg.stdin.close()
    except BrokenPipeError:
        pass
    try:
        code = ffmpeg.wait()
    except KeyboardInterrupt:
        log.warning("capture: interrupted again; stopping ffmpeg")
        ffmpeg.terminate()
        code = ffmpeg.wait()
        raise
    if code != 0:
        log.error("capture: ffmpeg exited with status %d", code)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    parser.add_argument("--fallback-dir", default="captures")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    template = args.command[1:] if args.command[:1] == ["--"] else args.command
    if template.count(OUTPUT_TOKEN) != 1:
        parser.error(f"the ffmpeg command after -- must hold {OUTPUT_TOKEN} exactly once")

    logging.basicConfig(level=logging.INFO, format="%(message)s", stream=sys.stderr)
    signal.signal(signal.SIGTERM, _interrupt)
    try:
        server = socket.create_server((args.host, args.port))
    except OSError as error:
        log.error("capture: could not listen on %s:%d: %s", args.host, args.port, error)
        return 1
    try:
        serve(server, args.fallback_dir, template)
    except KeyboardInterrupt:
        log.info("capture: stopped")
    return 0


def _interrupt(signum, frame):
    raise KeyboardInterrupt


if __name__ == "__main__":
    sys.exit(main())
