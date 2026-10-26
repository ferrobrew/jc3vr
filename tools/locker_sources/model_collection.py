"""Read version 12 model collections, preserving native collection instance indices.

The structure follows CModelCollection::Read: a name chunk, aligned matrices, model-name indices,
flags, entity LOD positions, LOD factors, and world bounds. These instances are static render data,
not CGameObject entities or WeaponData models.
"""

import math
import struct


def parse_collection(data):
    """Return the per-instance model name, world matrix, flags, and LOD/bounds metadata."""
    if len(data) < 32:
        raise ValueError("model collection: truncated header")
    magic, version, name_count, count, name_size, *_ = struct.unpack_from("<4s7I", data)
    if magic != b"MDI\0" or version != 12:
        raise ValueError(f"model collection: unsupported header {magic!r}, version {version}")
    end_names = 32 + name_size
    if end_names > len(data):
        raise ValueError("model collection: truncated name chunk")
    names = data[32:end_names].split(b"\0")
    if name_count and (not name_size or data[end_names - 1] != 0):
        raise ValueError("model collection: unterminated name chunk")
    if len(names) <= name_count or any(not name for name in names[:name_count]):
        raise ValueError("model collection: truncated or empty model name")
    try:
        names = [name.decode("ascii") for name in names[:name_count]]
    except UnicodeDecodeError as error:
        raise ValueError("model collection: non-ASCII model name") from error
    matrices = (end_names + 15) & ~15
    indices = matrices + 64 * count
    flags = indices + 2 * count
    lod_positions = flags + 2 * count
    lod_factors = lod_positions + 12 * count
    bounds = lod_factors + 4 * count
    if bounds + 24 * count != len(data):
        raise ValueError(f"model collection: size mismatch: expected {bounds + 24 * count}, got {len(data)}")
    instances = []
    for index in range(count):
        name_index = struct.unpack_from("<H", data, indices + 2 * index)[0]
        if name_index >= name_count:
            raise ValueError(f"model collection: instance {index}: invalid model index {name_index}")
        world = struct.unpack_from("<16f", data, matrices + 64 * index)
        if not all(math.isfinite(value) for value in world):
            raise ValueError(f"model collection: instance {index}: non-finite world matrix")
        instances.append({
            "collection_index": index,
            "model": names[name_index],
            "model_index": name_index,
            "world": world,
            "flags": struct.unpack_from("<H", data, flags + 2 * index)[0],
            "lod_position": struct.unpack_from("<3f", data, lod_positions + 12 * index),
            "lod_factor": struct.unpack_from("<f", data, lod_factors + 4 * index)[0],
            "bounds": struct.unpack_from("<6f", data, bounds + 24 * index),
        })
    return instances
