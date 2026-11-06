"""Read vertex positions from RBMDL render models (``.rbm``) made of ``GeneralMkIII`` blocks, skipping
any ``Window`` blocks among them.

The header is ``u32 5``, ``RBMDL``, ``u32`` major, minor, and revision (1, 16, and a revision),
``f32`` bounding box min and max, ``u32`` block count, and one more ``u32``. The blocks follow, each
ending in the ``0x89ABCDEF`` footer. A block starts with its ``u32`` type and a ``u8`` version; for
``GeneralMkIII`` (type ``0x2CEC5AD5``, ``hashlittle("GeneralMkIII")``, version 5) the position dequantisation scale is the ``f32`` at
the version byte plus ``0x29``. After the material (``u32``-length-prefixed texture paths) come the
vertex buffers: ``u32 n`` and ``n`` ``i16x4`` positions (a component divided by 32767 and multiplied
by the scale), then ``u32 n`` and ``n`` 20-byte records of packed UVs, normal, and tangent. Indices
and optional extra data follow, and their layout varies by model.

The reader locates the position buffer by its ``n``, ``8n`` bytes, ``n`` chain, and validates the
result against the header's bounding box. ``mesh`` also reads the triangles: the index buffer is a
``u32 m`` and ``m`` ``u16`` vertex indices, three per triangle, and the reader takes the one that
ends at the block's footer and whose indices all fall inside the block's vertices. A block with
extra data after its indices has no such buffer and contributes no triangles.

A ``Window`` block (type ``0x5B2003F6``, ``hashlittle("Window")``) is a glass surface, such as the
vial on the Bavarium Splitter. Its layout is not decoded: the reader skips it, and the remaining
positions then need only lie inside the header's bounding box rather than span it.
"""

import struct

BLOCK_GENERAL_JC3 = 0x2CEC5AD5
BLOCK_WINDOW = 0x5B2003F6
FOOTER = struct.pack("<I", 0x89ABCDEF)
# The largest gap between the positions' extent and the header's bounding box, in metres.
BBOX_TOLERANCE = 2e-3


class UnsupportedBlock(ValueError):
    pass


def positions(data: bytes, name: str) -> list[tuple[float, float, float]]:
    return mesh(data, name)[0]


def mesh(data: bytes, name: str) -> tuple[list[tuple[float, float, float]], list[tuple[int, int, int]]]:
    """The model's vertex positions and its triangles, as index triples into the positions."""
    if struct.unpack_from("<I5s", data, 0) != (5, b"RBMDL"):
        raise ValueError(f"render model {name}: not an RBMDL file")
    major, minor, _revision = struct.unpack_from("<3I", data, 9)
    if (major, minor) != (1, 16):
        raise ValueError(f"render model {name}: unsupported version {major}.{minor}")
    bbox = struct.unpack_from("<6f", data, 0x15)
    (count,) = struct.unpack_from("<I", data, 0x2D)
    start = 0x35
    out = []
    triangles = []
    skipped = False
    for remaining in range(count - 1, -1, -1):
        if struct.unpack_from("<I", data, start)[0] == BLOCK_WINDOW:
            start = _skip(data, start, remaining, name)
            skipped = True
            continue
        end = data.index(FOOTER, start)
        # The footer value can occur inside vertex data, so take the first end at which the block
        # parses.
        while True:
            try:
                block, buffer_end = _block(data, start, end, name)
                break
            except _NoPositionBuffer:
                end = data.index(FOOTER, end + 1)
        triangles += [(a + len(out), b + len(out), c + len(out)) for a, b, c in _triangles(data, buffer_end, end, len(block))]
        out += block
        start = end + 4
    if not out:
        raise ValueError(f"render model {name}: no block with positions")
    for axis in range(3):
        lo = min(p[axis] for p in out)
        hi = max(p[axis] for p in out)
        below, above = bbox[axis] - lo, hi - bbox[axis + 3]
        if skipped:
            # The skipped blocks may hold the extremes, so the rest need only lie inside the box.
            off = below > BBOX_TOLERANCE or above > BBOX_TOLERANCE
        else:
            off = abs(below) > BBOX_TOLERANCE or abs(above) > BBOX_TOLERANCE
        if off:
            raise ValueError(f"render model {name}: positions span {lo:.4f}..{hi:.4f} on axis {axis}, outside the header's bounding box")
    return out, triangles


def _skip(data: bytes, start: int, remaining: int, name: str) -> int:
    """The start of the block after the undecoded one at ``start``, with ``remaining`` blocks after
    it: the first footer that ends the file for the last block, or is followed by a known block
    type otherwise, since the footer value can occur inside the block's data."""
    end = data.index(FOOTER, start)
    while True:
        after = end + 4
        if remaining == 0 and after == len(data):
            return after
        if remaining > 0 and after + 4 <= len(data) and struct.unpack_from("<I", data, after)[0] in (BLOCK_GENERAL_JC3, BLOCK_WINDOW):
            return after
        end = data.find(FOOTER, end + 1)
        if end < 0:
            raise ValueError(f"render model {name}: no end to the block at {start:#x}")


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
        return [(x * k, y * k, z * k) for x, y, z, _ in struct.iter_unpack("<4h", raw)], offset + 4 + 8 * n
    raise _NoPositionBuffer(f"render model {name}: no position buffer in the block at {start:#x}")


def _triangles(data: bytes, start: int, end: int, n: int) -> list[tuple[int, int, int]]:
    # The index buffer ends at the footer, possibly padded to 4 bytes, so its count sits at
    # ``end - 4 - 2m`` (less up to 2 bytes of padding) for some m.
    for padding in (0, 2):
        for offset in range(end - 4 - padding - 6, start - 1, -2):
            (m,) = struct.unpack_from("<I", data, offset)
            if 2 * m != end - padding - offset - 4:
                continue
            if m % 3:
                break
            indices = struct.unpack_from(f"<{m}H", data, offset + 4)
            if max(indices, default=0) >= n:
                break
            return list(zip(indices[0::3], indices[1::3], indices[2::3]))
    return []
