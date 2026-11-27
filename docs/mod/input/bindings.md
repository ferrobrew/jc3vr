# Controller bindings

How the semantic actions ([controllers.md, "Action sets"](controllers.md#action-sets)) reach the game: the default layout, the binding tables, when their writes land, the masks, the menus, the chords, and what ends a write. The code is `crates/jc3vr-gameplay/src/controllers/bindings/`, with the tables in `tables/declare.rs` and the suggestions in `crates/jc3vr-view/src/vr/input/semantic/defaults.rs`. The engine's input pipeline and write API are in [engine/gameplay/input.md](../../engine/gameplay/input.md); the look and move taps that run beside the bindings are in [input.md](input.md).

## Default layout

The suggestions are laid out for the Index controller, which has A and B on both hands. Below, R and L are the dominant hand and the off-hand, A and B the dominant hand's lower and upper face buttons, and X and Y the off-hand's. "Right A" and "left A" are fixed physical buttons that do not mirror for a left-handed player. On the Touch the left controller's X, Y, and menu button stand for A, B, and the system button, and there is no trackpad.

The face buttons mirror the gamepad by side, so gamepad habits carry over: its A and Y (jump, and use or enter) are on the dominant hand, its X and B (reload, and the tethers' release) on the off-hand. Where an input stands for a gamepad button, its table holds every gameplay action the game's default gamepad keymap puts on that button, and the game's own context picks the one that applies, which keeps the game's tap and hold overloads: the reload button's short hold holsters, `USE_VEHICLE_MOD` is nitrous held and the turbo jump tapped, a held `ENTER_VEHICLE` takes the gunner seat, and a held `FIRE_GRAPPLE` tethers ([the default gamepad keymap](../../engine/gameplay/input.md#the-default-gamepad-keymap)).

In every mode the system button of either hand is pause (held) and the commlink (tapped), holding the right B outside the menus skips a skippable cutscene (`skip_cutscene`, which shares the button with the mode's own action, as the gamepad's B does), and touching the off-hand trackpad shows the controls summary. `open_commlink`, `recenter_view`, and `toggle_debug_panel` ship unbound, since the chords below do those.

### On foot

| Action | Default |
|---|---|
| Move, body turn | L stick (head-relative, or the off-hand's pointing with `controllers.locomotion.direction` set to `OffHand`); R stick horizontal, through the shared VR turn settings (`headpose.vr_turn`) |
| Fire | R trigger; a gun in the off-hand fires from the L trigger |
| Grapple | Y: a tap fires, a hold on a target tethers, a hold with tethers attached and nothing new under the aim tightens them, a double tap releases them ([physical-grapple.md](physical-grapple.md#the-grapple-button)) |
| Retract tethers | L trigger, or the L stick click while the off-hand holds a gun |
| Jump, stunt, open the parachute | A |
| Use, enter a vehicle, open the wingsuit | B; held, it also steps off a ledge the character would stop at, and keeps a ragdoll going |
| Reload | X, which ejects under the true reload ([reload.md](reload.md)); a short hold holsters, as on the gamepad |
| Detonate placed C4 | hold the R stick click (`DETONATE_EXPLOSIVE` alone, past the game's hold time) |
| Crouch toggle (seated posture) | R stick flicked down ([roomscale.md, "Head height"](roomscale.md#head-height)) |
| Weapon wheel | R stick pushed up, while the wheel is on ([weapon-selector.md](../weapon-selector.md)) |
| Next weapon | the L stick click, only with `weapons.holsters.stick_click_cycles` on and no gun in the off-hand |

Weapons are drawn from and docked on the body's sockets ([physical-weapons.md, "The sockets"](physical-weapons.md#the-sockets)), grenades and C4 come from their pouches, and nothing throws a grenade from a button. The d-pad's slot selects (`weapon_select_pad`, `weapon_select_press`) and `throw_grenade` exist for custom layouts and ship unbound. Melee (`PUSH_GRAPPLE`) is on no input, because the game's melee lunges the body toward its target, taking the body from the player, and on the button that releases tethers it would melee on every release.

Falling and the parachute use the same set and the same table without the move frame's camera rotation, the weapon cycle, and the ledge bypass. The parachute handles and the ripcord are grip targets ([flight.md](flight.md)).

### Vehicles

Every vehicle class shares one scheme. The grips grip only: the guns, the wheel, the handlebars, the levers, the handles ([cockpit-controls.md](cockpit-controls.md), [steering-wheel.md](steering-wheel.md)).

| Action | Default |
|---|---|
| Steer (car, motorcycle, boat), cyclic (helicopter), pitch and roll (plane) | L stick |
| Yaw (helicopter), rudder (plane) | R stick horizontal |
| Accelerate and reverse, collective up and down, thrust up and down | R trigger and L trigger, in the driver's seat |
| Fire vehicle weapons 1 and 2 | B and Y; a hand holding a gun fires that gun instead ([vehicle-weapons.md](vehicle-weapons.md)) |
| Fire the hand weapon out of the driver's seat | R trigger, and the L trigger for a gun in the off-hand |
| Grapple | L stick click, with the same gestures as on foot |
| Stunt position, exit | right A: tap and hold |
| Handbrake | left A |
| Nitrous and turbo jump, the helicopter's vehicle mod | R stick click held and tapped |
| Horn, a vehicle's magnet | the cockpit's horn button; `horn` ships unbound |

A gun takes over its hand's vehicle weapon action, not a button, so rebinding `fire_primary` moves the vehicle weapon and the held gun together. In the driver's seat the triggers stay the pedals whatever the hands hold.

The mech uses the land vehicle set: the stick walks it, the R trigger fires its right-hand weapon, the L trigger works the gravity gun (attract while held, throw on release) and B drops what it holds, Y punches, the right A tapped jumps and held exits, and the L stick click grapples. It has no physical controls.

### Wingsuit

The wingsuit's buttons are on fixed physical hands: the left trigger fires the weaponized suit's machine gun and the right trigger its rockets, the right B boosts, the right A opens the parachute, the left A held is the air brake, and the left Y grapples. A handheld gun reserves only its own trigger, so the other trigger still fires its suit weapon. The arms steer, brake, and close the suit and throw the barrel roll ([wingsuit-gestures.md](wingsuit-gestures.md), [flight.md](flight.md)), so the game's evade (`WINGSUIT_EVADE`) is on no button. The suit's guns aim by the gaze, because the gun aim covers only the wielded slot.

### Menus

Both hands navigate alike, whichever is dominant. Either A confirms (`GUI_OK`) and either B goes back (`GUI_CANCEL`). Either stick steps through a list and, pushed left or right, adjusts the selected row. Each hand's grip is its modifier: held, the stick's left and right step the tabs, and A and B become the gamepad's X and Y (the special and filter actions, and the use action that resets a setting). The grip press is the deliberate one (the force on the Index), whether a press is modified is latched at its press edge, and a grip alone does nothing. A press on either half of a trackpad also steps the tabs; a touch alone does not, so the touch stays free for the controls summary. The left trigger pages back and the right forward, the other way with the same hand's grip held, so both pages stay in reach while the native UI pointer owns one trigger. The left stick click is `GUI_FOCUS` and the right `GUI_RECENTER`, which recenters the map, not the view.

The game's base menu input ([`UIBase::BaseOnManageInput`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_base::UIBase##func-BaseOnManageInput)) reads only the left stick's menu actions, one direction at a time, and repeats a held direction itself, and the d-pad's actions repeat through separate handlers. So both sticks drive the left stick's actions and the bindings add no repeat. A stick read as raw half-axes would step up when pushed mostly sideways, so each hand latches the direction of its larger axis once it passes the controls tuning's `bindings.menu_stick_push` and holds it until the push falls below `bindings.menu_stick_hold_share` of that (gameplay tuning).

## Injection timing

The input tick runs in two stages (`controllers::bindings::stage`), because in play the device poll runs in the engine's input job, which overlaps game-thread work that writes the physics world, spawns and deletes objects, and frees animation controls ([engine/gameplay/input.md, "Where the device poll runs"](../../engine/gameplay/input.md#where-the-device-poll-runs)).

- The game stage runs the mode detector, the bindings' decisions, every subsystem's acts (the weapon handling, the inventory, the snatch, the physical grapple, the physical hands, the vehicles), and the gamepad-semantics pin, on the game thread as the first band of the world's pre-simulation queue ([`UpdateQueue::Execute`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/update_queue::UpdateQueue##func-Execute)). That is after the input job has finished and after the update-queue commit, so whatever the acts spawn is registered after the commit and first updated in the next tick, as the engine's own spawns are; a grenade spawned before the commit crashes the game in the same tick's post-simulation queue. When the update skips its queue (a paused context), the game stage runs after it, and the acts that spawn such objects refuse there through the permission `controllers::intent::stage` sets, since a paused update also skips the ammunition manager's pass that completes them.
- The poll stage is the post-call detour on [`InputDeviceManager::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_device_manager::InputDeviceManager##func-Update). It lands the writes the last game stage produced, right after the device poll, then reads and clears the look effectors ([input.md](input.md)). The writes cannot land in the game stage, because the pause menu's and the commlink's toggles are read inside the input job and a race's reset before the world update, and the next poll would overwrite them.

So a decision reaches the game one sim tick after it is made, after a poll and before every reader. Where the input update runs inline on the game thread (the load state and the frontend's loading), the game stage runs right after the poll stage and its writes land at once; the poll stage tells the two cases apart by the `InputJob` thread role ([threading.md](../threading.md)). A handoff carries the tick, the pending writes, and the body turn between the stages.

## Tables and masks

On every input tick the bindings evaluate the current mode's table over the tick's semantic actions (the latest `SemanticFrame` and the edges since the last tick) and inject the result. The gestures, grabs, and chords read the raw snapshot instead, reduced to deadzoned sticks and triggers, per-hand buttons with their edges, and what each hand's grip would take ([grip.md](grip.md)). The bindings inject only while the controller scheme is in effect, a controller is active, the game's device poll runs, and the mod's panel is down, so a real gamepad is never fought. The tables are declared once over a `Sink` and read twice: by the injection and by the controls summary ([controls-summary.md](controls-summary.md)).

A table names actions, never a hand or a button, so it acts on whatever the player bound them to, and a press counts on either hand. Where one input means different things by context, each meaning is its own action with the same default binding, and the table reads only the one that fits: `retract_tethers` on the off-hand trigger, or `retract_tethers_alt` on the off-hand stick click while that trigger fires a gun (`fire_off_hand_pistol`); `next_weapon` on the same click only while weapon cycling applies. The `turn` action drives the body turn, which the poll stage hands to the headpose's VR turn scheme rather than injecting, except in the helicopter and the plane, whose tables read their own yaw and rudder there.

What the mod claims of a hand is masked from the semantic actions bound there, matched through each action's bound sources. A masked action reads as released on that hand. The claims:

- A grip press latched to a grab masks that hand's grip for the whole press, so a holster draw never also grapples.
- A hand holding any part of a gun reserves its trigger for that gun (`bindings/trigger_ownership.rs`); outside the driver's seat its vehicle weapon action fires the gun too (`bindings/gun_fire.rs`).
- In a menu, the native UI pointer's trigger serves only `pointer_select`.
- A consumed chord member stays masked until it is released (see "Chords"), and input recovery masks a whole hand (see "Stand-down and recovery").
- An action bound to a stick click reads the click as the chord arbiter delivers it, not its raw state.
- A source the mod does not recognise is masked only with its whole hand.

Until a set's sources have been read, for an action whose enumeration failed, and for one whose sources are all opaque paths under no user path, the action's default binding stands in for its sources. Monado, natively and through wineopenxr, names every source with such a path (`/mnd/...`), which OpenXR says is opaque (`semantic::bound::opaque`); there a player's rebinding is invisible to the masks and the arbiter, which keep the suggested layout. An enumeration that mixes user paths with other paths is read path by path.

Float actions with press behaviour use their action's trigger, grip, or trackpad threshold with hysteresis, not the suggested binding's.

## Overloads

Two overloads the mod resolves itself, by the controls tuning's `bindings.long_press_s`. The `pause` action: a press shorter than that clicks the commlink on release, and one held that long clicks pause. In a vehicle, the stunt and the exit on the right A: a press released sooner takes the stunt position on its release, and one held that long taps the gamepad's Y at that moment, which the game reads as the exit, since it exits only on a release of a briefly held Y ([engine/gameplay/input.md, "Leaving and changing seats"](../../engine/gameplay/input.md#leaving-and-changing-seats)). The game's long hold of Y, the seat change, is not offered: it would have to be told apart from the exit before the exit is sent.

In the car's driver's seat, on the motorcycle, and on a mounted gun, with the vehicle moving fast enough, the long press holds the gamepad A's `OPEN_PARACHUTE` instead, from the moment it becomes long until it ends (`sink::Input::LongHeld`), which the game takes as the parachute bail, and never taps Y as well, since the game would queue the plain exit first. The choice is made once per press, on the tick it becomes long (`controllers::bindings::bail`): a choice that changed mid-press would drop the held `OPEN_PARACHUTE` without the tap, or hold it after the tap. Between presses the speed is read through a hysteresis band wholly above `PARACHUTE_BAIL_MIN_SPEED_MPS`, the animation graph's bail condition, from the gameplay tuning's `bindings.bail.enter_margin_mps` and `leave_margin_mps`, so a vehicle near the bail speed neither flickers nor bails where the game would refuse. The speed is the one the graph measures, the vehicle's velocity at Rico relative to what it stands on ([`Vehicle::GetGroundRelativePointVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Vehicle##func-GetGroundRelativePointVelocity)). Of the game's mounted gun states only the spinning mounted weapon's has the bail, and the bindings do not tell a fixed mount from a spinning one, so a fixed mount at speed would hold `OPEN_PARACHUTE` to no effect.

## Chords

The chords run only under the controller scheme, on fixed physical hands. `crates/jc3vr-gameplay/src/controllers/bindings/policy_a/` arbitrates them over the left stick click, the right stick click, the right B, and the right A, and its accepted output feeds the game bindings, the native UI pointer, and the debug panel's pointer alike.

- Both stick clicks held together for the controls tuning's `bindings.recenter_hold_s` recenter the view in every mode. The second click must join within `bindings.stick_chord_window_s`, in either order, and the hold starts when it joins. Completion requests the deferred manual recenter ([roomscale.md, "Recenters"](roomscale.md#recenters)) and pulses both hands. Recenter takes priority over the other chords.
- The left stick click, then the right B within `bindings.panel_chord_window_s`, toggles the debug panel, through the same transition as the keyboard shortcut.
- The left stick click, then the right A within the same window, drops a take marker, the same action as F2 ([capture.md](../capture.md#session-recording)).

A lone click waits for its chord windows before committing its single action: the left click for the longer of the recenter and panel windows, the right click for the recenter window. A tap released sooner is delivered on its release, and a released reserved click is delivered as a press followed by a release, with no credit for the withheld hold time. The right B or A alone commits at once while the left click is neutral; one pressed after the left click has committed as a single is an ordinary press. Order matters: a sync that brings both new presses at once does not establish left-first order and forms no panel or mark chord. When the right B and A both join the left click, the panel forms and the A is refused. Every member of a formed chord stays consumed in every route until one observation finds them all physically neutral, and their release edges are suppressed.

In a vehicle the left stick click is a right-handed player's grapple and the right B the primary fire, so the panel and mark chords are withheld whenever a vehicle set is active: the left click waits only for the recenter window, and a right B or A after it fires or acts at once.

The arbiter reads an ordered history of action syncs, each a coherent raw and semantic group stamped with its observation time, controller-frame identity, action schema, binding generation, and source-query outcomes, from a bounded queue separate from the recording's. Missing, stale, invalid, or overflowed history resets the affected routes and requires fresh neutral input, and a sample whose action read failed, whose source is inactive, or whose hand has no usable grip pose cannot establish neutral. Focus, tracking, binding, and context changes require verified neutral without crediting a held input as a new press. The source list OpenXR returns names possible bindings, not the contributor to a merged action value, so the arbiter keeps the sampled semantic value and never substitutes a raw click, and a merged semantic route that could include a consumed member stays quarantined until its own value is neutral.

The recorded policy decision has its own version: version 5 adds whether the developer chords are withheld in a vehicle set, version 4 decodes with the suppression read as off, and version 3 or earlier does not decode. Replay re-evaluates each recorded group and compares the decision before dispatch, and resumes after a missing or invalid decision only at a validated neutral boundary.

## Writes and their end

The game's readers tell a tap from a hold by the effector's state time, and [`LocalPlayerActionMap::ForceSetPressed`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::LocalPlayerActionMap##func-ForceSetPressed) zeroes that time on every call ([engine/gameplay/input.md, "The write API"](../../engine/gameplay/input.md#the-write-api)). The hold tracker keeps each held action's time itself and writes the effector's whole state: a click with no time on the first tick of a press, a press with the time so far on every held tick, and a release with the final time on the tick after, which the next poll turns idle. An analog action above a fixed value also reads as pressed.

Each write resolves its action through the lookup the game's readers use, and values are only raised, so a physical device's reading in the same effector is never reduced. An action no device binds resolves to the null effector, and the write is skipped and reported. The writes go to the player action map through its blockers; to the player map ignoring the blockers, for the pause and commlink toggles, which the game reads that way while a menu is up; and to the UI action map, which the menus read. The pause and commlink clicks go to both.

A write must not outlive the bindings' intent, and the poll cannot be relied on to end it. While [`InputDeviceManager::m_Enabled`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_device_manager::InputDeviceManager##field-m_Enabled) is clear (which is how the debug panel takes the game's input) the poll reads no devices and every effector keeps its last state, so a click left there reads as a fresh click every frame: the pause click alone opens and closes the pause menu in a loop. The device-manager detour passes the flag to the input tick, which stands down while it is clear. So the tracker remembers every effector it wrote on the last tick, and every global stand-down undoes them: with the poll running, each hold is released with its final time; with the poll disabled, every written effector is returned to idle, not released, because nothing moves a released effector on. The panel clears the flag between one poll and the next, so the tick that opens it stands down for the poll that follows; otherwise the move stick's raised values would wait for a poll that reads no devices, and Rico would keep walking under the panel.

A bare click is also undone on the next tick when it is not clicked again, because an effector still clicked a tick later is one the poll skipped (as it does when real time has not advanced). The tracker releases only the effector its click landed on, and only while it is still clicked, so a fresh physical click on another device is left alone. The undoing writes ignore the map's blockers, so a menu that blocked the map since cannot hide the effector. Input/bindings > Devices and thresholds lists the tick's writes.

## Stand-down and recovery

While a game menu or the map is up, the weapon and vehicle handling pause instead of standing down: a held foregrip, a primed grenade or charge, a round in hand, and a held physical control are still held when the menu closes. A snapshot sampled from the previous action set is not interpreted through the new mode's bindings; injection waits for a matching sample.

The global stand-downs (focus loss, no fresh input frame, a scheme change, the panel, a disabled poll) clear shared interaction state and disarm both hands, and each hand then rearms independently once it is neutral. Loss of a usable grip pose or of controller activity masks only that hand until its controls return to neutral; the other hand keeps its input and holds. Valid inferred poses still drive the hands, but physical gestures need tracked poses.

Neutral means nothing the bindings read from the hand would act: no button but a capacitive touch is down, the trigger is under its release and its deadzone, the grip's press reading is under its release, and the stick is under the smallest deadzone of its consumers (for the dominant hand also the turn threshold, where a snap rearms). A stick under the gameplay tuning's `bindings.stick_neutral_floor` always counts as neutral, so a worn stick's drift cannot keep a hand masked. Input/bindings > Devices and thresholds shows each hand as ready, not injecting, or masked with the control that holds it, and a hand masked for over a second is logged.

## Gamepad semantics

The game applies the rules of the input device it considers active, from [`SettingType::EPCUseGamepad`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/settings_manager::SettingType##variant-EPCUseGamepad): aim assist only on the gamepad, a different melee trigger on the keyboard, and the matching prompts. The game sets it from the frame's device events, which action-level injection does not produce, so it would stay at whatever device the player last touched. The bindings are written against the gamepad rules, so while the controller scheme is in effect the entry is held at the gamepad's value, written directly rather than through the setter, which would re-select the controller port and may persist the change. The previous value is restored when the pin ends, unless a real device event has written its own meanwhile.

## Unverified

Unchecked in the headset: that the runtime's digital edges of a float action arrive in step with the raw grip's and trigger's, so a grab latched from the raw grip and the grip action it masks agree on every press; that every runtime's bound sources name the components the masks match, through wineopenxr as well as natively, or else are all opaque; that every menu the controllers reach takes its stick and tab input through the base menu input (the map and the commlink have their own handling); and that the delay the chord windows add to a lone stick click goes unnoticed.
