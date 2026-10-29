# HUD depth follow-up

The HUD atlas is the only HUD depth path. The render-root split and the single-panel marker warp are deleted (see [debug-ui-prune.md](debug-ui-prune.md)). The split blocked the UI on the first pause, and the warp placed its depth bubbles vertically off their markers, so neither served as a fallback. With the atlas on, the crosshair and the centre notices draw on the flat panel at panel distance. No code reads the aim depth for them.

## Placement rule

A HUD element that describes one specific world object receives depth: an atlas cell at the object's depth, a placement on the hands or body, or a placement at aim depth. A general element that describes no single object stays on the panel. A group that mixes both kinds is split per child.

## Placement by element

| Element | Describes one world object | Placement | State |
|---|---|---|---|
| World markers | Yes | Atlas cell at the marker's depth | Done |
| Ammunition, heat, tether, weapon selection cross | Yes, the held weapon | On the hands | Done |
| Health | Yes, the player's body | Wrist bar; the game's vignette is suppressed | Done |
| Weapon and grapple reticles when the gaze aims | Yes, the aimed point | Atlas cell at aim depth | To do |
| Hit marker (`MCI_inflict_damage`) | Yes, the struck object | Atlas cell at aim depth; removed from the suppressed overlay clips | To do |
| Grapple reticle group (`MCI_grapple_reticle_group`) | Yes, the aim and attach points | At the depth of the projected point the native side passes to `UpdateGrappleReticle` | To do |
| Centre pickup counters, out of ammo, too far, planted-explosives meter, wingsuit notifications | No | Panel | Stays |
| Directional damage indicators (`hud_code.CharacterHitIndicator`) | No, they give the attacker's direction only | Panel | Stays |
| Grenade readout | Yes, the grenades on the body | Off-hand wrist or the grenade pouch, matching the ammunition readout | To do |
| Generic centre notices | No | Panel | Stays |
| Objectives, timers, notifications, and the chaos feed | No | Panel | Stays |
| Edge arrows for off-screen markers | No, they give direction only | Panel | Stays |
| Subtitles and the cutscene skip prompt | No | Panel | Stays |

The hand-aimed reticles are hidden under the controller scheme (`overlay.hide_game_reticle`), and the world overlay draws a depth-tested gun dot or laser and the grapple beam instead. The aim-depth cell therefore matters for the gamepad scheme and for driver and mounted weapons with hand aim off.

## Order of work

1. Build one atlas part placed at `hud::aim::current` depth for the weapon reticle, the grapple reticle, and the hit marker. Their clip paths are known in `payload/src/hud/scaleform.rs`. The aim recording in `payload/src/hooks/ui/mod.rs` and `hud/aim.rs` remain after the warp's deletion for this purpose.
2. Move the grenade readout. Its parent group, `MCI_safe_area_bottom_right`, already goes through the atlas, so the work is a child path, a part request and cell in `payload/src/hud/atlas/layout.rs`, and an anchor.
3. Split the centre group: the reticles and the hit marker move to depth, and the remaining children stay on the panel.
4. Correct the overlay suppression. `OVERLAY_CLIPS` in `payload/src/hud/scaleform.rs` names `MCI_vehicle_damage_indicators` and `MCI_character_damage_indicators`, which are empty stages. The game adds its hit indicators to the HUD document beside the point-of-interest pool, so the suppression does not reach them.

## Centre group contents

The static clip tree of `ui/hud.gfx` (patch copy, read with the vendored decompiler) gives the children of `MCI_safe_area_center`:

- The full-screen overlays `MCI_omni_damage`, `MCI_drowning_container`, `MCI_health_damage_manager`, and `MCI_sniper`.
- `MCI_reticles`, which holds the explosive pointer clips, `MCI_weapon_reticle_group`, and `MCI_grapple_reticle_group`.
- `MCI_beacon_helper`, the pickup counters `MCI_beacon_pickup`, `MCI_flare_pickup`, and `MCI_grenade_pickup`, `MCI_inflict_damage`, `MCI_out_of_ammo`, `MCI_too_far`, `MCI_doge`, `MCI_planted_explosives_meter`, `MCI_button_hints_stage.MCI_big_button_hint`, and `MCI_wingsuit_notifications`.
- `MCI_mech_reticle`, a sibling of `MCI_reticles`, and the empty stages `MCI_vehicle_damage_indicators` and `MCI_character_damage_indicators`.

Only the grapple reticle group takes a position from the native side. The weapon reticles and `MCI_inflict_damage` stay at the screen centre, so the hit marker marks the crosshair rather than the struck point. The explosive pointer stays at the centre and receives only a rotation toward the projected explosive. The movie creates the point-of-interest clips, the directional hit indicators, the dual tether, the subscore entries, and the target tracker entries at runtime with `new` and `addChild`. The native-side positioning was read from the debug decompile and is unverified against the release IDB.
