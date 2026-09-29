# Weapon entity data

A player weapon ships as an entity archive with an entity property file, render models, and effects. The entity property file places the fire positions; the render models are static meshes with no sight or muzzle locators. The byte-level readers for every format below are in `tools/sights/`, whose module docstrings give the layouts.

## Where the files are

A player weapon is the entity archive `editor/entities/jc_weapons/<class dir>/<weapon>/<weapon>.ee`. The class directories are `01_one_handed`, `02_two_handed`, `03_thrown`, `04_mounted`, `05_stationary`, and `06_vehicle`. The player's hand weapons are the entities in `01_one_handed` and `02_two_handed`. Their magazines are separate entities in the same directory, named `<weapon>_mag.ee`. Most hand weapons are in the base archive `archives_win64/game12`, and a few are in `game3`. DLC weapons are under `dlc_win64` and are not covered here.

The archives are `.tab`/`.arc` pairs. A `.tab` indexes its `.arc` by the Jenkins lookup3 `hashlittle` of each file's path, and the `.arc` stores each file uncompressed. The Gibbed tools' file lists map the hashes back to paths. The file lists are incomplete: `patch_win64/game1` holds a standalone replacement `.epe` for every hand weapon, but the lists name only some of them. The standalone copies differ in bytes from the copies embedded in the `.ee`, but their fire points and model parts are identical for every hand weapon. Which copy the game loads has not been traced; since they agree on fire points and models, the choice does not change the geometry.

The `.ee` is an AAF container of zlib chunks around a SARC. It embeds everything the entity uses: the entity property file (`<weapon>.epe`), the tuning files (`.wtunec`), the model LOD lists (`.lod`), the render models (`.rbm`), collision (`.pfxc`), effects (`.effc`), and textures. A SARC entry at offset 0 is a reference to a file stored elsewhere.

## The weapon object

The `.epe` is an RTPC property container. The weapon is the top-level object that owns `SWeaponPoint` children. In most files it is the first top-level object; in launcher files, effect tables and `CForcePulse` come first. Its `_class` is `CBulletWeaponBase`, `CSniperWeapon`, `CRPGWeapon`, and so on. It carries `name` (the entity name) and `name_hash`, which is `hashlittle(name)`.

The weapon's runtime name hash, [`WeaponBase::m_WeaponNameHash`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_WeaponNameHash), is the same hash. The game compares it against `HashString` of weapon names such as `w210_powercore` in `CRPGWeapon::UpdateFromUserPostSim_Parallel`, and `CHUDUI` maps it to display names through `CUIMenu::GetWeaponNameById`. The store that sets it at load has not been found.

## Fire points

`CWeaponBase::SetProperties` reads the `SWeaponPoint` children. The `type` property selects the point's role:

- Type 2 is a fire position. Its `world` matrix goes to `CWeaponData::AddInternalFirePosition` and becomes an entry of [`WeaponData::m_InternalFirePositions`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_InternalFirePositions).
- Type 0 is a gun's offset from the weapon base, one per `CWeaponData`.
- Type 1 appears on `w147` and `w703`. `SetProperties` does not add it as a fire position.

The point's gun-index property (named `WEAPON_DATA_INDEX_KEY` in [`tools/sights/weapon.py`](../../../tools/sights/weapon.py), which records its name hash) is the index of the `CWeaponData` the point belongs to, and -1 means every gun. Dual pistols use -1, so each gun gets the same fire point in its own frame.

Every hand weapon's fire position has an identity rotation, so the barrel is the point's -Z axis and up is +Y. The fire position sits on the bore line at the muzzle. No hand weapon has a type 0 point, so the gun frame, the weapon space, and the model space coincide. `w144` has several distinct fire positions, one per tube. `w703` has no type 2 point.

## Models

The weapon object's model property (named `MODEL_KEY` in [`tools/sights/weapon.py`](../../../tools/sights/weapon.py)) is `models/.../<model>_body.model_xml`. The path resolves to the `.lod` file of the same stem in the `.ee`, whose first entry is the closest LOD's `.rbm`. Child components add parts, each with its own `world` matrix and model path:

- `CBarrelRecoilWeaponComponent` adds pistol slides at identity, one per dual-wielded gun.
- `CModelAttachementWeaponComponent` named `model.magazine` or `model.ammo_belt` adds magazines and ammo belts at an offset.
- Other components add parts such as `ejection_port`.

The model directory is not always the entity's own. `w013` uses `w011`'s models, `w014` uses `w021_revolver_peacekeeper_diravello`, `w132` and `w173` use `w131`'s body, `w146` uses `w141`'s, `w178` and `w179` use `w112`'s, and `w703` uses `w701`'s. The model path in the `.epe` is the only reliable mapping.

The hand-weapon render models are RBMDL files of `GeneralJC3` blocks: static meshes with quantised positions and no bones. They carry no named sight or muzzle locators; the `.epe` fire position is the only muzzle locator. The exception is `w174_special_leafblower` (used by `w177_typhoon`), whose second block is of another block type.

Grip points come from shared `editor/entities/jc_weapons/00_shared/grip_points/*.wtunec` ADF files, which are not decoded here.
