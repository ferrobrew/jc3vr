"""Read the triangles of a vehicle part's render model (``.rbm``).

The header is as ``tools/sights/rbm.py`` describes: ``RBMDL`` 1.16, the bounding box at ``0x15``,
and the block count at ``0x2D``, the blocks following from ``0x35``, each ending in the
``0x89ABCDEF`` footer. Vehicle body panels are ``CarPaint14`` blocks (type ``0x483304D6``), whose
vertices are 24-byte records starting with a ``f32`` position, followed by a second stream with the
same count; other blocks are ``GeneralJC3`` and the like. Rather than parse each block type's
header and material, the reader finds a block's position stream as a ``u32 n`` followed by ``n``
records whose leading ``f32`` triples all lie inside the model's bounding box and that are
followed by another ``u32 n``, and its index buffer as the ``u32 m`` and ``m`` ``u16`` indices that
end at the block's footer, as the weapon reader does. A block whose streams it cannot find is
skipped.
"""

import struct

FOOTER = struct.pack("<I", 0x89ABCDEF)
STRIDES = (24, 12, 16, 20, 28, 32, 36, 40, 44, 48)
TOLERANCE = 2e-3
HEADER = 0x35


def bounding_box(data: bytes, name: str):
    if struct.unpack_from("<I5s", data, 0) != (5, b"RBMDL"):
        raise ValueError(f"render model {name}: not an RBMDL file")
    box = struct.unpack_from("<6f", data, 0x15)
    return box[:3], box[3:]


def triangles(data: bytes, name: str) -> list[tuple[tuple, tuple, tuple]]:
    """Every triangle of the model that could be read, as three positions in the model's space."""
    low, high = bounding_box(data, name)
    (count,) = struct.unpack_from("<I", data, 0x2D)
    out = []
    start = HEADER
    for _ in range(count):
        try:
            end = data.index(FOOTER, start)
        except ValueError:
            break
        # The footer value can occur inside vertex data; take the first end at which the block
        # yields both streams.
        while True:
            found = _block(data, start, end, low, high)
            if found is not None:
                break
            nxt = data.find(FOOTER, end + 1)
            if nxt < 0:
                break
            end = nxt
        if found is not None:
            positions, indices = found
            out += [(positions[a], positions[b], positions[c]) for a, b, c in zip(indices[0::3], indices[1::3], indices[2::3])]
        start = end + 4
    return out


def _block(data: bytes, start: int, end: int, low, high):
    def inside(p) -> bool:
        return all(low[i] - TOLERANCE <= p[i] <= high[i] + TOLERANCE for i in range(3))

    for offset in range(start + 5, end - 8):
        (n,) = struct.unpack_from("<I", data, offset)
        if n < 3 or n > 0xFFFF:
            continue
        for stride in STRIDES:
            stream_end = offset + 4 + stride * n
            if stream_end + 4 > end or struct.unpack_from("<I", data, stream_end)[0] != n:
                continue
            positions = [struct.unpack_from("<3f", data, offset + 4 + stride * k) for k in range(n)]
            if not all(inside(p) for p in positions):
                continue
            indices = _indices(data, stream_end, end, n)
            if indices:
                return positions, indices
    return None


def _indices(data: bytes, start: int, end: int, n: int):
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
            return indices
    return None
