# The true reload

Under the controller scheme the game's timed reload is replaced by one done with the hands: the magazine is ejected with a button or a flick of the gun, a fresh one is taken from a pouch on the belt and pushed into the gun's own magazine well, and a gun that ran dry is chambered by working its slide, charging handle, bolt, or pump. A gun can also be reloaded in one hand by pushing its well down onto the pouch. The code is in `payload/src/controllers/weapons/reload/`, the settings are `controllers.weapons.reload`, and the engine side is in [weapon-ammo-and-reload.md](../../engine/gameplay/weapon-ammo-and-reload.md). Nothing here has been run in the headset yet; see "Unverified" at the end.

## Which weapons

A weapon is reloaded by hand when it is wielded in the hands, has a magazine limit, is not one of the weapons whose reload has no animation, and is of a class with something to load: the pistols, revolvers, SMGs, rifles, shotguns, sniper rifles, grenade and rocket launchers, and the mortar (`plan.rs`). The minigun, the mounted and vehicle guns, and the special weapons keep the game's reload, as does every weapon with the reload off (`enabled`).

What a weapon loads and how its action is worked come from the wells table (see "Wells"), or from its class when the table has no entry for it:

| Load | Weapons | Eject | Load | Action |
|---|---|---|---|---|
| Magazine | pistols, SMGs, rifles, sniper rifles, grenade launchers, the Hydra's rocket pod | yes | a whole magazine | slide, charging handle, bolt, or none |
| Shell | shotguns | no | one shell per push | the pump of the pump-action shotgun, otherwise none |
| Rocket | rocket launchers, the mortar | no | one round into the muzzle | none |
| Cylinder | revolvers | yes (the spent cylinder) | a whole cylinder | none |

## Stopping the game's reload

Every animated reload the game starts goes through one call, [`Character::StartReloadingWieldingWeapon`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-StartReloadingWieldingWeapon): the reload button, the automatic reload of an empty or magazine-less gun, the predicted reload while idle, a reserve refilled by a pickup, an animation's request, and the launchers' and sniper rifles' own requests. A detour on it refuses the start for the local player while the input tick reloads the wielded weapon by hand (`gate.rs`). No reload act is queued, so the game's reload state never begins and nothing is left to clean up. The input tick publishes the weapon it reloads with a time stamp; if the tick stops for half a second, the detour stops refusing and the game reloads as usual. Switching the reload off mid-reload leaves a magazine-less gun to the game, which reloads it.

With `button` set to `Native`, a start while the game's reload input is held goes through: the reload button plays the game's timed reload, as an accessibility fallback, while the pouch, the flick, and the belt still work. With `Eject` (the default) the button ejects instead.

## Eject

A tap of the semantic `reload` action (the off-hand's lower face button on foot, in the default layout) ejects the dominant gun's magazine, or both pistols' when they are dual-wielded. The same button held is the game's holster, so only a press released within `tap_s` ejects. With `flick_eject` (off by default, since the button already ejects and a fast turn of the gun hand in play reads as a flick too easily), a gun hand turning faster than `flick_speed_dps` ejects that gun's magazine, at most once per `flick_cooldown_s`; the speed is the change in the hand's orientation between two input ticks (`eject.rs`).

The eject is the game's own magazine drop, run on the input tick: [`WeaponData::ThrowAwayMagazineObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##func-ThrowAwayMagazineObject) drops the gun's spare magazine prop from the magazine part (hiding the part) or from the gun itself, with the gun's velocity, so a flicked gun throws the magazine; [`WeaponData::SpawnMagazineObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##func-SpawnMagazineObject) asks for the next spare; and the weapon's magazine-out flag ([`WeaponBase::m_WeaponIsWithoutMagazine`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_WeaponIsWithoutMagazine)) is set, which the game itself reads to refuse fire. A weapon with no magazine prop drops nothing. The rounds left in the ejected magazine stay counted in the weapon until the next magazine seats, when the game's reload returns them to the reserve, as the game's own reload does; nothing is lost and there is no extra round in the chamber.

## Load

The pouch (`weapons.pouches.magazine`) is one universal pouch at the front of the belt on the side of the hand without the gun, so that hand reaches it without crossing the body, whichever hand the gun rides ([controllers.md](controllers.md#holsters-grab-volumes-the-foregrip-and-the-action-runs-tier) places it). It hands out whatever the held weapon takes, and it is a grab volume for the hand without a gun while a reloaded weapon is held. A grab with no ammunition left (and no unlimited ammunition) gives nothing and a faint pulse.

The hand holds the round with its top `held_top_m` ahead of the closed fist, pointing the way the fist points (`insert.rs`). The top catches the well when it comes within `capture_radius_m` of the well's mouth, turned no more than `capture_angle_deg` from the well's axis, and moving into the gun. A caught round slides only along the axis and seats `seat_depth_m` past the mouth; pulled back out it is held again. Letting go drops it, unless it is more than half seated, which seats it, and a round not seated within `timeout_s` is dropped. A magazine is taken only by a gun whose magazine is out; a shell or a rocket by a gun that is not full.

A seated magazine, cylinder, or rocket loads through the weapon's own reload ([`WeaponBase::Reload`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##vfunc-Reload)), which keeps every rule of the game's: the reserve, unlimited ammunition, the upgrades, the rocket on the launcher, the magazine-out flag, and the HUD. A shell adds one round through [`WeaponBase::SetMagazineAmmo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##vfunc-SetMagazineAmmo) and takes it from the reserve through [`Inventory::SetAmmoCount`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-SetAmmoCount), unless the ammunition is unlimited. Pickups, crates, and cheats only touch the reserve, so they keep working; the game's reload-on-refill is one of the starts the detour refuses.

While a round is in hand, a ring marks the well's mouth and a dot the hand holding the round. A weapon whose magazine is a separate model part shows the part again in the hand that took it, drawn at the hand's grip by the late update of the gun's parts, and hides it if the round is dropped with the gun's magazine still out; a seated magazine shows the part back in the gun ([`WeaponComponentBase::Enable`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponComponentBase##vfunc-Enable)). Pistols and SMGs draw their magazine as part of the body, so nothing but the dot shows in the hand. The ring and the dot are a debugging aid, off by default (`show_well`).

## Wells

The well's mouth and axis are in the gun's model frame, from the wells table (`tools/sights/wells.json`, derived from the game's models by `tools/sights/extract_wells.py`; see [the tool's README](../../../tools/sights/README.md#magazine-wells)). A weapon with a separate magazine part has its well where the part enters the gun; pistols and SMGs, whose magazine is part of the body mesh, have it at the bottom of the grip. The gun's frame is carried to the hand by measuring the gun's world matrix against the wrist target its placement followed, as the foregrip does (`frames.rs`); the measurement needs the wrist at full weight and is kept until the next one. A weapon the table lacks (or grades as failed) has its well at `well_offset_m` in the holding wrist's frame, its axis toward the wrist.

## The belt reload

Pushing a gun's well down onto the pouch reloads it in one hand (`belt.rs`), for every weapon reloaded by hand, whether or not a hand is free. The mouth has to enter the pouch moving down faster than `belt.min_push_speed_mps`, with the gun upright enough that its insertion axis leans no more than `belt.max_tilt_deg` from straight up, and then go `belt.depth_m` into it. The pouch is placed where the player's own hand reaches it, before the body-scale remap, and the gun rides the hand as drawn, so the mouth is carried back by that hand's remap shift before the two are compared. An entry that misses any of these does nothing until the mouth leaves the pouch, and the belt waits `belt.after_draw_s` after a draw, so drawing from the hip holster beside the pouch never reloads. A magazine still in the gun drops as the fresh one goes in, so no eject is needed first. A belt reload works the action as well. The gun cannot fire from the push until the mouth leaves the pouch. A shotgun takes one shell per push.

## The chamber

The game has no chambered round; a gun fires while its magazine has rounds. The chamber is the mod's (`chamber.rs`). With `chamber` on, a gun that fires its last round has an empty chamber, and a gun that is reloaded while its chamber is empty stays empty until its action is worked: the trigger only clicks. A tactical reload, of a gun with rounds left, keeps the round in the chamber. There is no slide-release button; the belt reload chambers by itself.

The action is worked with the hand without a gun:

- A slide or a charging handle: a grip press at the handle (a grab volume while the chamber waits, so the press does not fire the grapple), a pull back along the gun's axis to `action.pull_fraction` of its travel, and a release or a push back home.
- A bolt: the same, but it has to be pushed back home.
- A pump: the hand already on the foregrip slides back and forward along the barrel.

The handle's place and travel come from the wells table; a weapon whose handle the table does not place is never held for its chamber.

With `manual_action` on, a bolt-action rifle and a pump-action shotgun empty their chamber after every shot, whether or not `chamber` is on, and the game's own cycling is suppressed (see "The bolt and the pump"). A worked action loads the chamber when the magazine is in and holds a round. While a pistol's chamber is empty its slide is drawn back, and while it is worked it follows the hand: the game moves a slide back along its own axis after each shot and all the way while the magazine is empty ([`BarrelRecoilWeaponComponent::m_RecoilMaxLength`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::BarrelRecoilWeaponComponent##field-m_RecoilMaxLength)), and the late update of the gun's parts adds the rest (`render.rs`, through `hands/parts.rs`).

A gun held by its chamber, by a magazine out, or by the belt cannot fire: the input tick keeps its trigger from the game and pulses `weapons.haptics.dry_fire` at the pull's press edge, and the ammunition check clears the gun's shot as well against a buffered one (`empty.rs`). With the dual-wield split a held pistol's trigger is kept from the split, so the other pistol still fires.

## The bolt and the pump

The sniper rifle keeps its own bolt lock ([`SniperWeapon::m_HasFiredThisInput`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::SniperWeapon##field-m_HasFiredThisInput)), set by each shot, which blocks fire until the bolt animation runs. With `manual_action`, the rifle's update runs with its bolt-action setting cleared, so the game never queues the bolt act, and the lock keeps the rifle from firing until the player works the bolt; the worked bolt clears the lock. A reload by hand does not raise the reloading state that would clear the lock, so a rifle reloaded after running dry still needs its bolt worked.

The pump-action shotgun's pump is queued on every shot by its do-act component ([`DoActWeaponComponent::Fire_Start`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::DoActWeaponComponent##func-Fire_Start)), on the holder and on the gun's own skeleton. With `manual_action` the component's fire start is skipped for the local player, and nothing in the game stops the next shot, so the chamber does. The worked pump plays the gun's pump clip ([`WeaponData::DoAct`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##func-DoAct)).

## Dual-wield

With `dual_split` (and the dual-wield split on), each pistol has half the pair's magazine, the dominant hand taking the odd round (`dual.rs`). The game keeps one magazine, so the split keeps a count per hand beside it, and the game's magazine is always their sum; a change the split did not make (the game's own reload while the reload is off) deals the counts again. A shot takes from its hand's count, and a hand at zero is dry while the other still fires. An eject drops that pistol's magazine prop and returns its rounds to the reserve; a load fills only that hand's half from the reserve. The magazine-out flag is not used for a pistol of the pair, since it would stop both.

## Haptics

`haptics` holds the reload's pulses: the eject on the gun hand, a round taken from the pouch, an empty pouch, a round caught by the well and a seat on both hands, a belt seat on the gun hand, a full pull of the action on the working hand, and a chambered round on the gun hand. The dry fire is `weapons.haptics.dry_fire`.

## The readout

The Controllers tab's weapon section shows the reloaded weapon's name hash, its load and action, whether its well came from the table, the magazine and the reserve, each gun hand's chamber, split magazine, belt push, fire hold, and turning speed, the round in hand and its distance from the mouth, the action's work, the recent reload events, and the counters (ejects, seats, belt reloads, actions worked, dry fires, empty pouch grabs, and the game's reload starts refused).

## Unverified

None of this has been run in the headset. In particular:

- that calling the weapon's reload, the magazine drop, and the magazine setter from the input tick, on the main thread outside the world update, is safe: the game calls them from an animation track message inside the character's update, and the drop writes a physics body;
- that the HUD's ammunition counter follows the reload and the setter from the input tick;
- the wells table's points, the grab radius of the handles, and the flick speed;
- that a hidden magazine part shows again when a magazine seats, and that the held magazine looks right;
- that clearing the sniper rifle's bolt-action setting for its update skips the act and nothing else, and that skipping the pump's do-act start leaves nothing else waiting on it;
- that the animation's reads of the magazine-out flag pick no visibly different idle or draw clips while the magazine is out.
