"""Resolve the model a round in the hand is drawn with for a weapon the game has no round model for.

The shotguns and the revolvers load rounds one at a time or a cylinder at a time, and the game has
no model of a loose round for them: no spare magazine prop and no round part. Their entities embed
the effect that throws out a spent casing as the gun fires (``shell_eject_12ga.effc`` for the
shotguns, ``bullet_eject_revolver.effc`` for the revolvers), and that effect names the casing's
model by the ``hashlittle`` of its ``.lod`` path, which is the key the game's model cache holds the
model under. The effect is the entity's one embedded ``.effc`` whose name holds ``eject``; the model
is the one ``.lod`` embedded in the entity whose hash the effect holds, and its first entry is the
closest LOD's ``.rbm``.

A casing model is a cylinder along its own Z axis, centred on its origin. Which end is the head (the
base, which goes in last) is read from the mesh by eye and named in ``HEADS``: the shotgun shell's
brass head is the stepped band at its -Z end, and the pistol casing's head is the rim and extractor
groove at its +Z end. A round goes into the gun head last, so it travels toward the other end.
"""

import math
import struct
from dataclasses import dataclass

import rbm
from archives import GameArchives, hashlittle, read_entity
from weapon import Vec3

# The head of each casing model the shell-fed guns and revolvers eject, as the direction along the
# model's Z axis it lies in.
HEADS = {
    "models/jc_vfx/bullet_casing/shell_casing_shell_casing.lod": -1.0,
    "models/jc_vfx/bullet_casing/pistol_casing_bullet_casing.lod": 1.0,
}
# A revolver's cylinder holds six rounds, loaded together as a speed loader holds them. The models
# do not resolve the chambers well enough to count them, so this is a class default.
REVOLVER_CHAMBERS = 6

@dataclass
class RoundModel:
    # The ``.lod`` path, which the game's model cache keys by its ``hashlittle``.
    model: str
    # The feed end (the end away from the head) on the model's axis, in the model's own frame.
    top: Vec3
    # The direction the round travels into the gun, in the model's own frame, unit length.
    axis: Vec3
    # The casing's radius across its axis.
    radius: float
    # How many rounds one push loads, held side by side in a ring around the group's axis.
    count: int
    # The distance from the group's axis to each round's axis: zero for one round, and for several,
    # the ring on which neighbouring casings touch.
    ring: float


def resolve(entity_path: str, archives: GameArchives, count: int) -> RoundModel:
    """The casing model of the weapon at ``entity_path``, ``count`` of which one push loads.
    Raises ``LookupError`` when the entity has no casing ejection effect or its model is unknown."""
    entity = archives.read(entity_path)
    if entity is None:
        raise LookupError(f"round model: {entity_path} is not in the archives")
    files = read_entity(entity)
    effects = [name for name in files if name.endswith(".effc") and "eject" in name.rsplit("/", 1)[-1]]
    if len(effects) != 1:
        raise LookupError(f"round model: the entity embeds {len(effects)} ejection effects, not one")
    effect = files[effects[0]]
    models = [name for name in files if name.endswith(".lod") and struct.pack("<I", hashlittle(name)) in effect]
    if len(models) != 1:
        raise LookupError(f"round model: {effects[0]} names {len(models)} of the entity's models, not one")
    model = models[0]
    head = HEADS.get(model)
    if head is None:
        raise LookupError(f"round model: {model} is not a casing whose head is known")
    closest = files[model].decode().split()[0]
    data = files.get(closest) or archives.read(closest)
    if data is None:
        raise LookupError(f"round model: {closest} is neither embedded in the entity nor in the archives")
    positions = rbm.positions(data, closest)
    axis = (0.0, 0.0, -head)
    top = max(p[2] * axis[2] for p in positions) * axis[2]
    radius = max(max(abs(p[0]), abs(p[1])) for p in positions)
    return RoundModel(model, (0.0, 0.0, top), axis, radius, count, ring_radius(radius, count))


def ring_radius(radius: float, count: int) -> float:
    """The ring on which ``count`` casings of ``radius`` stand side by side, each touching its
    neighbours: zero for one."""
    return 0.0 if count <= 1 else radius / math.sin(math.pi / count)
