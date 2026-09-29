"""Read vertex positions from RBMDL render models (``.rbm``) made of ``GeneralJC3`` blocks.

The header is ``u32 5``, ``RBMDL``, ``u32`` major, minor, and revision (1, 16, and a revision),
``f32`` bounding box min and max, ``u32`` block count, and one more ``u32``. The blocks follow, each
ending in the ``0x89ABCDEF`` footer. A block starts with its ``u32`` type and a ``u8`` version; for
``GeneralJC3`` (type ``0x2CEC5AD5``, version 5) the position dequantisation scale is the ``f32`` at
the version byte plus ``0x29``. After the material (``u32``-length-prefixed texture paths) come the
vertex buffers: ``u32 n`` and ``n`` ``i16x4`` positions (a component divided by 32767 and multiplied
by the scale), then ``u32 n`` and ``n`` 20-byte records of packed UVs, normal, and tangent. Indices
and optional extra data follow, and their layout varies by model.

Only positions are read. The reader locates the position buffer by its ``n``, ``8n`` bytes, ``n``
chain, and validates the result against the header's bounding box.
"""

import struct

BLOCK_GENERAL_JC3 = 0x2CEC5AD5
FOOTER = struct.pack("<I", 0x89ABCDEF)
# The largest gap between the positions' extent and the header's bounding box, in metres.
BBOX_TOLERANCE = 2e-3


class UnsupportedBlock(ValueError):
    pass


def positions(data: bytes, name: str) -> list[tuple[float, float, float]]:
    if struct.unpack_from("<I5s", data, 0) != (5, b"RBMDL"):
        raise ValueError(f"render model {name}: not an RBMDL file")
    major, minor, _revision = struct.unpack_from("<3I", data, 9)
    if (major, minor) != (1, 16):
        raise ValueError(f"render model {name}: unsupported version {major}.{minor}")
    bbox = struct.unpack_from("<6f", data, 0x15)
    (count,) = struct.unpack_from("<I", data, 0x2D)
    start = 0x35
    out = []
    for _ in range(count):
        end = data.index(FOOTER, start)
        # The footer value can occur inside vertex data, so take the first end at which the block
        # parses.
        while True:
            try:
                out += _block(data, start, end, name)
                break
            except _NoPositionBuffer:
                end = data.index(FOOTER, end + 1)
        start = end + 4
    for axis in range(3):
        lo = min(p[axis] for p in out)
        hi = max(p[axis] for p in out)
        if abs(lo - bbox[axis]) > BBOX_TOLERANCE or abs(hi - bbox[axis + 3]) > BBOX_TOLERANCE:
            raise ValueError(f"render model {name}: positions span {lo:.4f}..{hi:.4f} on axis {axis}, outside the header's bounding box")
    return out


class _NoPositionBuffer(ValueError):
    pass


def _block(data: bytes, start: int, end: int, name: str):
    block_type, version = struct.unpack_from("<IB", data, start)
    if block_type != BLOCK_GENERAL_JC3 or version != 5:
        raise UnsupportedBlock(f"render model {name}: unsupported block type {block_type:#010x} version {version}")
    (scale,) = struct.unpack_from("<f", data, start + 4 + 0x29)
    k = scale / 32767.0
    for offset in range(start + 0x100, end - 8):
        (n,) = struct.unpack_from("<I", data, offset)
        if n < 3 or offset + 12 + 28 * n > end:
            continue
        if struct.unpack_from("<I", data, offset + 4 + 8 * n)[0] != n:
            continue
        raw = data[offset + 4 : offset + 4 + 8 * n]
        return [(x * k, y * k, z * k) for x, y, z, _ in struct.iter_unpack("<4h", raw)]
    raise _NoPositionBuffer(f"render model {name}: no position buffer in the block at {start:#x}")
