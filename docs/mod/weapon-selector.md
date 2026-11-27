# Weapon wheel

The weapon wheel opens the game's own selection cross with a held input and lets the hand choose an arm, in the manner of Half-Life: Alyx. It is off by default (`controllers.weapons.wheel.enabled`, in the debug UI's weapons feature): one cross shared by both hands and drawn by the game breaks the artifice of weapons held on the body and drawn from its sockets, which are the main way to draw. The code is `crates/jc3vr-weapons/src/controllers/weapons/wheel/`, and its tuning is the `wheel` section of `crates/jc3vr-weapons/tuning/inventory.toml` ([tuning.md](tuning.md)).

The game's selection logic is in [weapon-selection.md](../engine/gameplay/weapon-selection.md): four float slot-select actions, the largest held one wins, the selection follows the input while held, and the release commits. The wheel produces exactly that input through the binding injection, so the game's own offer, highlight, sound, and commit rules stay in charge.

## Opening

Two inputs open the wheel, and the hand that holds the opener steers it:

- `weapon_select_press` ships unbound, because the off-hand Index trackpad lies under the thumb on its way to the stick click the debug panel's chord presses. When bound, a press in a quadrant starts on that quadrant's arm, so a tap still selects that slot at once as the gamepad's d-pad does, and a press in the centre starts on the centre. The binding tables still declare the quadrant holds (the controls summary describes them from there); the wheel drops them from the tick's wanted actions and holds its own.
- `weapon_wheel`, a float on the `on_foot` set, bound by default to the dominant stick's vertical axis. A push past `wheel.stick_open` opens the wheel, and it stays open until the push falls below `wheel.stick_release`. This is the Touch controller's way in, and it can be rebound to a button.

The dominant stick's up half was the one free stick direction: the off-hand stick moves and its click cycles weapons (and retracts the tether while dual-wielding), the face buttons all carry a game hold, and the dominant stick turns sideways and crouches on its down flick. While the stick holds the wheel open, the body turn reads zero, so sideways drift does not turn the player.

The wheel acts where the weapon handling does (on the ground, falling, and under the parachute), and opens only while the game reads the slot-select inputs ([`Inventory::GetAnimationAllowsSwitchWeaponGUI`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-GetAnimationAllowsSwitchWeaponGUI)). An opener pressed while the game does not read them opens nothing until released.

## Choosing

At the open the steering hand's aim is recorded. Each tick, the controller's turn from there, in the aim's own frame, picks one of four quarters centred on up (the pistols), right (the two-handed weapon), left (the special weapon), and down (the planted explosive), the layout the cross draws under gamepad semantics ([controllers.md](input/bindings.md#gamepad-semantics)). The hand leaves the centre past `wheel.point_leave_deg` and returns inside `wheel.point_return_deg`, and an arm stays chosen until the hand is `wheel.hysteresis_deg` past the line to the next.

An arm whose slot the game does not offer counts as the centre, since the cross draws nothing there. The offer comes from the selection the game last pushed to its HUD ([`HUDUI::m_LastWeaponSelectionParams`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::hud_ui::HUDUI##field-m_LastWeaponSelectionParams)), because the planted explosive's offer depends on more than its slot holding a weapon.

The wheel holds exactly one slot-select action from the open to the close, and switches which one within a single tick, so no tick has all four at zero, which the game reads as a release. In the centre it holds the resting slot: the wielded weapon's arm when offered, which the game records as no change; otherwise the selected slot's arm, or the first offered one. Each change of choice, and the open, plays `Event::WheelTick` on the steering hand ([haptics.md](input/haptics.md)).

There is no explosive sub-selection: the down arm is the planted explosive slot alone; grenades are not on the cross.

## Closing

Releasing the opener on an arm other than the wielded weapon's commits: the game switches or draws on its next update, or keeps the selection pending while the animation forbids a switch. The commit plays `Event::WheelCommit`.

Anything else cancels: a release in the centre or on the wielded weapon's arm, the game's input closing, a change of mode (a vehicle, the wingsuit, swimming), a game menu, or the bindings standing down. A release in the centre with nothing wielded would draw the resting slot, and a menu or a closing input would let the game commit the pending selection once its input reads zero, so a cancel clears [`Inventory::m_SlotToSelectOnRelease`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##field-m_SlotToSelectOnRelease) on that input tick and the next `wheel.cancel_ticks`. The input tick runs before the game's update, so the clear lands before the commit check.

## Placement

The engine places the cross over the character's hips every HUD update ([`HUDUI::UpdateWeaponSelectionPlacement`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::hud_ui::HUDUI##func-UpdateWeaponSelectionPlacement)), projected through the render camera, which in the headset would clamp it to the panel's bottom edge. Instead the open records where the line from the head through the steering hand's grip meets the floating panel, and a post-call detour on the placement writes the cross there (`wheel/placement.rs`), clamped as the engine clamps its own to the safe area ([`HUDUI::m_SelectionWheelBB`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::hud_ui::HUDUI##field-m_SelectionWheelBB), [`UIManager::m_SafeArea`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_SafeArea)). The point lingers for `wheel.linger_ms` after the close so the hide animation plays where the cross was shown.

With the HUD atlas on, the cross is drawn at the hand on a quad of its own ([hud.md](hud.md#hud-depth), [ui-decoupling.md](rendering/ui-decoupling.md#the-atlas-as-built)). The wheel publishes its steering hand while open and through the hide animation (`cross_anchor`); while that lasts, the capture seam moves the cross into an atlas cell sized from `m_SelectionWheelBB`, and the HUD draws it where the hand was at the open, pulled toward the head, facing it, and fading out with the hide animation. The panel placement is written in the HUD update, before the capture-time move, and restored after the capture, so it is what the game sees and the fallback whenever the atlas does not draw the cross.

While the wheel is on, the controls summary lists it on each bound opener where it acts.

## Unverified in game

That switching the held select action within one tick retargets the highlight without a commit; that the placement write does not visibly fight the engine's own; whether the game reads the slot-select inputs on the parachute, falling, and grappling; that the cancel's clear beats the commit in every case, including a release on the same tick as a mode change; and the comfort of the thresholds.
