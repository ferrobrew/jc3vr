"""Read files out of Just Cause 3's archives.

The game stores its data in ``gameN.tab``/``gameN.arc`` pairs. A ``.tab`` is a ``TAB\\0`` header
(``u16 2``, ``u16 1``, ``u32`` alignment) followed by 12-byte entries of ``u32`` name hash, ``u32``
offset, and ``u32`` size into the ``.arc``, which stores each file uncompressed. The name hash is
Jenkins lookup3 ``hashlittle`` of the file's path. The archive trees are ``archives_win64`` (the base
game) and ``patch_win64`` (replacements that the game loads in preference to the base copies).

An entity archive (``.ee``) is an AAF container of zlib chunks around a SARC. The SARC lists named
entries; an entry at offset 0 is a reference to a file stored elsewhere, not embedded data.
"""

import logging
import struct
import zlib
from dataclasses import dataclass
from pathlib import Path

log = logging.getLogger(__name__)

# The archive trees, in ascending precedence: a later tree's copy of a file replaces an earlier one.
ARCHIVE_TREES = ("archives_win64", "patch_win64")


def hashlittle(data: bytes | str, initval: int = 0) -> int:
    """Bob Jenkins' lookup3 ``hashlittle``, which the game uses for file and object names."""
    if isinstance(data, str):
        data = data.encode()
    mask = 0xFFFFFFFF
    length = len(data)
    a = b = c = (0xDEADBEEF + length + initval) & mask
    i = 0
    while length > 12:
        a = (a + int.from_bytes(data[i : i + 4], "little")) & mask
        b = (b + int.from_bytes(data[i + 4 : i + 8], "little")) & mask
        c = (c + int.from_bytes(data[i + 8 : i + 12], "little")) & mask
        a = (a - c) & mask
        a ^= _rot(c, 4)
        c = (c + b) & mask
        b = (b - a) & mask
        b ^= _rot(a, 6)
        a = (a + c) & mask
        c = (c - b) & mask
        c ^= _rot(b, 8)
        b = (b + a) & mask
        a = (a - c) & mask
        a ^= _rot(c, 16)
        c = (c + b) & mask
        b = (b - a) & mask
        b ^= _rot(a, 19)
        a = (a + c) & mask
        c = (c - b) & mask
        c ^= _rot(b, 4)
        b = (b + a) & mask
        length -= 12
        i += 12
    if length == 0:
        return c
    tail = data[i:] + b"\0" * 12
    a = (a + int.from_bytes(tail[0:4], "little")) & mask
    b = (b + int.from_bytes(tail[4:8], "little")) & mask
    c = (c + int.from_bytes(tail[8:12], "little")) & mask
    c ^= b
    c = (c - _rot(b, 14)) & mask
    a ^= c
    a = (a - _rot(c, 11)) & mask
    b ^= a
    b = (b - _rot(a, 25)) & mask
    c ^= b
    c = (c - _rot(b, 16)) & mask
    a ^= c
    a = (a - _rot(c, 4)) & mask
    b ^= a
    b = (b - _rot(a, 14)) & mask
    c ^= b
    c = (c - _rot(b, 24)) & mask
    return c


@dataclass(frozen=True)
class Location:
    """Where one copy of a file sits: its ``.arc``, and its offset and size in it."""

    arc: Path
    offset: int
    size: int

    @property
    def label(self) -> str:
        return f"{self.arc.parent.name}/{self.arc.stem}"


class GameArchives:
    """Every file in the base and patch archives, indexed by name hash."""

    def __init__(self, game_dir: Path):
        self._files: dict[int, list[Location]] = {}
        tabs = 0
        for tree in ARCHIVE_TREES:
            for tab in sorted((game_dir / tree).glob("game*.tab"), key=_archive_number):
                for name_hash, offset, size in _read_tab(tab):
                    self._files.setdefault(name_hash, []).append(Location(tab.with_suffix(".arc"), offset, size))
                tabs += 1
        if not tabs:
            raise FileNotFoundError(f"archives: no game*.tab under {game_dir}/{{{','.join(ARCHIVE_TREES)}}}")
        log.info("indexed %d archives, %d file hashes", tabs, len(self._files))

    def locate(self, name: str) -> Location | None:
        """The copy of ``name`` that the game loads: the patch copy when there is one."""
        copies = self._files.get(hashlittle(name))
        if not copies:
            return None
        patch = [c for c in copies if c.arc.parent.name == "patch_win64"]
        if len(patch) > 1:
            # The load order between patch archives has not been established.
            log.warning("%s: %d patch copies (%s); using the last", name, len(patch), ", ".join(c.label for c in patch))
        return copies[-1]

    def read(self, name: str) -> bytes | None:
        location = self.locate(name)
        if location is None:
            return None
        with open(location.arc, "rb") as arc:
            arc.seek(location.offset)
            data = arc.read(location.size)
        if len(data) != location.size:
            raise ValueError(f"archives: {location.label}: {name} is truncated")
        return data


def read_entity(data: bytes) -> dict[str, bytes]:
    """The files embedded in an entity archive (``.ee``), by name. References are left out."""
    if data[:4] == b"AAF\0":
        data = _aaf(data)
    return {name: data[offset : offset + size] for name, offset, size in _sarc(data) if offset != 0}


def _rot(x: int, k: int) -> int:
    return ((x << k) | (x >> (32 - k))) & 0xFFFFFFFF


def _archive_number(tab: Path) -> int:
    return int(tab.stem.removeprefix("game"))


def _read_tab(path: Path):
    data = path.read_bytes()
    magic, version, unknown, alignment = struct.unpack_from("<4sHHI", data, 0)
    if magic != b"TAB\0" or (version, unknown) != (2, 1):
        raise ValueError(f"archives: {path} is not a version 2 TAB file")
    yield from struct.iter_unpack("<III", data[12 : 12 + (len(data) - 12) // 12 * 12])


def _aaf(data: bytes) -> bytes:
    # Header: 'AAF\0', version, a 28-byte magic string, then u32 total size, block size, and chunk
    # count at 0x24. Each chunk is u32 compressed size, uncompressed size, offset to the next chunk,
    # and 'EWAM', followed by raw deflate data (or stored data when the sizes match).
    total, _block_size, count = struct.unpack_from("<III", data, 0x24)
    offset = 0x30
    out = bytearray()
    for _ in range(count):
        compressed, uncompressed, next_offset, magic = struct.unpack_from("<III4s", data, offset)
        if magic != b"EWAM":
            raise ValueError(f"entity archive: bad chunk magic {magic!r} at {offset:#x}")
        body = data[offset + 16 : offset + 16 + compressed]
        out += body if compressed == uncompressed else zlib.decompress(body, -15)
        offset += next_offset
    if len(out) != total:
        raise ValueError(f"entity archive: unpacked {len(out)} bytes, header says {total}")
    return bytes(out)


def _sarc(data: bytes):
    # Header: u32 4, 'SARC', u32 version 2, u32 index size; then entries of u32 name length, the
    # null-padded name, u32 offset, and u32 size.
    _, magic, version, index_size = struct.unpack_from("<I4sII", data, 0)
    if magic != b"SARC" or version != 2:
        raise ValueError(f"entity archive: not a version 2 SARC ({magic!r}, {version})")
    offset, end = 16, 16 + index_size
    while offset < end:
        (name_length,) = struct.unpack_from("<I", data, offset)
        if name_length == 0:
            break
        name = data[offset + 4 : offset + 4 + name_length].rstrip(b"\0").decode()
        entry_offset, size = struct.unpack_from("<II", data, offset + 4 + name_length)
        yield name, entry_offset, size
        offset += 12 + name_length
