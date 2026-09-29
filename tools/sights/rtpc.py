"""Read RTPC property containers, such as the entity property files (``.epe``).

A container is ``RTPC``, ``u32`` version 1, and a root node. A node is ``u32`` name hash, ``u32``
data offset, ``u16`` property count, and ``u16`` child count. At the data offset sit the properties
(``u32`` name hash, 4 bytes of inline value or offset, ``u8`` type), then, aligned to 4 bytes, the
child nodes. Property and node name hashes are ``hashlittle`` of the name. Offsets are from the start
of the container.
"""

import struct
from dataclasses import dataclass, field

from archives import hashlittle

# The variant types this reader decodes. Other types read as ``None``.
INTEGER, FLOAT, STRING, VEC2, VEC3, VEC4, MAT4 = 1, 2, 3, 4, 5, 6, 8
INTEGERS, FLOATS = 9, 10


@dataclass
class Node:
    name_hash: int
    properties: dict[int, object] = field(default_factory=dict)
    children: list["Node"] = field(default_factory=list)

    def get(self, key: str | int, default=None):
        """A property by name, or by its hash for properties whose name is not known."""
        return self.properties.get(hashlittle(key) if isinstance(key, str) else key, default)


def parse(data: bytes) -> Node:
    magic, version = struct.unpack_from("<4sI", data, 0)
    if magic != b"RTPC" or version != 1:
        raise ValueError(f"property container: not a version 1 RTPC ({magic!r}, {version})")
    return _node(data, 8)


def _node(data: bytes, offset: int) -> Node:
    name_hash, data_offset, property_count, child_count = struct.unpack_from("<IIHH", data, offset)
    node = Node(name_hash)
    for i in range(property_count):
        key, raw, kind = struct.unpack_from("<I4sB", data, data_offset + 9 * i)
        node.properties[key] = _value(data, raw, kind)
    children = (data_offset + 9 * property_count + 3) & ~3
    node.children = [_node(data, children + 12 * i) for i in range(child_count)]
    return node


def _value(data: bytes, raw: bytes, kind: int):
    if kind == INTEGER:
        return struct.unpack("<i", raw)[0]
    if kind == FLOAT:
        return struct.unpack("<f", raw)[0]
    (offset,) = struct.unpack("<I", raw)
    if kind == STRING:
        return data[offset : data.index(b"\0", offset)].decode("ascii")
    if kind in (VEC2, VEC3, VEC4, MAT4):
        count = {VEC2: 2, VEC3: 3, VEC4: 4, MAT4: 16}[kind]
        return struct.unpack_from(f"<{count}f", data, offset)
    if kind in (INTEGERS, FLOATS):
        (count,) = struct.unpack_from("<I", data, offset)
        return struct.unpack_from(f"<{count}{'i' if kind == INTEGERS else 'f'}", data, offset + 4)
    return None
