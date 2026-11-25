"""Read ADF resources, such as a vehicle's ``_vehicle_deform.vmodc`` and ``_vehicle_parts.vmodc``.

An ADF file is `` FDA``, ``u32`` version 4, then ``u32`` count and offset pairs for the instances,
the type definitions, the string hashes, and the name table, and the total size. The name table is
one length byte per name followed by the names, each NUL-terminated. A type definition is 40 bytes
(``u32`` kind, size, alignment, and type hash, ``u64`` name index, ``u16`` flags and scalar type,
``u32`` element type hash, element count, and member count) followed by its members: 32 bytes each
for a structure (``u64`` name index, ``u32`` type hash, alignment, offset with the bit offset in its
top byte, default type, and ``u64`` default value), 12 bytes each for an enumeration. An instance is
``u32`` name hash, type hash, offset, and size, and a ``u64`` name index.

Inside an instance, an array is a ``u64`` offset and a ``u64`` count, and a string is a ``u64``
offset, both relative to the instance's start. The primitive types are not defined in the file and
are known by their type hashes.
"""

import struct
from dataclasses import dataclass

# The built-in scalar types, by type hash: the struct format of one value.
PRIMITIVES = {
    0x0CA2821D: "B",  # uint8
    0x580D0A62: "b",  # int8
    0x86D152BD: "H",  # uint16
    0xD13FCF93: "h",  # int16
    0x075E4E4F: "I",  # uint32
    0x192FE633: "i",  # int32
    0xA139E01F: "Q",  # uint64
    0xAF41354F: "q",  # int64
    0x7515A207: "f",  # float
    0xC609F663: "d",  # double
}
STRING = 0x8955583E

STRUCTURE, POINTER, ARRAY, INLINE_ARRAY, STRING_KIND, BITFIELD, ENUM, STRING_HASH = 1, 2, 3, 4, 5, 7, 8, 9


@dataclass
class _Type:
    kind: int
    size: int
    name: str
    element: int
    count: int
    members: list


def parse(data: bytes) -> dict[str, object]:
    """Every instance in the file, by name, decoded into dicts, lists, strings, and numbers."""
    magic, version, inst_count, inst_offset, type_count, type_offset, _sh_count, _sh_offset, name_count, name_offset, _total = struct.unpack_from("<4s10I", data, 0)
    if magic != b" FDA" or version != 4:
        raise ValueError(f"ADF: not a version 4 ADF file ({magic!r}, {version})")
    names = []
    p = name_offset + name_count
    for length in data[name_offset : name_offset + name_count]:
        names.append(data[p : p + length].decode())
        p += length + 1
    types: dict[int, _Type] = {}
    p = type_offset
    for _ in range(type_count):
        kind, size, _align, type_hash, name_index, _flags, _scalar, element, count, member_count = struct.unpack_from("<IIIIQHHIII", data, p)
        p += 40
        members = []
        if kind == STRUCTURE:
            for _ in range(member_count):
                m_name, m_type, _m_align, m_offset, _dtype, _dvalue = struct.unpack_from("<QIIIIQ", data, p)
                members.append((names[m_name], m_type, m_offset & 0xFFFFFF))
                p += 32
        elif kind == ENUM:
            p += 12 * member_count
        types[type_hash] = _Type(kind, size, names[name_index], element, count, members)
    out = {}
    for i in range(inst_count):
        _name_hash, type_hash, offset, _size, name_index = struct.unpack_from("<IIIIQ", data, inst_offset + 24 * i)
        out[names[name_index]] = _Reader(data, types, offset).value(type_hash, offset)
    return out


class _Reader:
    def __init__(self, data: bytes, types: dict, base: int):
        self.data, self.types, self.base = data, types, base

    def value(self, type_hash: int, at: int):
        if type_hash in PRIMITIVES:
            return struct.unpack_from("<" + PRIMITIVES[type_hash], self.data, at)[0]
        if type_hash == STRING:
            (offset,) = struct.unpack_from("<Q", self.data, at)
            return self._cstr(offset)
        t = self.types.get(type_hash)
        if t is None:
            raise ValueError(f"ADF: unknown type {type_hash:#010x}")
        if t.kind == STRUCTURE:
            return {name: self.value(m_type, at + offset) for name, m_type, offset in t.members}
        if t.kind == ARRAY:
            offset, count = struct.unpack_from("<QQ", self.data, at)
            stride = self._size(t.element)
            return [self.value(t.element, self.base + offset + stride * k) for k in range(count)]
        if t.kind == INLINE_ARRAY:
            stride = self._size(t.element)
            return [self.value(t.element, at + stride * k) for k in range(t.count)]
        if t.kind == STRING_KIND:
            (offset,) = struct.unpack_from("<Q", self.data, at)
            return self._cstr(offset)
        if t.kind in (ENUM, STRING_HASH):
            return struct.unpack_from("<I", self.data, at)[0]
        return None

    def _size(self, type_hash: int) -> int:
        if type_hash in PRIMITIVES:
            return struct.calcsize(PRIMITIVES[type_hash])
        if type_hash == STRING:
            return 8
        return self.types[type_hash].size

    def _cstr(self, offset: int) -> str | None:
        if offset == 0:
            return None
        start = self.base + offset
        return self.data[start : self.data.index(b"\0", start)].decode("utf-8", "replace")
