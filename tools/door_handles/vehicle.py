"""Load what the handles need from a vehicle entity: its parts, its doors and their meshes, its
deform points, and its interaction graphs.

A vehicle entity (``.ee``) embeds ``<model>_vehicle_parts.vmodc`` (ADF ``VehicleParts``: every
part's name, behaviour, model, parent, and ``parent_space_offset``, and the ``door_parts`` and
``wheel_parts`` lists) and ``<model>_vehicle_deform.vmodc`` (ADF ``DeformPoints``: named
transforms in the vehicle's space, among them the seats the interaction graph's deformable point
nodes name, and each loose part's constraint, ``..._CON``). A part's mesh is authored in its own
space; the chain of ``parent_space_offset`` from the root part takes it into the vehicle's space,
as the part's root-space transform does at run time before any hinge, swing, or deformation moves
it. The entity property file (``.epe``) holds the interaction graphs; a standalone copy in the
archives replaces the embedded one, as for the weapons.
"""

import logging
from dataclasses import dataclass, field

import adf
import graph
import vmesh
from archives import GameArchives, read_entity
from xform import Vec3, Xform

log = logging.getLogger("door_handles")

PARTS_SUFFIX = "_vehicle_parts.vmodc"
DEFORM_SUFFIX = "_vehicle_deform.vmodc"


@dataclass
class Mesh:
    """A part's mesh in its own space: the model box, and the triangles when they could be read."""

    box_min: Vec3
    box_max: Vec3
    triangles: list[tuple[Vec3, Vec3, Vec3]] = field(default_factory=list)


@dataclass
class Door:
    part: str
    part_hash: int
    part_index: int
    behaviour: str
    to_vehicle: Xform
    mesh: Mesh | None
    hinge: Vec3 | None
    """The door's constraint point, in the door's own space: where it hangs from its parent."""


@dataclass
class Vehicle:
    entity: str
    model: str
    doors: list[Door]
    deform_points: dict[str, Xform]
    graphs: list
    ground_y: float | None
    """The height of the ground under the wheels, in the vehicle's space: the lowest wheel's hub
    less its radius. ``None`` without wheels."""


def load(entity: str, archives: GameArchives) -> Vehicle:
    data = archives.read(entity)
    if data is None:
        raise LookupError(f"{entity}: not in the archives")
    files = read_entity(data)
    epe_path = entity.removesuffix(".ee") + ".epe"
    epe = archives.read(epe_path) or files.get(epe_path)
    if epe is None:
        raise LookupError("no entity property file")
    parts_path = next((k for k in files if k.endswith(PARTS_SUFFIX)), None)
    if parts_path is None:
        raise LookupError("no vehicle parts resource")
    model = parts_path.rsplit("/", 1)[-1].removesuffix(PARTS_SUFFIX)
    parts = adf.parse(files[parts_path])["VehicleParts"]
    deform_path = next((k for k in files if k.endswith(DEFORM_SUFFIX)), None)
    deform = adf.parse(files[deform_path])["DeformPoints"]["points"] if deform_path else []

    transforms = _part_transforms(parts["parts"])
    deform_points = {p["name"]: Xform.from_matrix(p["start_transform"]) for p in deform}
    constraints = {p["from_part"]: Xform.from_matrix(p["start_transform"]).origin for p in deform if (p.get("deform_point_type") or {}).get("string") == "Constraint"}
    doors = []
    for entry in parts["door_parts"]:
        index = entry["part_index"]
        part = parts["parts"][index]
        name = part["name"]["string"]
        to_vehicle = transforms[index]
        hinge = constraints.get(name)
        doors.append(
            Door(
                part=name,
                part_hash=part["name"]["hash"],
                part_index=index,
                behaviour=part["behavior"]["string"] or "",
                to_vehicle=to_vehicle,
                mesh=_mesh(part["model"]["string"], files, archives),
                hinge=to_vehicle.inverse().apply(hinge) if hinge is not None else None,
            )
        )
    return Vehicle(
        entity=entity,
        model=model,
        doors=doors,
        deform_points=deform_points,
        graphs=graph.graphs(epe),
        ground_y=_ground(parts, transforms, files, archives),
    )


def _part_transforms(parts: list[dict]) -> list[Xform]:
    """Each part's own space to the vehicle's space, through its parents' offsets."""
    out: list[Xform | None] = [None] * len(parts)

    def resolve(i: int, depth: int = 0) -> Xform:
        if out[i] is None:
            local = Xform.from_matrix(parts[i]["parent_space_offset"])
            parent = parts[i]["parent_part_index"]
            if parent < 0 or parent >= len(parts) or depth > len(parts):
                out[i] = local
            else:
                out[i] = local.then(resolve(parent, depth + 1))
        return out[i]

    return [resolve(i) for i in range(len(parts))]


def _mesh(model_path: str, files: dict[str, bytes], archives: GameArchives, with_triangles: bool = True) -> Mesh | None:
    if not model_path:
        return None
    lod = files.get(model_path) or archives.read(model_path)
    if lod is None:
        return None
    rbm_path = lod.decode(errors="replace").split()[0]
    data = files.get(rbm_path) or archives.read(rbm_path)
    if data is None:
        return None
    low, high = vmesh.bounding_box(data, rbm_path)
    return Mesh(low, high, vmesh.triangles(data, rbm_path) if with_triangles else [])


def _ground(parts: dict, transforms: list[Xform], files: dict[str, bytes], archives: GameArchives) -> float | None:
    lowest = None
    for entry in parts.get("wheel_parts", []):
        index = entry.get("part_index", -1)
        if not 0 <= index < len(transforms):
            continue
        mesh = _mesh(parts["parts"][index]["model"]["string"], files, archives, with_triangles=False)
        if mesh is None:
            continue
        # A wheel is modelled about its hub, so its box's top is its radius.
        bottom = transforms[index].origin[1] - mesh.box_max[1]
        lowest = bottom if lowest is None else min(lowest, bottom)
    return lowest
