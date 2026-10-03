# Weapon selector

Status: a plan; not built.

A hand-steered weapon selector in the style of Half-Life: Alyx, built on the game's own selection cross. The game's selection logic and cross are in [weapon-selection.md](../engine/gameplay/weapon-selection.md): four float slot-select actions, the largest held one wins, the selection follows the input while held, and the release commits. That is exactly what the binding injection already produces (`Want::Hold` and the hold and inject steps in `payload/src/controllers/bindings/`), so the selector needs no synthetic stick: the mod maps the hand's offset to one of four slots and holds that slot's select action.

## Phase 1: the game's cross on the panel, steered by the hand

- Open: a press of the selector button past a hold threshold starts holding the select action of the wielded slot, so the cross appears at once and an immediate release changes nothing.
- Steer: the hand's position at the press is recorded in a frame facing the head, and each tick the hand's offset in the plane facing the head picks one of four 90-degree sectors centred on up (dual-wield), right (two-handed), left (special), and down (explosives), matching the gamepad layout the cross uses under the gamepad-semantics pin ([controllers.md](input/controllers.md#gamepad-semantics)). The new select action is held and the old one dropped in the same write, so no tick has all four at zero, which would commit.
- Deadzone: inside a small radius (around 4 to 5 cm to start) the wielded slot is held, which the game records as none and so cancels on release. Sectors switch only a few degrees past a boundary, and the deadzone is re-entered at a smaller radius than it is left. A sector whose slot is empty snaps to the wielded slot too, since its arm is invisible and the highlight would otherwise vanish. Slot validity comes from [`Inventory::m_Weapons`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##field-m_Weapons); the planted explosive's validity depends on more than its slot, so a detour on `CHUDUI::UpdateWeaponSelection` is the simplest source of the valid flags.
- Commit: releasing the button stops every select hold; the game switches or draws on its next update, or retries while the animation forbids it.
- Cancel: releasing in the deadzone or on the wielded weapon. With nothing wielded every valid arm draws, so there is no clean cancel; a deadzone release could draw the last weapon instead.
- Gates: no select action is held while the game's switch permission is closed, or a later opening would see a held input, and every hold drops on a mode change (a vehicle, a menu).
- Time and haptics: time does not slow by default, as in Alyx; an optional slowdown through `payload/src/director/time.rs` would also slow the switch animation and must not fight the director's setting. A haptic tick marks each sector change and the commit.
- Placement: the engine places the cross over the hips through the raw camera, which in VR lands it at the panel's bottom edge, away from the hand. After `CHUDUI::UpdateWeaponSelectionPlacement` writes its position, the mod intersects the head-to-hand ray at the press with the panel (`intersect_panel` in `payload/src/hud/pointer/panel.rs`), converts the hit to stage coordinates, clamps with the cross's extent, and writes the clip's position, so the cross appears on the panel behind the hand. This needs `UpdateWeaponSelectionPlacement` bound in pyxis.
- Code: a pure state machine in `controllers/weapons/wheel.rs` (press origin, sectors with deadzone and hysteresis, validity and snapping, gates, and the select action to hold this tick), feeding its `Desired` entries to the hold resolver (`payload/src/controllers/bindings/hold.rs`), with `test-case` tests for opening, retargeting without a zero tick, deadzone release, an empty arm, a gate closing mid-hold, and a mode change mid-hold.
- pyxis: `CCharacter::UpdatePlayerGlobalInput`, `CInventory::GetAnimationAllowsSwitchWeaponGUI`, `CHUDUI::UpdateWeaponSelection` with its parameter struct, `UpdateWeaponSelectionPlacement`, `RecalculateWheelBB`, the inventory's selection fields, and the indices of [`Character::m_WeaponInputs`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_WeaponInputs).

## Phase 2: the cross at the hand

The cross moves off the panel onto a quad facing the head at the press point, or just in front of the off hand, roughly 25 to 30 cm across. The first route is a crop: while the cross is shown, it is forced to a fixed stage position (the stage centre, mostly empty now that hand aim hides the game reticle), the panel discards that rectangle, and the hand quad samples it. Its limits are the crop's in [ui-decoupling.md](rendering/ui-decoupling.md#techniques): whatever else the game draws there comes along, and sharpness is bounded by the HUD texture. The atlas proposed there removes both, and the cross is one of its easiest parts: static, never pooled, with no world-projected children. Phase 1's placement stays as the fallback with the crop disabled.

## Fallback

If the game's cross proves unusable in the headset, for example because retargeting while held or the release timing behaves differently from the reading of the code, the mod hides `MCI_weapon_selection_wheel` with the existing clip-visibility machinery and draws its own four arms at the hand in the world-space overlay, still driven by the same select-action injection so the game's selection, validity, and commit logic stay in charge. The overlay draws only untextured shapes and lines today, so this needs a textured sprite primitive, the weapon icons from `textures/ui/weapon_icons/` (loaded through the engine's texture loader, or extracted and converted once), the weapon ids, and a font path for the labels. It is the most controllable route and the most new code.

## Bindings

The selector button is the off-hand stick click, which today cycles to the next weapon on foot ([controllers.md](input/controllers.md#holsters-grab-volumes-the-foregrip-and-the-action-runs-tier)). It becomes tap-or-hold: a tap clicks `NEXT_WEAPON` on release, so a cycle fires on release instead of on press, and a hold past roughly 0.25 to 0.3 s opens the selector. That needs a press latch like the one `SystemButton` uses. The selector should get its own toggle, independent of the holsters, since it is the main switcher for a player who does not use them.

Open conflict: while the dual-wield split acts, the off-hand stick click holds `RETRACT_GRAPPLE`, because the off trigger fires the second pistol. Retract is a hold, so tap-or-hold cannot share the button. The options are to open the selector there only with no tether attached (which needs a tether count, not yet located) or to give the selector another input under dual-wield. This has to be decided before building. Vehicles stay out of phase 1 (the game shows no cross in a seat except on a motorcycle, where the vehicle table owns the click), and so does the wingsuit, which has no hand weapons.

## In-game unknowns

1. Whether switching the held select action within one tick retargets without a commit; the code says it does, but the effector write timing per tick needs confirming.
2. Where the cross lands on the panel without the placement override, and whether the engine's position averaging makes the override lag.
3. Whether the switch permission is open on the parachute, while swimming or grappling, and on a motorcycle.
4. Whether a held select action survives a pause as a ghost commit on return.
5. Whether `NEXT_WEAPON` moved to the release still steps once.
6. For the crop: what else the game draws in the stage centre during combat, and how legible the crop is at the HUD's resolution.
7. With nothing wielded, whether every valid arm draws on release.
