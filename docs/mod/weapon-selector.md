# Weapon wheel

The weapon wheel opens the game's own selection cross with a held input and lets the hand choose an arm, in the manner of Half-Life: Alyx; the holsters stay the main way to draw. The game's selection logic and cross are in [weapon-selection.md](../engine/gameplay/weapon-selection.md): four float slot-select actions, the largest held one wins, the selection follows the input while held, and the release commits. The wheel produces exactly that input through the binding injection, so the game's own offer, highlight, sound, and commit rules stay in charge and no synthetic stick is needed. The code is `payload/src/controllers/weapons/wheel/`.

## Opening

Two inputs open the wheel, and the hand that holds the opener steers it:

- `weapon_select_press`, the off-hand Index trackpad pressed down. Before the wheel, a press latched to a quadrant held that quadrant's slot-select action until the release, as the gamepad's d-pad does. The wheel keeps that: a press in a quadrant starts on that quadrant's arm, so a tap still selects the slot at once, and a press in the centre starts on the centre. The binding tables still declare the quadrant holds (the controls summary describes them from there), and the wheel drops them from the tick's wanted actions and holds its own.
- `weapon_wheel`, a new float action on the `on_foot` set, bound by default to the dominant stick's vertical axis. Pushing the stick up past `stick_open` opens the wheel, and it stays open until the push falls below `stick_release`. The Touch controller has no trackpad, so this is its way in; on the Index it works too.

The dominant stick's up half was the one free stick direction. The off-hand stick is movement and its click is the weapon cycle (and the tether retract while dual-wielding, which is a hold, so a tap-or-hold could not share it); the face buttons all carry a hold of the game's already; the dominant stick's horizontal axis turns and its down flick is the seated crouch, which reads only the lower half of the axis. The wheel reads only the upper half. While the stick holds the wheel open, the body turn reads zero, so the stick's sideways drift does not turn the player. Being its own action, `weapon_wheel` can be rebound in the runtime, to a button for example: any value past `stick_open` opens it.

The wheel acts where the weapon handling does (on the ground, falling, and under the parachute), and opens only while the game reads the slot-select inputs ([`Inventory::GetAnimationAllowsSwitchWeaponGUI`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-GetAnimationAllowsSwitchWeaponGUI)). An opener pressed while the game does not read them opens nothing until it is released.

## Choosing

At the open, the steering hand's pose is recorded. Each tick its deflection from there picks one of four quarters of the wheel centred on up (the pistols), right (the two-handed weapon), left (the special weapon), and down (the planted explosive), the layout the cross draws under gamepad semantics ([controllers.md](input/controllers.md#gamepad-semantics)). `steer` chooses how the deflection is read:

- `Point` (the default): the turn of the controller's aim from where it pointed at the open, in the aim's own frame (yaw toward the arm's side, pitch toward up or down), in degrees. The hand leaves the centre past `point_leave_deg` and returns to it inside `point_return_deg`.
- `Offset`: the controller's movement in the plane facing the head (the head's level right and the cockpit's up), in metres, with `offset_leave_m` and `offset_return_m`.

An arm stays chosen until the hand is `hysteresis_deg` past the line to the next one. An arm whose slot the game does not offer counts as the centre: the cross draws no arm there, and the game would record nothing for it. The offer comes from the selection the game last pushed to its HUD ([`HUDUI::m_LastWeaponSelectionParams`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::hud_ui::HUDUI##field-m_LastWeaponSelectionParams)), since the planted explosive's offer depends on more than its slot holding a weapon.

The wheel holds exactly one slot-select action from the open to the close, and switches which one within a single tick, so no tick has all four at zero (which the game reads as a release). In the centre it holds the resting slot: the wielded weapon's arm when the game offers it, which the game records as no change; with nothing on the cross wielded, the selected slot's arm, or the first offered one. A trackpad quadrant's arm is chosen in the centre until the hand first leaves it. Each change of choice plays `Event::WheelTick` on the steering hand ([input/haptics.md](input/haptics.md)), as does the open.

## Closing

Releasing the opener stops the hold. On an arm other than the wielded weapon's, the game commits: it switches or draws on its next update, or keeps the selection pending and retries while the animation forbids a switch. The commit plays `Event::WheelCommit` on the steering hand.

Anything else cancels: a release in the centre or on the wielded weapon's arm, the game's input closing while open, a change of mode (a vehicle, the wingsuit, swimming), a game menu, or the bindings standing down. A release in the centre with nothing wielded would draw the resting slot, and a menu or a closing input would let the game commit the pending selection when its input reads zero again, so a cancel clears the game's pending selection ([`Inventory::m_SlotToSelectOnRelease`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##field-m_SlotToSelectOnRelease)) on that input tick and the next few, in case the game read a select input that was still held on its next update. The input tick runs before the game's update, so the clear lands before the commit check.

There is no explosive sub-selection: the cross's down arm is the planted explosive slot alone (the C4); grenades and the other slots are not on the cross.

## Placement

The engine places the cross over the character's hips every HUD update ([`HUDUI::UpdateWeaponSelectionPlacement`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::hud_ui::HUDUI##func-UpdateWeaponSelectionPlacement)), projected through the render camera, which in the headset is below the panel's view, so the cross would sit clamped at the panel's bottom edge. With `place_at_hand`, the open records where the line from the head through the steering hand's grip meets the floating panel (the pointer's panel intersection), and a post-call detour on the placement writes the cross's position there, clamped as the engine clamps its own so the whole cross stays in the safe area ([`HUDUI::m_SelectionWheelBB`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::hud_ui::HUDUI##field-m_SelectionWheelBB), [`UIManager::m_SafeArea`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_SafeArea)). The cross then appears on the panel behind the hand, seen from the head. The point lingers briefly after the close, so the cross's hide animation plays where it was shown. The panel UV maps to stage coordinates through the same stage frame the world markers use.

With the HUD atlas on (`hud.atlas.enabled` and `hud.atlas.cross.enabled`), the cross is drawn at the hand itself, on a quad of its own ([ui-decoupling.md](rendering/ui-decoupling.md#the-atlas-as-built)). The wheel publishes its steering hand while it is open and for the cross's hide animation after it closes (`cross_anchor` in `controllers/weapons/wheel/mod.rs`); while that lasts, the capture seam moves the cross into an atlas cell sized from `m_SelectionWheelBB`, and the HUD draws the cell where the steering hand was when the wheel opened, pulled a little toward the head and held there relative to the head, facing it, and fading out over the hide animation. The wheel's placement write happens in the HUD update, before the capture-time move, and the cross is moved back to it after the capture, so the two compose: the panel placement is what the game sees and the fallback whenever the atlas does not draw the cross.

## Settings

`controllers.weapons.wheel`: `enabled`, `trackpad`, `stick`, `stick_open`, `stick_release`, `steer`, `point_leave_deg`, `point_return_deg`, `offset_leave_m`, `offset_return_m`, `hysteresis_deg`, and `place_at_hand`. The controls summary lists the wheel on each opener ("HOLD" on the trackpad, "HOLD UP" on the stick) where it acts.

## In-game unknowns

1. That switching the held select action within one tick retargets the highlight without a commit, as the code reads.
2. That the placement override does not visibly fight the engine's own averaged write, and that the cross lands behind the hand on the panel (the panel UV to stage mapping assumes the panel's aspect is the HUD texture's).
3. Whether the game reads the slot-select inputs on the parachute and while falling or grappling.
4. That the cancel's clear of the pending selection beats the game's commit in every case, including a release on the same tick as a mode change.
5. That a trackpad force press, which also touches the pad and so can show the controls summary, is acceptable while the wheel is open.
6. The comfort of the default point and offset thresholds and the stick's open threshold.
