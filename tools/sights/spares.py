"""Resolve a weapon's spare magazine prop to its model.

A gun's spare magazine is a rigid object the game spawns by the weapon's ``magazine_tag``. The tag
names a definition in the prop spawn table (``settings/spawn_prop_defs.bin``): the definition
object whose name hash is ``hashlittle`` of the tag, and whose ``entity`` property is the magazine
entity (``..._mag.ee``). That entity embeds exactly one model LOD list, whose first entry is the
closest LOD's ``.rbm``. Its vertices are in the prop's own frame, which is where the game places
the prop when it drops it.
"""

from dataclasses import dataclass

import rbm
import rtpc
from archives import GameArchives, hashlittle, read_entity
from weapon import Vec3, Weapon

SPAWN_PROP_DEFS = "settings/spawn_prop_defs.bin"


@dataclass
class SpareModel:
    entity: str
    # The closest LOD's ``.rbm`` path, comparable with ``Part.model``.
    model: str
    positions: list[Vec3]


class SpareMagazines:
    """The prop spawn table's entities by definition name hash."""

    def __init__(self, archives: GameArchives):
        self._archives = archives
        data = archives.read(SPAWN_PROP_DEFS)
        if data is None:
            raise LookupError(f"spare magazines: {SPAWN_PROP_DEFS} is not in the archives")
        self._entities = {node.name_hash: node.get("entity") for node in _walk(rtpc.parse(data)) if node.get("entity")}

    def model(self, weapon: Weapon) -> SpareModel | None:
        """The weapon's spare magazine model, or ``None`` for a weapon without a magazine tag.
        Raises ``LookupError`` when the tag does not resolve to a model."""
        if weapon.magazine_tag is None:
            return None
        entity = self._entities.get(hashlittle(weapon.magazine_tag))
        if entity is None:
            raise LookupError(f"spare magazine: the tag {weapon.magazine_tag} names no prop spawn definition")
        entity = entity.replace("\\", "/")
        data = self._archives.read(entity)
        if data is None:
            raise LookupError(f"spare magazine: {entity} is not in the archives")
        files = read_entity(data)
        lods = [name for name in files if name.endswith(".lod")]
        if len(lods) != 1:
            raise LookupError(f"spare magazine: {entity} embeds {len(lods)} model LOD lists, not one")
        closest = files[lods[0]].decode().split()[0]
        model = files.get(closest) or self._archives.read(closest)
        if model is None:
            raise LookupError(f"spare magazine: {closest} is neither embedded in {entity} nor in the archives")
        return SpareModel(entity, closest, rbm.positions(model, closest))


def top_centre(positions: list[Vec3], axis: Vec3) -> Vec3:
    """The bounding box centre moved along ``axis`` to the model's furthest extent along it."""
    centre = tuple((min(p[i] for p in positions) + max(p[i] for p in positions)) / 2 for i in range(3))
    top = max(_dot(p, axis) for p in positions)
    shift = top - _dot(centre, axis)
    return tuple(c + a * shift for c, a in zip(centre, axis))


def _walk(node: rtpc.Node):
    yield node
    for child in node.children:
        yield from _walk(child)


def _dot(a: Vec3, b: Vec3) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
