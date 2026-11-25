# Vehicle door handles

`extract_door_handles.py` places every vehicle door's handle from the game's own data and writes the table the payload embeds, `door_handles.json`. For the front doors that the player's entry animations open, the handle is where the animation's hand is at the moment the door opens, projected onto the door's outer surface. For every other door (rear doors, hoods, trunks, hatches, and the doors of vehicles whose entries open no door by hand) it is a box guess that takes the door's hinge from its constraint point. The engine facts the tool rests on are in [`docs/engine/gameplay/vehicle-doors.md`](../../docs/engine/gameplay/vehicle-doors.md), "Entering through a door"; how the payload uses the table is in [`docs/mod/input/vehicle-doors.md`](../../docs/mod/input/vehicle-doors.md).

The tool uses only the Python standard library, and borrows the archive, entity, and property container readers from [`tools/sights`](../sights).

## Prerequisites

The same as for the sight tool: Python 3.10 or newer, the game install (`archives_win64/` and `patch_win64/`), and a checkout of the Gibbed Just Cause 3 tools for its file lists, which name the vehicle entities. The paths come from `JC3_GAME_DIR` and `JC3_GIBBED_TOOLS`, loaded from `.env` by direnv.

## Usage

```sh
# Regenerate the table beside the script.
python3 extract_door_handles.py

# Explicit paths.
python3 extract_door_handles.py --game "$JC3_GAME_DIR" --gibbed-tools "$JC3_GIBBED_TOOLS"

# Check the geometry on made-up doors; needs no game data.
python3 extract_door_handles.py --self-check
```

`--output` defaults to `door_handles.json` beside the script. A run takes about 20 seconds and logs one line per door of each vehicle model to stderr, saying how its handle was placed and any doubt about it. Like `sights.json`, the table is generated but kept in the repository, and the payload embeds it at build time (`payload/src/controllers/physical_hands/doors/table.rs`).

## Output

A JSON array with one object per door part, sorted by vehicle and part, one object per line:

```json
{"vehicle": "modernsaloon", "part": "modernsaloon_door_f_l", "part_hash": 3153224613, "slot": "FrontLeftDoor", "handle_m": [-0.9307, 0.9869, 0.3669], "normal": [-0.9716, 0.2361, -0.0165], "grade": "clip", "entity": "v0009_car_autostraad_modernsaloon_civilian_01", "entry": "entry.driver", "clip": "rico_vehi_car_low_std_to_driver.ban", "hand": "left", "open_s": 0.2333, "palm_m": [-0.897, 0.9869, 0.3669], "notes": ""}
```

- `part_hash` is the `hashlittle` hash of the part's name, as the vehicle's parts resource stores it and as a live part's entry carries it (`Part::m_name`). It is the table's key: every variant of a model shares its parts, so one entry serves them all.
- `slot` is the part's behaviour, which names its door slot.
- `handle_m` is the handle in the door part's own space, metres, and `normal` the outer face's unit normal there. A door part's mesh is authored in the vehicle's space (its parent space offset is the identity), so for every door in this table the part's space is the vehicle's: +X right, +Y up, -Z forward. The tool still takes the point through the part's chain of parent space offsets, so a part with a real offset would come out right.
- `grade` says how the handle was placed:
  - `clip`: from an entry clip, with no doubt.
  - `clip_doubtful`: from an entry clip, with a doubt in `notes`: the hand more than 10 cm outside the door's box, the palm more than 20 cm from the surface, an entry state other than the two standard ones, or the warp not finished when the door opens.
  - `hinge`: from the door's box and constraint point.
- `entry`, `clip`, `hand`, `open_s`, and `palm_m` (the hand's point before projection, in the part's space) are there for review on clip-placed doors; `entity` is the variant the entry came from. The payload reads only `vehicle`, `part`, `part_hash`, `handle_m`, `normal`, and `grade`.

The table as generated covers 224 door parts of 56 vehicle models: 50 `clip`, 18 `clip_doubtful` (all front doors), and 156 `hinge`. Every vehicle model with a door part in its parts resource is in it.

## Pipeline

1. Collect the vehicle entities from the file lists (`editor/entities/jc_vehicles/**.ee`), the debug variants last.
2. For each, read the vehicle parts resource (`<model>_vehicle_parts.vmodc`, ADF) for the door parts, their behaviours, models, and parent space offsets, and the wheels; the deform points (`<model>_vehicle_deform.vmodc`, ADF) for the seats and the doors' constraints; and the interaction graphs from the entity property file. The door meshes come from each part's closest LOD; `vmesh.py` reads their triangles from the `CarPaint14` blocks body panels use.
3. For each graph entry whose state starts `S_ENTER`, find its animation node in the player's animation set (`rico_base.asb`), `ANIM_<state without S_>_<modifier>` and then `ANIM_<state without S_>`. The modifier is the entry's, then its node's, then the graph's. NPC entries look in the soldiers' set (`soldier_gen_base.asb`) the same way.
4. Take the node's first `open_door`, `quick_open_door`, or `open_door_with_identifier` message. A message at the clip's start opens the door before any hand reaches it, and the entry is skipped; this is the case for every NPC entry clip. The door is the slot the message names, or for the other two the entry's identifier, which is the slot of the seat the entry leads to.
5. Sample the clip (`havok.py`: Havok 2014 spline-compressed animation, the `POLAR32`, `THREECOMP40`, `THREECOMP48`, and uncompressed rotation encodings) at the message's time and at its end, through the skeleton (`rico.bsk` for the player), with the clip's root motion.
6. Place the palms (`LeftHandAttach` and `RightHandAttach`) in the vehicle's space by the warp model below, and take the palm nearest a part of the door's slot. A palm more than 30 cm outside the door's box is not on the door, and the door falls through to the box guess.
7. Project the palm onto the door's outer surface along the door's outward axis (its thinnest box axis, pointing away from the vehicle's origin): the outermost crossing of the mesh, or, when the line misses (a palm above a small car's door, in the window), the nearest point of the outer skin, the triangles across the axis within 10 cm of the box's outer face.
8. When several entries reach a door, the standard entries (`S_ENTER_DRIVER_VEHICLE`, `S_ENTER_PASSENGER_VEHICLE`) win over the rest, the player's over the NPCs', and then the nearest palm.
9. Every other door gets a box guess, as the payload's own: on the outer face, a tenth of the door's run in from the edge across from the hinge, halfway along that edge. The hinge's edge is the one nearer the door's constraint point (the deform point of type `Constraint` whose part is the door), or the door kind's edge without one. A side door takes the height of the clip-placed side doors of its vehicle when there are any, since a car's rear handles sit at the front ones' height. The point is then projected as in step 7.

## The warp model

The engine warps an entry clip's root motion so that the entry's alignment bone (`Reference`) ends the clip on the destination node's transform, here a seat's deform point. Each part of the correction (the horizontal position, the height, and the heading) runs over its own segment of the clip, `SEG_WARP_HORIZONTAL`, `SEG_WARP_VERTICAL`, and `SEG_WARP_ORIENTATION`, or `SEG_WARP` for all three. After the horizontal and heading segments, a bone at time `t` is at `bone(t) * inverse(anchor(end)) * destination`. Before the vertical segment starts, the height is the clip's own above the ground the character started on; the tool takes that ground from the wheels (the lowest hub less its wheel's radius) and blends the two heights by how far the vertical segment has run.

On the standard car clips the horizontal and heading segments end exactly at the open message and the vertical one starts well after it, so the handle's position along and across the door is fixed by the seat, and its height is the clip's own above the ground. On the modern saloon the two heights agree within a centimetre; on the old jeep, entered with the medium clip, the clip's own height is 37 cm above where the end-anchored height would put it.

## Known limits

- The handle is the clip's, not the model's. Each clip serves a class of vehicles (`MOD_LOW`, the medium default, `MOD_HIGH`, the helicopters, the cabin boats), so on a car that differs from the one the clip was authored against, the hand opens the door some way from the modelled handle: on the old mini the palm is 21 cm out and well above the door's skin. The doubtful grades flag the worst of these.
- That the clip is warped onto the destination's full transform rests on the entry warp's best-point mode, which the state's warp properties choose. The modes were read, the choice for each entry state was not.
- The early finish time of an animation track, which would end the warp before the clip's end, is not read.
- The ground under the wheels is the wheels' modelled radius below their hubs, not the suspension's rest.
- `THREECOMP24` and `STRAIGHT16` rotations are not decoded; no entry clip the tool reads uses them.
- The vehicles with no clip-placed doors, all on box guesses, are the trucks, buses, tanks, planes, and boats whose entries open no door by hand: `apc`, `armoredtransport`, `articulatedtruck`, `bavariumtank`, `cargotransport`, `fighterbomber`, `fighterjet`, `heavyassault`, `mediumtank`, `minetruck`, `monorailmissilecar`, `monstertruck`, `oldbus`, `oldtruck`, `seascout`, `smallfloatprop`, `smalltank`, `spyspeedboat`, and `vintagefighter`.
- DLC vehicles under `dlc_win64` are not read.
