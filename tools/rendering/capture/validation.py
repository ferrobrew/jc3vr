"""Validation of capture artifacts written by the game."""

import dataclasses
import json
import struct
import zlib
from pathlib import Path

from tools.rendering.protocol import (
    Capture,
    Mode,
    NativeCapture,
    PhaseCapture,
    ProtocolError,
)
from tools.rendering.session.errors import SessionError


def read_object(path):
    try:
        value = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise SessionError(f"capture: invalid JSON artifact {path}: {error}") from error
    if not isinstance(value, dict):
        raise SessionError(f"capture: expected a JSON object in {path}")
    return value


def validate_capture(folder: Path, capture_id: str, result: NativeCapture | Capture, mode: Mode | None = None,
                     generation: int | None = None, motion_capture: PhaseCapture | None = None) -> NativeCapture | Capture:
    metadata = read_object(folder / f"{capture_id}.capture.json")
    if motion_capture is not None and mode is None:
        raise ProtocolError("visual protocol: motion capture requires a diagnostic mode")
    if mode is not None:
        if not isinstance(result, Capture):
            raise ProtocolError("visual protocol: expected a diagnostic capture")
        if Capture.from_wire(metadata) != result:
            raise ProtocolError("visual protocol: screenshot metadata disagrees with its acknowledgement")
        result.validate(capture_id, mode, generation, validate_png(folder / f"{capture_id}.capture.png"),
                        motion_capture=motion_capture)
        return result
    capture = NativeCapture(*(getattr(result, item.name) for item in dataclasses.fields(NativeCapture)))
    if NativeCapture.from_wire(metadata) != capture:
        raise ProtocolError("visual protocol: native capture metadata disagrees with its acknowledgement")
    if capture.id != capture_id or not capture.gameplay_ready or not capture.xr_session_running:
        raise SessionError("capture: screenshot is not a gameplay XR capture")
    if capture.fresh_eyes != (True, True):
        raise SessionError("capture: screenshot is not a fresh gameplay XR pair")
    if capture.source not in ("eye_textures", "collapsed_back_buffer"):
        raise SessionError("capture: screenshot has an unknown source")
    width, height = validate_png(folder / f"{capture_id}.capture.png")
    if (width, height) != (capture.width, capture.height) or width != capture.eye_width * 2 or height != capture.eye_height:
        raise SessionError("capture: PNG dimensions disagree with the eye metadata")
    return capture


def validate_png(path):
    data = path.read_bytes()
    if len(data) > 128 * 1024 * 1024 or data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SessionError(f"capture: invalid PNG signature or size: {path}")
    offset = 8
    header = None
    compressed = bytearray()
    ended = False
    idat_closed = False
    while offset < len(data):
        if offset + 12 > len(data):
            raise SessionError("capture: truncated PNG chunk")
        size, kind = struct.unpack_from(">I4s", data, offset)
        end = offset + 12 + size
        if end > len(data):
            raise SessionError("capture: truncated PNG payload")
        payload = data[offset + 8:end - 4]
        crc = struct.unpack_from(">I", data, end - 4)[0]
        if zlib.crc32(kind + payload) & 0xffffffff != crc:
            raise SessionError("capture: PNG chunk CRC mismatch")
        if header is None and kind != b"IHDR":
            raise SessionError("capture: PNG does not begin with IHDR")
        if kind == b"IHDR":
            if header is not None or size != 13:
                raise SessionError("capture: invalid PNG IHDR")
            header = struct.unpack(">IIBBBBB", payload)
        elif kind == b"IDAT":
            if idat_closed:
                raise SessionError("capture: nonconsecutive PNG IDAT chunks")
            compressed.extend(payload)
        elif kind == b"IEND":
            if size or end != len(data):
                raise SessionError("capture: invalid PNG IEND or trailing bytes")
            ended = True
            break
        else:
            if compressed:
                idat_closed = True
            if kind[:1].isupper():
                raise SessionError(f"capture: unsupported critical PNG chunk {kind!r}")
        offset = end
    if header is None or not ended or not compressed:
        raise SessionError("capture: PNG is incomplete")
    width, height, depth, color, compression, filtering, interlace = header
    if not (0 < width <= 16384 and 0 < height <= 16384) or depth != 8 or color not in (2, 6) or (compression, filtering, interlace) != (0, 0, 0):
        raise SessionError("capture: unsupported PNG layout")
    stride = width * (3 if color == 2 else 4) + 1
    expected = stride * height
    if expected > 256 * 1024 * 1024:
        raise SessionError("capture: PNG decoded image is too large")
    decoder = zlib.decompressobj()
    try:
        pixels = decoder.decompress(compressed, expected + 1)
    except zlib.error as error:
        raise SessionError(f"capture: corrupt PNG deflate stream: {error}") from error
    if len(pixels) != expected or not decoder.eof or decoder.unused_data or decoder.unconsumed_tail:
        raise SessionError("capture: PNG decoded length or zlib stream is invalid")
    if any(pixels[row * stride] > 4 for row in range(height)):
        raise SessionError("capture: invalid PNG row filter")
    return width, height
