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
| Pickup and interaction prompts | Yes, the object | At the object's depth if the game projects them from a world position, otherwise at aim depth | To do, needs the clip names |
| Grenade readout | Yes, the grenades on the body | Off-hand wrist or the grenade pouch, matching the ammunition readout | To do |
| Generic centre notices | No | Panel | Stays |
| Objectives, timers, notifications, and the chaos feed | No | Panel | Stays |
| Edge arrows for off-screen markers | No, they give direction only | Panel | Stays |
| Subtitles and the cutscene skip prompt | No | Panel | Stays |

The hand-aimed reticles are hidden under the controller scheme (`overlay.hide_game_reticle`), and the world overlay draws a depth-tested gun dot or laser and the grapple beam instead. The aim-depth cell therefore matters for the gamepad scheme and for driver and mounted weapons with hand aim off.

## Order of work

1. Build one atlas part placed at `hud::aim::current` depth for the weapon reticle, the grapple reticle, and the hit marker. Their clip paths are known in `payload/src/hud/scaleform.rs`. The aim recording in `payload/src/hooks/ui/mod.rs` and `hud/aim.rs` remain after the warp's deletion for this purpose.
2. Move the grenade readout. Its parent group, `MCI_safe_area_bottom_right`, already goes through the atlas, so the work is a child path, a part request and cell in `payload/src/hud/atlas/layout.rs`, and an anchor.
3. Capture the HUD display tree with the HUD tab's dump in the game, so that the children of `MCI_safe_area_center` and the other safe-area groups have names and bounds. The repository names only the children of the centre and bottom-right groups, and the contents of the other groups are unverified.
4. Split the centre group: prompts tied to an object move to depth, and generic notices stay on the panel.
