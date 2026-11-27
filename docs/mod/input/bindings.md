# Controller bindings

How the semantic actions ([controllers.md, "Action sets"](controllers.md#action-sets)) reach the game, and the controls summary on the wrist that describes them. The code is `crates/jc3vr-gameplay/src/controllers/bindings/` (tables in `tables/declare.rs`), with the suggested bindings in `crates/jc3vr-view/src/vr/input/semantic/defaults.rs`. The engine's input pipeline and write API are in [engine/gameplay/input.md](../../engine/gameplay/input.md); the look and move taps beside the bindings are in [input.md](input.md).

## Default layout

The suggestions target the Index controller. Below, R and L are the dominant hand and the off-hand, A and B the dominant hand's face buttons, and X and Y the off-hand's; "right A" and "left A" are fixed physical buttons that do not mirror for a left-handed player.

The face buttons mirror the gamepad by side, and an input that stands for a gamepad button injects every gameplay action the game's default keymap puts there, so the game's own context and tap-or-hold overloads pick the action ([the default gamepad keymap](../../engine/gameplay/input.md#the-default-gamepad-keymap)).

In every mode the system button is pause (held) and the commlink (tapped), holding the right B skips a skippable cutscene, and touching the off-hand trackpad shows the controls summary.

On foot, the L stick moves (head-relative, or by the off-hand's pointing) and the R stick turns through the shared VR turn (`headpose.vr_turn`), flicks down to toggle the seated crouch ([roomscale.md, "Head height"](roomscale.md#head-height)), and pushes up for the weapon wheel ([weapon-selector.md](../weapon-selector.md)). The R trigger fires, and the L trigger fires a gun in the off-hand or else retracts the tethers (moving to the L stick click while that hand holds a gun). Y is the grapple ([physical-grapple.md](physical-grapple.md#the-grapple-button)), A jumps, stunts, and opens the parachute, B uses, enters, and opens the wingsuit, X reloads ([reload.md](reload.md)), and holding the R stick click detonates placed C4. Weapons and grenades come from the body ([physical-weapons.md, "The sockets"](physical-weapons.md#the-sockets)), so nothing throws a grenade from a button. Melee is on no input, because the game's melee lunges the body toward its target. Falling and the parachute use the same table.

### Vehicles

Every vehicle class shares one scheme, and the grips only grip ([cockpit-controls.md](cockpit-controls.md), [steering-wheel.md](steering-wheel.md)). The L stick steers (the cyclic, or pitch and roll), the R stick's horizontal is yaw or rudder, and in the driver's seat the triggers are the pedals whatever the hands hold. B and Y fire vehicle weapons 1 and 2; a hand holding a gun fires that gun from its action instead ([vehicle-weapons.md](vehicle-weapons.md)), so rebinding `fire_primary` moves both. The L stick click grapples, the right A taps for the stunt position and holds to exit, the left A is the handbrake, and the R stick click is the vehicle mod (nitrous held, turbo jump tapped).

### Wingsuit

The wingsuit's buttons are on fixed physical hands: the left and right triggers fire the suit's machine gun and rockets, the right B boosts, the right A opens the parachute, the left A held is the air brake, and the left Y grapples. A handheld gun reserves only its own trigger. The arms steer, brake, close the suit, and roll ([flight.md](flight.md)), so the game's evade is on no button.

### Menus

Both hands navigate alike: A confirms, B goes back, either stick steps and adjusts, and a held grip turns the stick into tab steps and A and B into the gamepad's X and Y. The game's base menu input ([`UIBase::BaseOnManageInput`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_base::UIBase##func-BaseOnManageInput)) reads only the left stick's menu actions and repeats a held direction itself, so both sticks drive those actions, each latched to its larger axis, and the bindings add no repeat.

## Injection timing

The input tick runs in two stages (`controllers::bindings::stage`), because the device poll runs in the engine's input job, which overlaps game-thread work that writes the physics world and spawns and frees objects ([engine/gameplay/input.md, "Where the device poll runs"](../../engine/gameplay/input.md#where-the-device-poll-runs)).

- The game stage runs the mode detector, the bindings' decisions, and every subsystem's acts on the game thread, as the first band of the world's pre-simulation queue ([`UpdateQueue::Execute`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/update_queue::UpdateQueue##func-Execute)), after the input job and the update-queue commit; a grenade spawned before the commit crashes in the same tick's post-simulation queue. When a paused context skips the queue, the game stage runs after the update and the spawning acts refuse (`controllers::intent::stage`).
- The poll stage, the post-call detour on [`InputDeviceManager::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_device_manager::InputDeviceManager##func-Update), lands the last game stage's writes right after the poll, then taps the look ([input.md](input.md)). The writes cannot land in the game stage, because the pause and commlink toggles are read inside the input job and the next poll would overwrite them.

So a decision reaches the game one sim tick later, except where the input update runs inline on the game thread (loading), told apart by the `InputJob` thread role ([threading.md](../threading.md)).

## Tables and masks

Each tick the current mode's table is evaluated over the tick's semantic actions and injected; gestures, grabs, and chords read the raw snapshot instead. The bindings inject only while the controller scheme is in effect, a controller is active, the device poll runs, and the mod's panel is down, so a real gamepad is never fought. The tables are declared once over a `Sink` and read twice, by the injection and by the controls summary.

A table names actions, never hands or buttons, so it follows the player's rebinding. Where one input means different things by context, each meaning is its own action with the same default binding (`retract_tethers` and `retract_tethers_alt`, for example), and the table reads the one that fits.

What the mod claims of a hand is masked from the actions bound there, matched through each action's bound sources, and a masked action reads as released: a grip press latched to a grab, a trigger reserved by a held gun (`bindings/trigger_ownership.rs`, with vehicle fire routed in `bindings/gun_fire.rs`), the UI pointer's trigger in a menu, a consumed chord member until release, and a hand in recovery. Where sources are unread, failed, or all opaque paths, the action's default binding stands in; Monado names every source with an opaque path (`semantic::bound::opaque`), so there a rebinding is invisible to the masks and the chord arbiter.

## Overloads

A short `pause` press clicks the commlink and a long one pause (`bindings.long_press_s`). In a vehicle a short right A takes the stunt position, and a long one taps the gamepad's Y, which the game reads as the exit since it exits on the release of a briefly held Y ([engine/gameplay/input.md, "Leaving and changing seats"](../../engine/gameplay/input.md#leaving-and-changing-seats)). In the car, on the motorcycle, or on a mounted gun at speed, the long press instead holds `OPEN_PARACHUTE`, the bail (`controllers::bindings::bail`), chosen once per press so the hold and the tap never mix. The speed is the one the animation graph measures ([`Vehicle::GetGroundRelativePointVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-GetGroundRelativePointVelocity)), with hysteresis wholly above the graph's threshold, so the mod never bails where the game would refuse.

## Chords

The chords run on fixed physical hands, arbitrated in `controllers/bindings/policy_a/` over the two stick clicks and the right B and A; the accepted output feeds the bindings, the UI pointer, and the debug panel's pointer.

- Both stick clicks held recenter the view in every mode ([roomscale.md, "Recenters"](roomscale.md#recenters)); this takes priority.
- The left stick click, then the right B, toggles the debug panel.
- The left stick click, then the right A, drops a take marker ([capture.md](../capture.md#session-recording)).

A lone click waits out its chord windows before committing its own action, and a formed chord's members stay consumed until all are physically neutral. In a vehicle the left click is the grapple and the right B a weapon, so the panel and mark chords are withheld there.

Missing or stale sync history requires fresh neutral input, and a merged route that might include a consumed member stays quarantined until its own value is neutral, because OpenXR's source list names possible bindings rather than the contributor.

## Writes and their end

The game's readers tell a tap from a hold by the effector's state time, which [`LocalPlayerActionMap::ForceSetPressed`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::LocalPlayerActionMap##func-ForceSetPressed) zeroes on every call ([engine/gameplay/input.md, "The write API"](../../engine/gameplay/input.md#the-write-api)). So the hold tracker keeps each hold's time and writes the effector's whole state: a click on the first tick, a press with the time so far, and a release with the final time. Writes only raise values, so a physical device in the same effector is never reduced.

A write must not outlive the bindings' intent. While [`InputDeviceManager::m_Enabled`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_device_manager::InputDeviceManager##field-m_Enabled) is clear (how the debug panel takes input), the poll reads no devices and effectors keep their last state, so a stale pause click would open and close the menu in a loop. So every global stand-down undoes what the tracker wrote (released while the poll runs, idle while it does not), and a bare click not repeated is undone on the next tick.

## Stand-down and recovery

A game menu or the map pauses the weapon and vehicle handling rather than standing them down, so what the hands hold survives it. The global stand-downs (focus loss, no fresh input frame, a scheme change, the panel, a disabled poll) clear shared interaction state and disarm both hands; each hand rearms once it is neutral. Losing a hand's grip pose or activity masks only that hand, and a stick floor keeps a worn stick's drift from holding a hand masked. Input/bindings > Devices and thresholds shows each hand's state and the tick's writes.

## Gamepad semantics

The game applies the active device's rules from [`SettingType::EPCUseGamepad`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/settings_manager::SettingType##variant-EPCUseGamepad) (aim assist, the melee trigger, the prompts), and sets it from device events that action-level injection does not produce. The bindings are written against the gamepad rules, so under the controller scheme the entry is held at the gamepad's value, written directly rather than through the setter (which would re-select the port and may persist it), and restored when the pin ends unless a real device has written its own.

## The controls summary

The controller scheme hides the game's button hints, whose gamepad glyphs do not describe the controllers. Instead, a panel on the back of the off-hand wrist shows the controls usable now, above the mode's essentials. The model is `crates/jc3vr-gameplay/src/controllers/summary/`, the presentation `crates/jc3vr-hud/src/controllers/summary/` and `crates/jc3vr-hud/src/hud/summary_panel.rs`; the game's hints and localisation are [button-hints-and-localisation.md](../../engine/button-hints-and-localisation.md). It has no settings.

The rows describe the binding tables through the runtime's bound sources, so they follow rebinding, and an unbound row never appears. A row shows only while usable: `summary::refresh` takes an `Eligibility` snapshot of the input owner's final routing, so the summary never rediscovers ownership, and the subsystems behind the mod's own mechanics supply their rows from their post-step state. The grapple button's rows are the double tap that releases the tethers, the hold that tightens them, and, while the hook reels the character in on a line the lock can hold, the hold that locks the line ([grapple-lock.md](grapple-lock.md#the-button)); the lock replaces the tighten row for that stretch, so the button never shows two actions. Below them, `summary::essentials` lists the controls a player cannot find by trying (the tether release, the parachute and wingsuit, a vehicle's exit), because usable rows alone leave the wrist nearly empty when help is wanted.

A current hint from [`HUDUI::m_ButtonHints`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::hud_ui::HUDUI##field-m_ButtonHints) merges into its row but cannot revive an ineligible one. Overflow makes more pages, which cycle on a dwell rather than taking a paging input. The panel shows on a glance at the wrist, briefly after a mode change, and while the off-hand trackpad is touched.

## Unverified

Unchecked in the headset: that a float action's digital edges arrive in step with the raw grip and trigger, so a latched grab and the action it masks agree; that every runtime's bound sources name the components the masks match, or are all opaque; that the chord windows' delay on a lone click goes unnoticed; and the summary's paging and wrist placement.
