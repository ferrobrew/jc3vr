"""Load a player weapon entity: its name hash, fire points, and model vertices in weapon space.

A weapon entity archive (``.ee``) embeds its entity property file (``.epe``), its model LOD lists
(``.lod``), and its render models (``.rbm``). The patch archives ship standalone replacement
``.epe`` files for some weapons, and the standalone copy takes precedence over the embedded one.

In the ``.epe``, the weapon is the top-level object that owns ``SWeaponPoint`` children (in launcher
files, effect tables come first). A ``SWeaponPoint`` of type 2 is a fire position, whose ``world``
matrix places the muzzle; the barrel is its -Z axis. The weapon's model and each child component's
model are named by a ``.model_xml`` path, which resolves to the ``.lod`` file of the same stem; its
first entry is the closest LOD's ``.rbm``.
"""

from dataclasses import dataclass, field

import rbm
import rtpc
from archives import GameArchives, hashlittle, read_entity

# The model path, and the index of the gun (``CWeaponData``) a fire point belongs to (-1 for every
# gun). Their property names are not known.
MODEL_KEY = 0xF9DCF6AB
WEAPON_DATA_INDEX_KEY = 0x61ECCE72
FIRE_POSITION = 2
# Components whose parts are not part of the sight geometry: magazines and ammo belts hang below or
# beside the receiver and move when reloading.
SKIPPED_PART_NAMES = ("magazine", "ammo")

Matrix = tuple[float, ...]
Vec3 = tuple[float, float, float]


@dataclass
class FirePoint:
    name: str
    matrix: Matrix
    weapon_data: int

    @property
    def translation(self) -> Vec3:
        return self.matrix[12], self.matrix[13], self.matrix[14]


@dataclass
class Part:
    name: str
    model: str
    positions: list[Vec3]


@dataclass
class Weapon:
    name: str
    class_name: str
    stored_name_hash: int
    epe_source: str
    fire_points: list[FirePoint] = field(default_factory=list)
    parts: list[Part] = field(default_factory=list)


def load(entity_path: str, archives: GameArchives) -> Weapon:
    entity = archives.read(entity_path)
    if entity is None:
        raise LookupError(f"{entity_path}: not in the archives")
    files = read_entity(entity)
    epe_path = entity_path.removesuffix(".ee") + ".epe"
    epe = archives.read(epe_path)
    epe_source = archives.locate(epe_path).label if epe is not None else "embedded"
    if epe is None:
        epe = files.get(epe_path)
    if epe is None:
        raise LookupError(f"{epe_path}: neither standalone nor embedded in the entity")

    root = rtpc.parse(epe)
    top = next((o for o in root.children if any(c.get("_class") == "SWeaponPoint" for c in o.children)), root.children[0])
    weapon = Weapon(
        name=top.get("name"),
        class_name=top.get("_class"),
        stored_name_hash=top.get("name_hash", 0) & 0xFFFFFFFF,
        epe_source=epe_source,
    )
    models = [(weapon.name, top.get(MODEL_KEY), None)]
    for child in top.children:
        name = child.get("name", "")
        if child.get("_class") == "SWeaponPoint":
            if child.get("type") == FIRE_POSITION:
                weapon.fire_points.append(FirePoint(name, child.get("world"), child.get(WEAPON_DATA_INDEX_KEY, -1)))
        elif child.get(MODEL_KEY) and not any(s in name for s in SKIPPED_PART_NAMES):
            models.append((name, child.get(MODEL_KEY), child.get("world")))

    seen = set()
    for part, model, matrix in models:
        if not model or (model, matrix) in seen:
            continue
        seen.add((model, matrix))
        lod_path = model.removesuffix(".model_xml") + ".lod"
        lod = _resolve(lod_path, files, archives)
        closest = lod.decode().split()[0]
        points = rbm.positions(_resolve(closest, files, archives), closest)
        if matrix is not None:
            points = [_transform(matrix, p) for p in points]
        weapon.parts.append(Part(part, closest, points))
    return weapon


def name_hash(entity_path: str) -> int:
    """The weapon's name hash (``WeaponBase::m_WeaponNameHash``): ``hashlittle`` of the entity name."""
    return hashlittle(entity_path.rsplit("/", 1)[-1].removesuffix(".ee"))


def _resolve(path: str, files: dict[str, bytes], archives: GameArchives) -> bytes:
    data = files.get(path)
    if data is None:
        data = archives.read(path)
    if data is None:
        raise LookupError(f"{path}: neither embedded in the entity nor in the archives")
    return data


def _transform(m: Matrix, p: Vec3) -> Vec3:
    # Row-major, row vectors, translation in the last row.
    x, y, z = p
    return (
        x * m[0] + y * m[4] + z * m[8] + m[12],
        x * m[1] + y * m[5] + z * m[9] + m[13],
        x * m[2] + y * m[6] + z * m[10] + m[14],
    )
