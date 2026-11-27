# The true reload

Under the controller scheme the game's timed reload is replaced by one done with the hands: the old magazine is ejected with a button, knocked out with the fresh one, or pulled out by hand; a fresh one is taken from the belt pouch and pushed into the gun's own well; and a gun can be reloaded in one hand by pushing it down onto the pouch. The code is `crates/jc3vr-weapons/src/controllers/weapons/reload/`, tuned by `crates/jc3vr-weapons/tuning/reload.toml` ([tuning.md](../tuning.md)). The setting is `controllers.weapons.reload.enabled` (`TRUE RELOAD` in the pause menu, on); off, the reload button plays the game's timed reload. The engine side is [weapon-ammo-and-reload.md](../../engine/gameplay/weapon-ammo-and-reload.md). None of it has been run in the headset.

## Which weapons and what they load

A weapon is reloaded by hand when it is in a hand, has a magazine limit, is not an untimed or unlimited-kind reload, and is of a class with something to load: pistols, revolvers, SMGs, rifles, shotguns, sniper rifles, grenade and rocket launchers, and the mortar (`plan.rs`). The minigun, mounted and vehicle guns, and the special weapons keep the game's reload.

What each weapon loads, where its well is in the model frame, and where its handle is come from the wells table `tools/sights/wells.json`, extracted from the game's archives ([tools/sights/README.md](../../../tools/sights/README.md)); a test in `plan.rs` holds the table to an audited list. A weapon the table lacks takes its class's load and a well beside the holding wrist. The loads are a magazine, a shell (one per push), a rocket (into the muzzle), and a revolver's cylinder (loaded whole from a speedloader).

Every plan is cocked (`ReloadPlan::cocked`): a seated magazine chambers its first round and no slide, bolt, or pump waits on the hand. The bolt rifle's update runs with its bolt-action setting cleared so the game never plays its bolt act, and its bolt lock ([`SniperWeapon::m_HasFiredThisInput`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::SniperWeapon##field-m_HasFiredThisInput)) is cleared once the trigger falls, so it fires once per pull; the pump shotgun keeps the game's own pump. An action worked by hand (`travel.rs`, `action.rs`) is dormant behind the debug switch `controllers.weapons.reload.hand_worked_action` (off; "Hand-worked gun action" on the debug UI's Reload and gun action page), and issue #59 decides its future.

## Stopping the game's reload

Every animated reload the game starts goes through [`Character::StartReloadingWieldingWeapon`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-StartReloadingWieldingWeapon). A detour refuses it for the local player while either hand's weapon is reloaded by hand (`gate.rs`), so the game's reload state never begins and nothing needs cleaning up. If the input tick stops publishing for `gate.stale_ms`, the detour stops refusing.

## Eject

A tap of the `reload` action (held, it is the game's holster, so only a release within `tap_s` ejects) ejects the magazine of the driest gun in either hand, the dominant hand's on a tie, passing over a gun already ejected, so a double tap ejects both (`eject.rs`). In a seat a double tap of an empty gun's trigger ejects it.

### The drop

The eject, knock, pull, and belt swap drop the magazine through one path (`reload/serial/`): the input tick queues a request of copied identities, and the detour of [`Character::UpdatePassPostPhysics_Serial`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdatePassPostPhysics_Serial) runs it for the local character before the original polls the animation's track messages, in the serial range where the game makes these calls itself. It rechecks everything before calling [`WeaponData::ThrowAwayMagazineObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##func-ThrowAwayMagazineObject) (dropping the spare prop and hiding the part), [`WeaponData::SpawnMagazineObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##func-SpawnMagazineObject), and setting [`WeaponBase::m_WeaponIsWithoutMagazine`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_WeaponIsWithoutMagazine), which refuses fire. A refused request changes nothing and plays the dry-fire click.

## Magazines in the world

A dropped magazine keeps its rounds and can be picked up and seated (`reload/loose/`). A manual drop that the fixed store can retain carries the magazine-owned rounds out (a live chamber stays); one it cannot retain leaves them counted in the weapon until the next seat returns them to the reserve, as the game's own reload does. While a gun's magazine is out, each noted magazine of its tag ([`WeaponData::m_MagazineUniqueTag`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_MagazineUniqueTag)) within `loose.range_m` is a grab target for the free hand; its own body is carried to the hand by capped velocities and seats with its own rounds.

## Pulling the magazine out

A free hand can pull a detachable magazine from a table-placed well (`reload/extraction/`): a fresh grip on the magazine starts the pull, and travelling `extraction.pull_m` out along the well's axis, measured relative to the moving gun, completes it, putting the retained magazine in the pulling hand. Opening the grip or straying cancels it. An active pull owns the hand, holds the gun's fire, and blocks its belt push; for a new grip on the magazine, `reload::prioritize_extraction` removes the overlapping brace, handover, and action targets.

### The magazine's zone

The handover's reach on a pistol grip covers the magazine below it, so a cylinder down the well's axis below the mouth is the magazine's (`reload/zones.rs`): a grip there starts the pull and never takes the gun, and only a grab above the well hands the gun over. Without a pull to start, a grab in the zone takes nothing. The grab view draws the zones ([hand-views.md](hand-views.md#the-grab-view)).

## Load

The magazine pouch ([weapon-handling.md](weapon-handling.md#pouches) places it) hands the free hand whatever the gun in the other hand takes: a magazine at any time (it seats only once the well is empty), a shell or rocket while the gun has room, a speedloader for a revolver. Its centre is the displayed model's (`pouch_models::display_centre`), so the player reaches for what they see.

The hand holds the round as a magazine is held for loading, feed end up the finger line (`insert.rs`). Its top catches the well within `capture_radius_m` of the mouth, within `capture_angle_deg` of the axis, and moving into the gun relative to the well (wider for launchers); it is generous because nothing stops the hand as a real gun would. A caught round follows the axis and seats at `seat_depth_m`; a round let go near the mouth floats in (`insert::Float`), and anything else drops. While a round is in hand the well is a touch target with a gold ring ([grip.md](grip.md#markers)), red while the old magazine is still in, and an X-ray outline at the seat guides the push (`reload/guide/`).

### What the hand shows

A pouch magazine is a second instance of the gun's spare magazine model ([grenades.md](grenades.md#the-magazine-in-the-hand)), so the old magazine stays drawn in the gun until it leaves; a seat hides the instance and shows the gun's part again ([`WeaponComponentBase::Enable`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponComponentBase##vfunc-Enable)). Shotgun and revolver rounds, which have no model, are drawn with the gun's ejected-casing model (`ReloadPlan::round`). A round with neither model (the UPU-210's cartridge, rockets, the Hydra's pod) is the gun's own round part, shown in the hand by the late update (`render::is_round_part`). The Automat 12, which has no magazine prop, borrows the UP-61's (`engine::borrow_spare`).

### The seat

A seated magazine, cylinder, or rocket loads through the weapon's own reload ([`WeaponBase::Reload`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##vfunc-Reload)), which keeps every game rule: the reserve, unlimited ammunition, upgrades, the magazine-out flag, and the HUD. A shell adds one round through [`WeaponBase::SetMagazineAmmo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##vfunc-SetMagazineAmmo), paid from the reserve.

## Knocking the old magazine out

While the well holds a magazine, a fresh one waits rather than catching, and a slow push is refused with the red ring and click. Two motions knock the old one out (`reload/knock/`, `reload/knocking.rs`): a push reaching the mouth at `push_knock_mps` or faster, after which the fresh one catches the empty well, or a strike on the inserted magazine at `knock.min_speed_mps` or faster. The held magazine has no body, so the strike is a swept test of its tip against a capsule down the well. The knock queues the drop and loads nothing; once the drop returns, an impulse at the struck point sends the prop on at `knock.share` of the tip's speed, and the gun's part stays shown until it lands, since a fresh prop is not drawn for a frame or two.

## The belt reload

Pushing a gun down onto its belt pouch reloads it in one hand (`belt.rs`), whether or not a hand is free: the magazine's base (or the mouth, or a revolver's cylinder face) comes within `belt.radius_m` of the pouch with the insertion axis near upright, then goes `belt.depth_m` deeper. A magazine still in drops as the fresh one goes in, and a shotgun takes one shell per push. An entry that cannot push (tilted, full, no reserve, or just drawn from the hip socket) does nothing and holds nothing back, so a gun lowered to the hip fires as ever; after a push, the gun cannot fire until it leaves the pouch. With a gun in each hand, each has its own pouch on its own side, marked with a gold ring while its push is eligible (`reload::belt_marks`).

## Launchers

The grenade and rocket launchers and the mortar have no action; a seated round fires at once ([weapon-ammo-and-reload.md, "Launchers"](../../engine/gameplay/weapon-ammo-and-reload.md#launchers)). A one-round launcher's spent round does not hold the well (`ReloadPlan::spent_round_replaced`), so a dry launcher reloads with one push, and the seat discards the spent round and ends each gun's burst ([`WeaponData::m_BurstShotsLeft`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_BurstShotsLeft)) first (`engine::discard_spent_round`), because a cut-short burst debits the magazine only at its last shot and would leave the seat finding it full.

## Revolvers

A revolver has no magazine prop, so it is reloaded as in the films (`reload/revolver.rs`, `reload/cylinders.rs`). Its skeleton carries the crane ([`WeaponData::CHAMBER_BONE_HASH`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##const-CHAMBER_BONE_HASH)) and cylinder ([`CHAMBER_SPIN_BONE_HASH`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##const-CHAMBER_SPIN_BONE_HASH)), swung as the game's reload clips do (`revolver::CRANE`, written after the pose update in `reload/bones.rs`). A flick of the wrist throws the cylinder out, tipping the muzzle up dumps it (unfired rounds return to the reserve, `reload/casings.rs` drops the casings), a speedloader seats against its rear face, and a flick back closes it. The belt push is the one-handed route: the shut cylinder's face is pushed onto the pouch's speedloader, refilling it over its rounds.

## The chamber

The game has no chamber inventory, so the reload partitions the loaded count into magazine-owned and chamber-owned rounds (`reload/ammo/`) without creating ammunition. A drop takes only the magazine's rounds, so a live chamber can fire once with the magazine out, and a tactical reload returns only the replaced magazine's rounds. A gun cannot fire while it is empty, its magazine is out with no live chamber, it is being ejected or loaded, its magazine is being pulled, its cylinder is out, or it sits in the pouch after a push (`fire_hold.rs`, `empty.rs`): its trigger is kept from the game and the pull plays a dry-fire click.

## Unlimited ammunition

The game's unlimited ammunition only makes its reload free, and that reload is refused ([weapon-ammo-and-reload.md, "Unlimited ammunition"](../../engine/gameplay/weapon-ammo-and-reload.md#unlimited-ammunition)). So each shot of a gun reloaded by hand puts its round back at the per-shot seam and each tick tops up a short magazine (`reload/infinite.rs`); a magazine dropped meanwhile carries no rounds, so made-up rounds never reach a finite load. The rule is the game's, read for the local player (`engine::unlimited`): [`Character::HasUnlimitedAmmo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-HasUnlimitedAmmo) or the tuning's [`m_unlimited_ammo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponTuning##field-m_unlimited_ammo), because the weapon's own query reads its user, which need not be the player for a gun armed in a hand.

## Each hand's gun

Every gun in a hand is reloaded on its own ([two-handed-inventory.md, "The per-hand tick"](two-handed-inventory.md#the-per-hand-tick)): plan, well, magazine, chamber, and fire hold are its own weapon's. A single pistol ([physical-weapons.md, "Separate pistols"](physical-weapons.md#separate-pistols)) loads half the pair's magazine, the excess of the game's reload returning to the reserve.

### Armed weapons left to the game

The game's reload only reloads the wielded weapon, and an armed weapon's own request is refused, so a weapon armed in a hand whose reload is the game's would never reload. It is refilled through `WeaponBase::Reload` once its magazine stays empty for `refill_delay_ms` (`reload/native.rs`), with no animation. A weapon with no magazine limit fires straight from the reserve ([two-handed-inventory.md, "Heavy weapons"](two-handed-inventory.md#heavy-weapons)).

## Haptics and sounds

Each step plays a weapons-category cue ([haptics.md](haptics.md)); a seat's thud is followed by a double pulse once the gun can fire.

### Sounds

The game voices a reload from its clips' track messages and its reload start ([weapon-ammo-and-reload.md, "The guns' sounds"](../../engine/gameplay/weapon-ammo-and-reload.md#the-guns-sounds)), none of which runs here, so each move plays the gun's own sound from its effects ([`WeaponData::m_AttachedEffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_AttachedEffects)) where it has one (`controllers::weapons::sounds`). A seat plays the insert sound, else the reload's end, else the dry-fire click borrowed as the magazine's catch (`engine::play_seat_sound`); a refusal plays the dry-fire click; a socket draw and dock play the draw and holster sounds ([weapon-sockets.md, "Drawing and docking"](weapon-sockets.md#drawing-and-docking)). The game's other reload sounds are whole reloads in one event, so the eject, knock, pull, and cylinder are silent.

## The readout

The debug UI's Weapons > Reload and gun action page shows the `Magazines` status row ([debug-ui.md](../debug-ui.md#status-rows)), the serial mailbox, and each reloaded weapon's state. The session log carries each step under `reload:`.

## Outstanding checks

Whether dropped props survive the spawn system and are readable in time for the knock's impulse; whether the magazine and casing models sit square in the hand; whether the table's wells and the knock capsule match the guns as drawn and the capture and knock speeds feel right; whether the crane joints written after the pre-simulation update render without racing; and whether a round put back at the per-shot seam survives a burst's late native debit.
