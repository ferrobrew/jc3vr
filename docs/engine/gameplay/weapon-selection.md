# Weapon selection

How the player picks a weapon: the slot-select and cycle actions, how the inventory turns them into a selection and a switch, and the selection cross the HUD shows while a select action is held. The game has no radial wheel steered by a stick and no slowdown while selecting: the cross is four arms that appear while a slot-select button is held, highlight the held slot, and commit on release. The default buttons are in [input.md](input.md#the-default-gamepad-keymap).

None of the native functions below except [`Inventory::UpdateWeaponHandling`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-UpdateWeaponHandling) is bound in pyxis yet; they are verified in the IDB and named here without numbers. The movie side was read from `ui/hud.gfx`'s ActionScript.

## The actions

| Action | Gamepad | Keyboard |
|---|---|---|
| [`SELECT_DUEL_WIELD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-SELECT_DUEL_WIELD) | d-pad up | 1 |
| [`SELECT_TWO_HANDED`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-SELECT_TWO_HANDED) | d-pad right | 2 |
| [`SELECT_TWO_HANDED_SPECIAL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-SELECT_TWO_HANDED_SPECIAL) | d-pad left | 3 |
| [`SELECT_EXPLOSIVES`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-SELECT_EXPLOSIVES) | d-pad down | Caps Lock |
| [`NEXT_WEAPON`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-NEXT_WEAPON) / [`PREV_WEAPON`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-PREV_WEAPON) | none | mouse wheel up / down |

`CCharacter::UpdatePlayerGlobalInput`, called from the character's pre-state-machine parallel pass, reads them for the local player when no blackboard value blocks it and while `CInventory::GetAnimationAllowsSwitchWeaponGUI` holds (an animation-latched inventory permission with one character state bit clear). It folds them into [`Character::m_WeaponInputs`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_WeaponInputs) by taking the maximum and clamping: the four slot-select actions as float values from the action map, and the two cycle actions as pressed states. Which entries of `m_WeaponInputs` hold them is not yet documented on the field.

The `NInventorySelect` state tasks read no input. Each queues its configured act when the game time since a per-character timestamp falls inside its configured window (unverified).

## The decision

[`Inventory::UpdateWeaponHandling`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-UpdateWeaponHandling) acts on the inputs every frame.

- The slot-select value that is largest this frame wins. Ties prefer two-handed, then dual-wield, special, and explosives. A slot is a candidate only when its [`m_Weapons`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##field-m_Weapons) entry holds a weapon. The planted explosive also needs the planted-explosives ability, the animation permission, and a state bit clear, and with keyboard semantics its button has to be released once first (`m_CanSelectExplosiveOnRelease`).
- While any select input is non-zero, the HUD is told to show the cross with the winning slot selected and which of the four slots are valid, and the inventory records the winner as the slot to select on release, unless the winner is the slot already wielded, in which case it records none. The record follows the input every frame, so pressing a different select action while still holding retargets the selection.
- When all four select inputs are zero and a slot is pending, the inventory commits: `CCharacter::SwitchWeapon` when the animation allows switching and the slot differs from the wielded one (or the planted explosive is in planting mode), or the selected slot is set and [`Character::DrawWeapon`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-DrawWeapon) is called when nothing is wielded and drawing is allowed. Otherwise the slot stays pending and is retried every frame, and the HUD shows only that slot until a timeout.
- With keyboard semantics a planted-explosive selection commits while still held; with gamepad semantics it waits for the release like the others. The semantics come from the [`EPCUseGamepad`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/settings_manager::SettingType##variant-EPCUseGamepad) setting.
- `NEXT_WEAPON` and `PREV_WEAPON` step through dual-wield, two-handed, and special (not explosives), skipping empty slots, queue the result, show the cross with it highlighted, and commit a short fixed delay after the last step. Pressing both cancels the step.
- A holster press sets a latch that hides the cross and drops the pending slot until every select input is released; it also starts a holster.
- The cross is not shown in a vehicle seat, except on a motorcycle.

## The selection cross

`MCI_weapon_selection_wheel` is a static clip on `hud.gfx`'s main timeline (listed on [`ScaleformInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::ScaleformInfo)). Its arms are `MCI_dual`, `MCI_two_handed`, `MCI_two_handed_special`, and `MCI_explosives`, each with `MCI_weapon_icon`, `TXT_name`, `MCI_arrows`, and a direction-specific background, and the movie maps the inventory's slots onto them (`WeaponSlotToWheelSlot`): [`E_SLOT_DUAL_WIELD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::WeaponSlot##variant-E_SLOT_DUAL_WIELD), [`E_SLOT_TWO_HANDED`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::WeaponSlot##variant-E_SLOT_TWO_HANDED), [`E_SLOT_TWO_HANDED_SPECIAL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::WeaponSlot##variant-E_SLOT_TWO_HANDED_SPECIAL), and [`E_SLOT_PLANTED_EXPLOSIVE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::WeaponSlot##variant-E_SLOT_PLANTED_EXPLOSIVE). The heavy, hand-grenade, FOW, and beacon slots are not on it.

With gamepad semantics (the movie's `m_IsUsingGamepad`, fed by the `OnUseGamepad` call described on [`UIManager::m_IsUsingGamepad`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_IsUsingGamepad)) the arms tween out from the centre into the d-pad cross: dual-wield up, two-handed right, special left, explosives down. With keyboard semantics they lie in one horizontal row. A `MCI_selection_arrow` child and its angle, scale, and radius fields are hidden and unused, the remains of an analog radial selector.

Each arm shows the weapon's icon, a streamed texture `textures/ui/weapon_icons/<weapon id>.ddsc` loaded through `hud_code.weapon_icon_type.LoadIcon`, and the weapon's name. The cross shows no ammunition; the always-present weapon display is the bottom-right block `MCI_safe_area_bottom_right.MCI_weapons_parent.MCI_weapons` (the icon's `_sm` variant, the name, a magazine bar, and the reserve), which fades out on a timer.

The native side calls four ActionScript methods:

- `InitWeapons(weapon_data)`: the weapon id and name for each slot.
- `OnPickupWeapon(id, slot, name)` and `OnSelectWeapon(slot, ...)`.
- `UpdateWeaponSelection(selected_slot, should_show, valid_dual, valid_two_handed, valid_special, valid_explosives)`. The arms' visibility follows the valid flags. A show edge tweens the arms out at full alpha; while shown, the selected arm is at full alpha and the others are dimmed, and each change of selection plays `jc3_gui_hud_weapon_switch`; a hide edge scales the selected arm up while fading it and retracts the rest.

`CHUDUI::UpdateWeaponSelection`, whose only caller is `UpdateWeaponHandling`, packs a selection-parameters struct (whether to show, the selected slot, and the four valid flags) into six values and invokes the AS3 method, only when a field has changed. It is the single point through which all of the cross's state passes.

The movie's `GetWeaponSelectionWheelBB{Left,Right,Top,Bottom}` give the cross's extent for the current layout; `CHUDUI::RecalculateWheelBB` reads them on `InitWeapons` and on a change of gamepad use.

## Placement

`CHUDUI::UpdateWeaponSelectionPlacement`, called every frame from [`HUDUI::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::hud_ui::HUDUI##func-Update), takes the hips bone's world position and passes it to `CUIMenu::UpdateHysteresis`. That projects it with [`UIManager::Convert3DCoords`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-Convert3DCoords) through the render camera's view-projection directly, not through [`Get2DInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-Get2DInfo) or [`Convert3DCoordsDefault`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##func-Convert3DCoordsDefault), clamps the result so the cross's extent stays on screen, averages it with the last few positions, and writes the clip's x and y with a [display-info write](../rendering/ui-rendering.md#display-info-writes). The cross therefore sits over the character's hips, smoothed.
