"""Validation of capture artifacts written by the game."""

import dataclasses
import json
import struct
import zlib
from pathlib import Path

from tools.rendering.comparison.stages import artifact_path
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
                     generation: int | None = None, motion_capture: PhaseCapture | None = None,
                     water_inputs: bool = False, sun_inputs: bool = False,
                     shader_identity: bool = False, resolve_inputs: bool = False,
                     sun_depth_clip=None, water_reflection=None) -> NativeCapture | Capture:
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
        if metadata.get("sun_depth_clip") != sun_depth_clip:
            raise ProtocolError("visual protocol: screenshot sun depth clip request mismatch")
        if sun_depth_clip is not None and mode == Mode.SINGLE_PASS:
            treatments = [record for record in metadata.get("stages", [])
                          if isinstance(record, dict) and record.get("stage") == "sun_depth_clip_treatment"]
            if (len(treatments) != 1 or treatments[0].get("eye") != 1 or
                    treatments[0].get("error") is not None or
                    treatments[0].get("frame") != metadata.get("predicted_display_time_ns") or
                    (treatments[0].get("sun_input") or {}).get("capture_id") != capture_id):
                raise ProtocolError("visual protocol: right-eye sun depth clip treatment was not applied")
        if metadata.get("water_reflection") != water_reflection:
            raise ProtocolError("visual protocol: screenshot water reflection request mismatch")
        if water_reflection is not None:
            validate_water_treatment(folder, capture_id, metadata, mode)
        if resolve_inputs:
            validate_resolve_inputs(folder, capture_id, metadata, mode, shader_identity)
        if water_inputs:
            validate_water_inputs(folder, capture_id, metadata, mode, shader_identity)
        if sun_inputs:
            validate_sun_inputs(folder, capture_id, metadata, mode)
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


def validate_water_treatment(folder: Path, capture_id: str, metadata: dict, mode: Mode) -> None:
    if mode != Mode.MULTIPASS or not isinstance(metadata.get("stages"), list):
        raise ProtocolError("visual protocol: water reflection treatment requires multipass stages")
    records = metadata["stages"]
    for eye in (0, 1):
        treatment = [r for r in records if isinstance(r, dict) and
                     r.get("stage") == "water_reflection_treatment" and r.get("eye") == eye]
        if len(treatment) != 1:
            raise ProtocolError("visual protocol: missing or duplicate water treatment")
        record = treatment[0]
        info = record.get("water_input")
        if (record.get("error") is not None or record.get("frame") != metadata.get("predicted_display_time_ns") or
                not isinstance(info, dict) or info.get("capture_id") != capture_id or
                type(info.get("treatment_draws")) is not int or info["treatment_draws"] <= 0):
            raise ProtocolError("visual protocol: water reflection treatment was not applied")
        inputs = [r for r in records if isinstance(r, dict) and
                  r.get("stage") == "water_input_reflection_alpha" and r.get("eye") == eye]
        if len(inputs) != 1 or not isinstance(inputs[0].get("source"), dict):
            raise ProtocolError("visual protocol: missing treated reflection weight input")
        source = inputs[0]["source"]
        if (source.get("width"), source.get("height"), source.get("bytes_per_pixel"), source.get("format_id")) != (1, 1, 4, 41):
            raise ProtocolError("visual protocol: treated reflection weight has unexpected dimensions or format")
        try:
            raw = inputs[0].get("raw")
            if not isinstance(raw, str) or artifact_path(folder / f"{capture_id}.capture.json", raw).read_bytes() != bytes(4):
                raise ProtocolError("visual protocol: treated reflection weight is not zero")
        except (OSError, ValueError) as error:
            raise ProtocolError(f"visual protocol: invalid treated reflection artifact: {error}") from error


def validate_resolve_inputs(folder: Path, capture_id: str, metadata: dict, mode: Mode, shader_identity: bool) -> None:
    capture = metadata.get("resolve_inputs")
    if (not isinstance(capture, dict) or type(capture.get("frame")) is not int or
            capture["frame"] != metadata.get("predicted_display_time_ns") or
            type(capture.get("generation")) is not int or not isinstance(metadata.get("visual"), dict) or
            capture["generation"] != metadata["visual"].get("generation") or
            not isinstance(capture.get("eyes"), list) or len(capture["eyes"]) != 2):
        raise ProtocolError("visual protocol: invalid resolve capture identity")
    for eye_index, (eye, required) in enumerate(zip(capture["eyes"], mode.rendered_eyes)):
        if not isinstance(eye, dict) or eye.get("status") != ("observed" if required else "not_required"):
            raise ProtocolError("visual protocol: requested resolve eye is missing or ambiguous")
        if not required:
            continue
        observation = eye.get("observation")
        if (not isinstance(observation, dict) or not isinstance(observation.get("pixel_shader"), dict) or
                type(observation["pixel_shader"].get("pointer")) is not int or observation["pixel_shader"]["pointer"] <= 0):
            raise ProtocolError("visual protocol: invalid resolve shader observation")
        counters = observation.get("frame_counters")
        if not isinstance(counters, dict) or any(type(counters.get(key)) is not int or not 0 <= counters[key] <= 0xffffffff
                                                 for key in ("counter", "frame_index", "ring_index")):
            raise ProtocolError("visual protocol: invalid resolve frame counters")
        buffers = observation.get("constant_buffers")
        if not isinstance(buffers, list) or len(buffers) != 4:
            raise ProtocolError("visual protocol: invalid resolve constant buffer list")
        for slot, buffer in enumerate(buffers):
            if (not isinstance(buffer, dict) or type(buffer.get("slot")) is not int or buffer["slot"] != slot or
                    any(type(buffer.get(key)) is not int or buffer[key] < 0 for key in ("pointer", "byte_width"))):
                raise ProtocolError("visual protocol: invalid resolve constant buffer")
            first, count = buffer.get("first_constant"), buffer.get("num_constants")
            if (first is None) != (count is None) or (first is not None and
                    (type(first) is not int or type(count) is not int or first < 0 or count < 0 or
                     first > 4096 or count > 4096)):
                raise ProtocolError("visual protocol: invalid resolve constant buffer range")
            if (buffer["pointer"] == 0) != (buffer["byte_width"] == 0):
                raise ProtocolError("visual protocol: inconsistent resolve constant buffer binding")
            if slot in (0, 3):
                content = buffer.get("content")
                if not isinstance(content, dict) or not isinstance(content.get("status"), str):
                    raise ProtocolError("visual protocol: missing resolve constant buffer content status")
                status = content["status"]
                if status == "captured":
                    raw = content.get("raw")
                    if not isinstance(raw, str) or first is None or count is None or count == 0 or count * 16 > 65536 or first * 16 + count * 16 > buffer["byte_width"]:
                        raise ProtocolError("visual protocol: invalid resolve constant buffer capture")
                    try:
                        path = artifact_path(folder / f"{capture_id}.capture.json", raw)
                        if path.name != f"{capture_id}.resolve_cb{slot}.eye{eye_index}.raw" or path.stat().st_size != count * 16:
                            raise ProtocolError("visual protocol: resolve constant buffer artifact mismatch")
                    except (OSError, ValueError) as error:
                        raise ProtocolError(f"visual protocol: invalid resolve constant buffer artifact: {error}") from error
                elif status not in ("unbound", "unknown_range", "empty_range", "oversize", "invalid_range", "read_error"):
                    raise ProtocolError("visual protocol: unknown resolve constant buffer content status")
                elif status == "read_error" and not isinstance(content.get("error"), str):
                    raise ProtocolError("visual protocol: invalid resolve constant buffer error")
            elif "content" in buffer:
                raise ProtocolError("visual protocol: unexpected resolve constant buffer content")
        resources = observation.get("resources")
        slots = (*range(15), *range(16, 32), 44, 45)
        if not isinstance(resources, list) or len(resources) != len(slots):
            raise ProtocolError("visual protocol: invalid resolve resource list")
        for slot, resource in zip(slots, resources):
            if (not isinstance(resource, dict) or type(resource.get("slot")) is not int or resource["slot"] != slot or
                    any(type(resource.get(key)) is not int or resource[key] < 0 for key in
                        ("view", "resource", "format", "dimension"))):
                raise ProtocolError("visual protocol: invalid resolve resource binding")
            texture = resource.get("texture")
            if resource["view"] == 0 and (resource["resource"] != 0 or texture is not None):
                raise ProtocolError("visual protocol: inconsistent resolve resource binding")
            if texture is not None and (not isinstance(texture, dict) or any(
                    type(texture.get(key)) is not int or texture[key] <= 0
                    for key in ("width", "height", "depth", "mip_levels", "array_size", "format"))):
                raise ProtocolError("visual protocol: invalid resolve texture description")
        identity = observation["pixel_shader"].get("identity")
        if shader_identity and not valid_mapped_identity(identity):
            raise ProtocolError("visual protocol: resolve pixel shader identity is unmapped")


def valid_mapped_identity(identity) -> bool:
    return (isinstance(identity, dict) and identity.get("status") == "mapped" and
            isinstance(identity.get("identity"), dict) and
            all(isinstance(identity["identity"].get(key), str) and
                len(identity["identity"][key]) == 64 and
                all(ch in "0123456789abcdef" for ch in identity["identity"][key])
                for key in ("pristine_sha256", "actual_sha256")))


def validate_water_inputs(folder: Path, capture_id: str, metadata: dict, mode: Mode,
                          shader_identity: bool = False) -> None:
    slots = {"water_input_reflection": ("reflection", 1),
             "water_input_reflection_alpha": ("reflection_alpha", 12),
             "water_input_refraction": ("refraction", 3)}
    records = metadata.get("stages")
    if not isinstance(records, list):
        raise ProtocolError("visual protocol: requested water inputs have no stage records")
    expected = {(stage, eye) for stage in slots for eye, required in enumerate(mode.rendered_eyes) if required}
    seen = set()
    for record in records:
        if not isinstance(record, dict):
            raise ProtocolError("visual protocol: invalid water input stage record")
        stage = record.get("stage")
        if not isinstance(stage, str):
            raise ProtocolError("visual protocol: invalid stage name")
        if stage not in slots:
            continue
        eye = record.get("eye")
        if type(eye) is not int or (stage, eye) not in expected or (stage, eye) in seen:
            raise ProtocolError("visual protocol: unexpected or duplicate water input stage")
        seen.add((stage, eye))
        info = record.get("water_input")
        source = record.get("source")
        if (not isinstance(info, dict) or info.get("capture_id") != capture_id or
                info.get("slot") != slots[stage][0] or info.get("shader_slot") != slots[stage][1] or
                type(info.get("pixel_shader")) is not int or info["pixel_shader"] <= 0 or
                type(info.get("draw_index")) is not int or info["draw_index"] < 0 or
                type(info.get("tessellated")) is not bool or
                any(type(info.get(key)) is not int for key in
                    ("view_format", "view_dimension", "most_detailed_mip", "mip_levels", "resource", "resource_format")) or
                type(info.get("resource")) is not int or info["resource"] <= 0 or
                info["view_dimension"] != 4 or info["most_detailed_mip"] < 0 or info["mip_levels"] <= 0 or
                record.get("error") is not None or not isinstance(source, dict)):
            raise ProtocolError("visual protocol: invalid requested water input metadata")
        identity = info.get("shader_identity")
        if shader_identity and not valid_mapped_identity(identity):
            raise ProtocolError("visual protocol: water pixel shader identity is unmapped or incomplete")
        if type(source.get("format_id")) is not int or source["format_id"] != info["view_format"] or (info["resource_format"], info["view_format"]) not in ((26, 26), (41, 41), (39, 41)):
            raise ProtocolError("visual protocol: water input source format disagrees with its view")
        width, height, pixel_bytes = (source.get(key) for key in ("width", "height", "bytes_per_pixel"))
        if any(type(value) is not int or value <= 0 for value in (width, height, pixel_bytes)):
            raise ProtocolError("visual protocol: invalid water input dimensions or format")
        raw = record.get("raw")
        if not isinstance(raw, str):
            raise ProtocolError("visual protocol: requested water input has no raw artifact")
        try:
            path = artifact_path(folder / f"{capture_id}.capture.json", raw)
            if path.stat().st_size != width * height * pixel_bytes:
                raise ProtocolError("visual protocol: water input artifact size mismatch")
        except (OSError, ValueError) as error:
            raise ProtocolError(f"visual protocol: invalid water input artifact: {error}") from error
    if seen != expected:
        raise ProtocolError("visual protocol: requested water input stages are missing")


def validate_sun_inputs(folder: Path, capture_id: str, metadata: dict, mode: Mode) -> None:
    stages = {"pre_sun_color", "pre_sun_depth"}
    records = metadata.get("stages")
    if not isinstance(records, list):
        raise ProtocolError("visual protocol: requested sun inputs have no stage records")
    expected = {(stage, eye) for stage in stages for eye, required in enumerate(mode.rendered_eyes) if required}
    seen = set()
    for record in records:
        if not isinstance(record, dict) or not isinstance(record.get("stage"), str):
            raise ProtocolError("visual protocol: invalid sun input stage record")
        stage = record["stage"]
        if stage not in stages:
            continue
        eye = record.get("eye")
        if type(eye) is not int or (stage, eye) not in expected or (stage, eye) in seen:
            raise ProtocolError("visual protocol: unexpected or duplicate sun input stage")
        seen.add((stage, eye))
        info, source = record.get("sun_input"), record.get("source")
        if (not isinstance(info, dict) or info.get("capture_id") != capture_id or
                any(type(info.get(key)) is not int for key in
                    ("view", "resource", "view_format", "view_dimension", "mip")) or
                info["view"] <= 0 or info["resource"] <= 0 or info["mip"] != 0 or
                record.get("error") is not None or not isinstance(source, dict)):
            raise ProtocolError("visual protocol: invalid requested sun input metadata")
        source_format = source.get("format_id")
        if type(source_format) is not int:
            raise ProtocolError("visual protocol: invalid sun input source format")
        valid_format = ((info["view_format"] == 26 and source_format == 26) if stage == "pre_sun_color"
                        else (info["view_format"], source_format) in ((20, 19), (20, 20), (40, 39), (40, 40), (45, 44), (45, 45), (55, 53), (55, 55)))
        if not valid_format:
            raise ProtocolError("visual protocol: unsupported sun input resource/view format combination")
        width, height, pixel_bytes = (source.get(key) for key in ("width", "height", "bytes_per_pixel"))
        if any(type(value) is not int or value <= 0 for value in (width, height, pixel_bytes)):
            raise ProtocolError("visual protocol: invalid sun input dimensions or format")
        raw = record.get("raw")
        if not isinstance(raw, str):
            raise ProtocolError("visual protocol: requested sun input has no raw artifact")
        try:
            path = artifact_path(folder / f"{capture_id}.capture.json", raw)
            if path.stat().st_size != width * height * pixel_bytes:
                raise ProtocolError("visual protocol: sun input artifact size mismatch")
        except (OSError, ValueError) as error:
            raise ProtocolError(f"visual protocol: invalid sun input artifact: {error}") from error
    if seen != expected:
        raise ProtocolError("visual protocol: requested sun input stages are missing")


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
