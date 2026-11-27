# Weapon entity data

A player weapon ships as an entity archive holding an entity property file, render models, and effects. The property file places the fire positions; the render models are static meshes with no sight or muzzle locators. The byte-level readers for every format below are in `tools/sights/`, whose module docstrings give the layouts.

## Where the files are

A player weapon is the entity archive `editor/entities/jc_weapons/<class dir>/<weapon>/<weapon>.ee`. The class directories are `01_one_handed`, `02_two_handed`, `03_thrown`, `04_mounted`, `05_stationary`, and `06_vehicle`; the player's hand weapons are in the first two, with their magazines as separate entities `<weapon>_mag.ee` beside them. Most hand weapons are in the base archive `archives_win64/game12`, a few in `game3`. A DLC's archives are under `dlc_win64/<dlc>/` (or its `arc/`), its paths start with `dlc/<dlc>/`, and its weapon entities sit directly in the class directory, as `dlc/sky_fortress/editor/entities/jc_weapons/02_two_handed/w119_molten_bavarium_spitter.ee` does. The DLC hand weapons are the Bavarium Splitter (`w119_molten_bavarium_spitter`, an assault rifle with a magazine part, shipped in both Sky Fortress and Mech Land Assault) and the Lightning Gun (`w180_lightninggun`, weapon name `w180_lightning_gun`). A DLC's prop spawn table is its own `dlc/<dlc>/settings/spawn_prop_defs.bin`.

The archives are `.tab`/`.arc` pairs: a `.tab` indexes its `.arc` by the Jenkins lookup3 `hashlittle` of each file's path, and the `.arc` stores each file uncompressed. The Gibbed tools' file lists map the hashes back to paths but are incomplete: `patch_win64/game1` holds a standalone replacement `.epe` for every hand weapon, and the lists name only some. The standalone copies differ in bytes from the copies embedded in the `.ee`, but their fire points and model parts are identical for every hand weapon; which copy the game loads has not been traced.

The `.ee` is an AAF container of zlib chunks around a SARC. It embeds everything the entity uses: the entity property file (`<weapon>.epe`), the tuning files (`.wtunec`), the model LOD lists (`.lod`), the render models (`.rbm`), collision (`.pfxc`), effects (`.effc`), and textures. A SARC entry at offset 0 is a reference to a file stored elsewhere.

## The weapon object

The `.epe` is an RTPC property container. The weapon is the top-level object that owns `SWeaponPoint` children: usually the first top-level object, though in launcher files effect tables and `CForcePulse` come first. Its `_class` is `CBulletWeaponBase`, `CSniperWeapon`, `CRPGWeapon`, and so on, and it carries `name` (the entity name) and `name_hash`, which is `hashlittle(name)`.

The runtime name hash, [`WeaponBase::m_WeaponNameHash`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_WeaponNameHash), is the same hash. `CRPGWeapon::UpdateFromUserPostSim_Parallel` compares it against `HashString` of names such as `w210_powercore`, and `CHUDUI` maps it to display names through `CUIMenu::GetWeaponNameById`. The store that sets it at load has not been found.

## Fire points

`CWeaponBase::SetProperties` reads the `SWeaponPoint` children, whose `type` property selects the role:

- A fire position (`FIRE_POSITION` in [`tools/sights/weapon.py`](../../../tools/sights/weapon.py)): its `world` matrix goes to `CWeaponData::AddInternalFirePosition` and becomes an entry of [`WeaponData::m_InternalFirePositions`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_InternalFirePositions).
- A gun's offset from the weapon base, one per `CWeaponData`.
- A third type, on `w147` and `w703`, which `SetProperties` does not add as a fire position.

The point's gun-index property (`WEAPON_DATA_INDEX_KEY` in the same script, which records its name hash) is the index of the `CWeaponData` the point belongs to, or a sentinel meaning every gun (the script records its value). Dual pistols use the sentinel, so each gun gets the same fire point in its own frame.

Every hand weapon's fire position has an identity rotation, so the barrel is the point's -Z axis and up is +Y, and it sits on the bore line at the muzzle. No hand weapon has a gun-offset point, so the gun frame, the weapon space, and the model space coincide. `w144` has one fire position per tube; `w703` has none.

## Models

The weapon object's model property (`MODEL_KEY` in the script) is `models/.../<model>_body.model_xml`, which resolves to the `.lod` file of the same stem in the `.ee`, whose first entry is the closest LOD's `.rbm`. Child components add parts, each with its own `world` matrix and model path:

- `CBarrelRecoilWeaponComponent` adds pistol slides at identity, one per dual-wielded gun.
- `CModelAttachementWeaponComponent` named `model.magazine` or `model.ammo_belt` adds magazines and ammo belts at an offset; the reload hides and shows the magazine part and drops a separate magazine prop ([weapon-ammo-and-reload.md](weapon-ammo-and-reload.md#the-magazine-drop-and-the-spare-prop)).
- Other components add parts such as `ejection_port`.

The model directory is not always the entity's own: `w013` uses `w011`'s models, `w014` uses `w021_revolver_peacekeeper_diravello`, `w132` and `w173` use `w131`'s body, `w146` uses `w141`'s, `w178` and `w179` use `w112`'s, and `w703` uses `w701`'s. The model path in the `.epe` is the only reliable mapping.

The hand-weapon render models are RBMDL files of `GeneralMkIII` blocks: static meshes with quantised positions and no bones or named locators, so the `.epe` fire position is the only muzzle locator. The exceptions are `w174_special_leafblower` (used by `w177_typhoon`), whose second block is a `General` block, and the Bavarium Splitter, whose body has a `Window` block for its glass vial. A block's type is `hashlittle` of its name.

Grip points come from shared `editor/entities/jc_weapons/00_shared/grip_points/*.wtunec` ADF files, which are not decoded here.
