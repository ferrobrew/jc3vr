# Weapon sight lines

`extract_sights.py` derives each player weapon's sight line from the game's own models and writes it as a table of sight overrides, `sights.json`. Each entry gives the sight line's origin and direction relative to the weapon's fire point, which is the frame the payload's `SightOverride` (`payload/src/controllers/config.rs`) is expressed in. The engine facts behind the tool (where the files are, the weapon object, fire points, and model parts) are in [`docs/engine/gameplay/weapon-data.md`](../../docs/engine/gameplay/weapon-data.md).

The tool reads the archives directly and uses only the Python standard library. Plots are the exception and need matplotlib.

## Prerequisites

- Python 3.10 or newer.
- The game install. The tool reads `archives_win64/` and `patch_win64/` under it.
- A checkout of the Gibbed Just Cause 3 tools. The tool reads only its file lists, `bin/projects/Just Cause 3/files/**/*.filelist`, to find the weapon entity paths; nothing needs building, and mono is not needed.
- matplotlib, only for `--plots`. On NixOS: `nix-shell -p "python3.withPackages (p: [p.matplotlib])"`.

The paths follow the same environment variables as the reverse-engineering skill, loaded from `.env` by direnv: `JC3_GAME_DIR` and `JC3_GIBBED_TOOLS`.

## Usage

```sh
# Regenerate the table beside the script.
python3 extract_sights.py

# Explicit paths, side views for review, and the extracted entity files for inspection.
python3 extract_sights.py \
  --game "$HOME/.local/share/Steam/steamapps/common/Just Cause 3" \
  --gibbed-tools "$HOME/programming/gibbed-justcause3-tools-fork" \
  --plots /tmp/sights/png --dump /tmp/sights/files
```

`--game` defaults to `$JC3_GAME_DIR`, then to the default Steam library path. `--gibbed-tools` defaults to `$JC3_GIBBED_TOOLS`. `--output` defaults to `sights.json` beside the script. `--dump DIR` writes every file embedded in each entity archive, plus the `.epe` in use, under `DIR/<weapon>/`. `--plots DIR` writes a side view and a top view per weapon: centreline vertices in red, the bore in blue, and the derived sight line and its two defining points in green. The run takes a couple of seconds and logs one line per weapon to stderr, including the reason for each doubtful or failed result.

`sights.json` is generated but kept in the repository, like `payload/src/director/spawn_defs.json`: it is small, it is derived from game data that does not change, and regenerating it needs the game install. The payload embeds it at build time (`payload/src/controllers/aim/sights/table.rs`), so a change to it takes effect on the next payload build. The `--dump` and `--plots` outputs are game content and review material, so write them outside the repository.

## Output

A JSON array with one object per weapon entity, sorted by name, one object per line:

```json
{"weapon": "w161_sniper_rifle_u_sv_45", "weapon_hash": 3923877588, "offset_m": [0.0, 0.0869, 0.0], "angles_deg": [0.0, 0.0], "grade": "scoped"}
```

- `weapon_hash` is `hashlittle` of the entity name, which equals the weapon's runtime `WeaponBase::m_WeaponNameHash`. It is the table's key.
- `offset_m` is the sight line's origin relative to the fire point, in the fire point's frame (+Y up, -Z along the barrel), in metres. It is the point on the sight line directly above or below the muzzle, not the eye position; the direction is the same either way, and an origin at the muzzle keeps a ray cast clear of the gun's own model.
- `angles_deg` is the sight line's pitch (positive up) and yaw (positive left) relative to the barrel, in degrees. The yaw is always 0 because every weapon model is laterally symmetric.
- `grade` is the confidence in the entry:
  - `scoped`: a scope tube was found and the line runs down its axis. These entries are reliable.
  - `pistol`: iron sights on a one-handed weapon (pistols, revolvers, SMGs, and the sawn-off shotgun) whose line passes the plausibility checks. The sights sit within about 1 cm of the fire point, so an error here is small.
  - `iron_uncertain`: iron sights on a two-handed weapon, or a one-handed result that fails the checks. These need review against the plots before use.
  - `failed`: no line was derived. The entry carries a zero offset and zero angles, which is the same as no correction.
  - `tuned`: tuned in the headset and pasted in (see "Tuned entries"). The tool never derives this grade, and keeps these entries when it regenerates the table.

The fields `weapon_hash`, `offset_m`, and `angles_deg` are the serde form of `SightOverride`, so each entry deserializes into one; `weapon` and `grade` are extra fields that serde ignores by default.

## In the payload

The payload looks a held weapon's sight correction up by `weapon_hash`: the per-weapon entry in the mod's config first, then this table, then none, which leaves the sight line on the fire point. It uses only the `scoped`, `pistol`, and `tuned` entries; an `iron_uncertain` or `failed` entry is not trusted and leaves the weapon on its fire point. See [`docs/mod/input/controllers.md`, "Sight lines"](../../docs/mod/input/controllers.md#sight-lines).

## Tuned entries

The payload's sight tuning tool, in the Controllers tab's aiming settings, captures a held weapon's offset from lining its sights up on a marker, and offers fine adjustment. Its values live in the mod's config, which is not saved, so its "Copy as JSON" button puts the weapon's entry on the clipboard as one line of this table, graded `tuned`. Paste it over the weapon's line here (or add it, for a weapon the table lacks) and rebuild the payload. A regeneration keeps every `tuned` entry of the existing output in place of the derived entry for its weapon hash, so `--output` must point at the file that holds them.

## Pipeline

1. Collect the entity paths from the file lists: every `editor/entities/jc_weapons/{01_one_handed,02_two_handed}/*/*.ee` except the `_mag.ee` magazine entities.
2. Index every `.tab` under `archives_win64/` and `patch_win64/` by name hash. A file present in a patch archive is read from the patch.
3. Unpack each entity archive (AAF around a SARC). Take the standalone `.epe` from the archives when one exists (`patch_win64/game1` has one for every hand weapon), and the embedded one otherwise.
4. Parse the `.epe` (RTPC). Find the weapon object, its first type 2 `SWeaponPoint` (the fire position), and the model paths of the weapon and its components, skipping magazines and ammo belts.
5. Resolve each model path to its `.lod` list and read the closest LOD's `.rbm` vertex positions, transformed by the part's `world` matrix into weapon space.

## Algorithm

`sightline.py` works in the fire point's frame: the origin at the muzzle, x lateral, h up, and s rearward, so the barrel points toward -s. The barrel axis is the fire point's -Z axis and needs no inference from the model. L is the gun's length behind the muzzle.

First, the tool looks for a scope. It cuts the model into 2 cm slices along s. In each slice, among the vertices within 9 cm of the slice's top and above 2 cm, r is the largest |x|, and the axis height c is the mean height of the vertices within 1.5 mm of that width, because a tube is widest at its axis. The slice is a ring when r is between 12 mm and 45 mm, the top sits one radius above c (within 35%), the widest vertices lie on both sides of the centreline, and c is more than 3 cm above the bore. The longest run of rings whose axis heights step by less than 6 mm, spanning at least 10 cm, is the scope. The sight line runs through the first and last ring centres, which are the objective and the eyepiece.

Without a scope, the tool uses iron sights. It builds the top profile of the centreline slab, |x| of at most 4 mm, in 5 mm bins over the front 75% of L, which excludes the stock. The sight line is the longest edge of the profile's upper convex hull, which runs from the front post tip to the top of the rear sight.

From the line's two points it computes the height at the muzzle and the pitch, positive when the line climbs toward the muzzle. An iron-sight line is doubtful when its pitch exceeds 6 degrees, or when its two points span less than a quarter of L.

## Known failure cases

- The iron-sight rule takes whatever is highest on the centreline, not the sights. It catches a muzzle device (`w117`, `w118`), a carry handle or a top rail with a mounted optic (`w117`, `w121`, `w153`), a feed cover or ammo box (`w131`, `w132`, `w173`), and the optic on `w032`. The doubt checks flag most of these, but not `w118`, whose wrong line has a small pitch.
- The rocket launchers (`w141` to `w147`), `w176`, and the `w7xx` special weapons have no conventional sights, and their lines follow the tube or its fittings.
- Iron-sight pitch rests on a handful of vertices. The front sight is often modelled taller than the rear (the models are not zeroed, since the game is third-person), so a line through post and notch looks several degrees above the bore. `w111`'s line runs to the back of the receiver cover instead of the rear sight leaf.
- The 4 mm centreline slab can miss the bottom of a rear notch, so a pistol's line runs over the notch's sides.
- `w163`'s ring fit is slightly off and gives a small pitch that the scope does not have.
- `w144` has one fire position per tube; the tool uses the first.
- `w177_typhoon` fails: its model has a second block of a type the reader does not support.
- `w703_sungun` fails: it has no type 2 fire position.
- DLC weapons under `dlc_win64` are not read.

Two refinements would help the iron-sight cases: take the front sight as the tallest centreline point in the front 40% of the gun and the rear sight as the tallest in the next 30%, instead of the hull; and detect red-dot and holographic sight windows (vertices at both the top and bottom of a slice with a gap between) and use the window centre.

## Magazine wells

`extract_wells.py` derives each player weapon's reload geometry from the same models and writes it as `wells.json`: where the magazine, shell, or round goes in, which way it travels, and where the hand grabs to chamber. It covers the same weapons as `sights.json`, reads the archives the same way, and has the same prerequisites. The payload embeds the table at build time (`payload/src/controllers/weapons/reload/wells.rs`), so a change to it takes effect on the next payload build.

### Usage

```sh
# Regenerate the table beside the script.
python3 extract_wells.py

# Review views and the extracted entity files.
python3 extract_wells.py --plots /tmp/wells/png --dump /tmp/wells/files
```

`--game`, `--gibbed-tools`, and `--dump` behave as for `extract_sights.py`, and `--output` defaults to `wells.json` beside the script. `--plots DIR` writes a side, a top, and a front view per weapon: the gun's triangles in grey, the magazine or round part in red, the fire point as a blue cross, the mouth as a green circle with the insertion axis as an arrow ending at it, and the handle as a magenta cross. A run takes a couple of seconds (about 15 with plots) and logs one line per weapon to stderr, at warning level with the reasons when a result is doubtful.

### Frame

Every point and direction is in weapon space: metres, +Y up, -Z toward the muzzle, and +X to the gun's right. That is the frame of the gun's runtime world matrix and of each model part's `world` matrix, so a point here maps to the world through the gun's matrix alone. The origin sits near the grip.

### Output

A JSON array with one object per weapon entity, sorted by name, one object per line:

```json
{"weapon": "w111_assault_rifle_up_39", "weapon_hash": 2307691279, "kind": "magazine", "mouth_m": [-0.0003, 0.0382, -0.1414], "insert_axis": [0.0, 1.0, 0.0], "magazine_part": true, "part_name": "model.magazine", "part_top_m": 0.0679, "part_length_m": 0.1357, "action": "charging_handle", "handle_m": [0.0395, 0.1472, -0.1777], "travel_m": 0.07, "grade": "part", "notes": "travel: class default; handle: the receiver's right-side protrusion"}
```

- `weapon_hash` is `hashlittle` of the entity name, as in `sights.json`, and is the table's key.
- `kind` is how the weapon is loaded: `magazine`, `shell` (a shell at a time, through a port or into the breech), `rocket` (a round into the muzzle, or the Hydra's pod into the rear), `cylinder` (revolvers), or `none` (specials with nothing to load).
- `mouth_m` is the point on the gun's outer surface where the magazine or round enters. With a magazine part, it is where the part's feed end crosses into the body. Without one, the magazine is modelled into the body and cannot be hidden, so the mouth is the base of the visible magazine: the grip base on a pistol, the bottom of the magazine on an SMG. For shells it is the loading port or the breech, for rockets the front of the tube, and for revolvers the centre of the cylinder's rear face.
- `insert_axis` is the unit direction the magazine or round travels as it goes in.
- `magazine_part` and `part_name` name the separate part that carries the magazine or round (`model.magazine`, `model.cartridge_full`, `model.rocket_pod`, or `model.rpg`), when there is one. The part's offset matrix is on the component at runtime, so the payload can hide or move it.
- `part_top_m` is the feed end of that part, as a signed distance from the part's origin along `insert_axis`, and `part_length_m` is the part's extent along the axis. Without a part, `part_top_m` is null and `part_length_m` is the visible length of a magazine modelled below the receiver, or null when the magazine is inside the grip.
- `action` is what chambers a round: `slide`, `charging_handle`, `bolt`, `pump`, or `none`. `handle_m` is its grab point and `travel_m` its travel along the barrel.
- `grade` is the confidence in the mouth and axis: `part` (measured against a magazine part), `mesh` (measured from the body mesh), `default` (a class default, or nothing to load), or `failed`.
- `notes` names the fields that hold class defaults rather than measurements (every `travel_m`, and the handles that are not measured), says what the mouth is for that weapon, and repeats any doubt from the log.

### Algorithm

`wells.py` reads the triangles of every part as well as the positions (`rbm.mesh`) and measures by casting lines through them. A projection along an axis buckets the triangles by their footprint, and a line parallel to the axis reports the coordinates along the axis where it crosses a triangle.

The weapon's family, the class word of its name, picks the method, with the weapon's class for the launchers.

- Magazine part (assault rifles, machine guns, snipers, grenade launchers): the insertion axis is the part's +Y. The feed end is the part's vertices within 1.5 cm of its top along the axis. Lines on a 3 mm grid over the feed end's footprint, widened by 6 mm to catch a well's rim, run along the axis, and each keeps its first crossing of the body within 15 cm below to 5 cm above the feed end. The mouth is at the first quartile of those crossings, centred on the lines inside the footprint that crossed. When nothing crosses inside the feed end's footprint, the whole part's footprint is used.
- Magazine in the body (pistols, SMGs, and `w153`): the base is the body's lowest vertices. Lines along the barrel at heights from 2.8 to 6 cm above it find the column's front and rear faces, each starting from the previous centre so that a slanted column is followed, and a line fitted through the centres is the axis. The mouth is where that line leaves the base. A base more than 5 cm from the origin along the barrel is a magazine apart from the grip, and its visible length is how far the column rises before it widens by half into the receiver.
- Rockets: lines along the barrel within 7 cm of the round's axis find the front of the tube, at the first decile of their first crossings. The insertion axis is +Z. The Hydra's pod is treated as a magazine part sliding in along -Z.
- Revolvers: the cylinder is the longest run of the widest cross-sections around the bore, from lines cast across the gun. Lines along the barrel through the cylinder's side cross its front and then its rear face, and the mouth is the centre of the rear face.
- Pump shotgun: lines up through the centreline map the underside. The trigger guard is where a thin bar has a hole above it, within 15 cm ahead of the grip; the pump is the longest stretch of the lowest underside ahead of it. The mouth is the loading port, an opening in the receiver's underside between the two, and the handle is the pump's centre.
- Break-action shotgun: the breech is where the cross-section around the bore widens behind the barrel, at the bore's height.
- Handles: a pistol's handle is the rear of its slide part, and `w161`'s the tip of its recoiling `ejection_port` part. Otherwise the handle is the tip of the receiver's most prominent narrow protrusion on the right near the bore, standing at least 6 mm proud of the receiver around it, and without one, the class default: the top of the receiver over the grip.

When a mouth cannot be measured, the entry takes a class default placed from the measured weapons of its kind and is graded `default`.

### Known failure cases

- `w131` and `w132` hang their box to the right of the receiver, beside it rather than under it. The mouth is the receiver's underside over the box, about 2 cm below the box's top. Their handle is a protrusion beside the box and may be a latch rather than the charging handle.
- `w153` is graded as a box magazine, not a shell port: its model has a box magazine behind the grip, modelled into the body. It has no measured handle.
- `w123`'s cartridge pack sits under the barrels with an identity rotation, so its insertion axis is taken as +Y.
- `w032`'s handle is a knob on the right of the receiver over the grip, which may be a selector. `w033`'s handle is the class default.
- Every `travel_m` is a class default: the slide recoil component's properties are not named, and nothing in the model gives the travel.
- The grenade launchers get no action: their drums and barrel clamps dominate the side profile.
- `w155`'s fire position sits 14 cm behind its muzzle, so the breech search starts from the front of the model. `w176` and `w704` are treated as rockets because they carry a `model.rpg` round. `w147`, `w173`, `w178`, `w179`, and the `w7xx` specials other than `w704` have nothing to load.
- `w177_typhoon` fails, as for the sight lines. DLC weapons are not read.
