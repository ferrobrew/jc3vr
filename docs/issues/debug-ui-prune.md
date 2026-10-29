# Debug UI prune

Status: done. Every page is pruned to the structure below, and a cleanup pass followed: the remap switches live only in the pause menu, the procedural body's switch and the torso follow moved from the pause menu to Body fit, and the off-by-default modes the prune left as constants are deleted. The rest of this page is the plan as it was carried out.

The debug UI holds about 1,180 settings and 90 buttons across 8 pages and 59 features (`payload/src/ui/inventory/catalog.rs`). The 2026-10 reorganisation moved every widget under a new heading and added a parity gate that rejects removals, so the volume and redundancy that motivated the feedback remain. The prune reduces the main window to about 120 controls: every feature keeps one on/off switch, tuning stays only where it is per-user or per-hardware or under an open bug, and settled values become named constants with their configuration fields deleted.

Deleting configuration fields is compatible with existing files. `payload/src/config/load.rs` merges the file over the defaults, skips unknown keys, and logs them in one warning. The next save drops them. A player who changed a now-baked value receives the constant.

## Rules

Each feature shows its switch and any remaining tuning, then a collapsible live readout. The Controls, Live status, and Visualizations sections are removed, along with every placeholder that redirects to another feature. Overlay switches and shared overlay settings live only in the global visualisation drawer (`payload/src/ui/visualizations.rs`). A field that the VR pause menu already exposes is removed from the debug UI. Sub-feature switches collapse into the feature switch: for example, Weapons > Hold has one switch instead of separate switches for either-hand, handover, and steal.

Tuning kept under an open bug: the snatch yank speed and distance, and the brace rebuild thresholds. These return to constants when the bugs in [gameplay-review-followup.md](gameplay-review-followup.md) close.

## Target structure

| Page | Features |
|---|---|
| Status | Status (merges Session, All status, and Active tools; attention rows first) |
| Body | Head and view, Posture, Body fit (merges Root and walls, Torso, and Procedural), Animation and IK, Hand presentation |
| Hands | Grip and fingers (merges Hand tracking and Fingers), Physical hands, Characters and doors, Snatch, Feedback |
| Weapons | Hold, Support, Aim and sights, Reload, Inventory, Explosives, Selection |
| Movement | On foot (merges Locomotion and Swimming), Grapple, Climbing, Vehicles (merges Driving, Vehicle controls, and Mounted), Air (merges Wingsuit and Parachute) |
| Rendering | Runtime, Stereo, Foveation, HUD, Previews and performance |
| Input | Devices and thresholds (merges Devices, Thresholds, Bindings, and Menu panel), Wrist summary, Self-check |
| Tools | Tracking capture, Controller capture, Replay, Director (merges Director safety, Director spawn, and Keyboard), Environment, Video, Presets |
| Experimental | Single-pass stereo, Far field, Post-FX skips (controls kept as they are), and the button that opens the render RE window |

The render RE window is a separate egui window opened from the Experimental page. It holds the render-block type registry toggles, the per-eye skip toggles and skip-pass range, the eye-1 gates, the exposure A/B, the pass captures, the terrain and flicker diagnostics, and the per-eye camera readout. These tools serve SPS and render reverse-engineering and do not belong in the main window.

## Feature dispositions

The per-feature lists come from a widget inventory at `b1e36445`. File paths are relative to `payload/src/ui/`.

Body. Calibration is removed: its controls are placeholders and its readout echoes the pause menu's body dimensions. Head and view (`camera.rs`) keeps the camera switch, freeze mode, and reset to captured pose. Posture (`controllers/roomscale.rs`) keeps the crouch capsule, the crouch-below height, and the seated crouch toggle. Body fit keeps stop-at-walls, the lean switch, and the fit, root-owner, pushback, and torso readouts. The chase gain, feed-forward, and maximum speed fields are reachable only from `payload/src/controllers/body/rig/harness.rs`; the fields, `chase::velocity`, and the harness path that calls it are deleted. Animation and IK keeps `body_ik.enabled` (moved from `vr.rs`), skip arm IK, remap head, and remap hands. The 28 body-IK weights become constants and the old upper-body IK stays; its retirement is separate work. The body observation evidence readout (`camera.rs`) is deleted. Hand presentation (`controllers/hands.rs`) keeps hand IK, late update, and render-rate upper body.

Hands. Grip and fingers keeps rigid grip, the wrist offset position and rotation with their reset, finger curls, and finger source. The per-finger open and closed ranges become constants; per-hardware differences between Index and Quest controllers are a later question. Physical hands keeps palms, hand bodies, push off surfaces, pickup, vault, character reactions, throw scale, and the grip acquisition evidence button. The 47 hand-body constants, the pickup physics block, the shove block, and the grab and collider view settings become constants or move to the drawer. Characters and doors keeps grab characters, hijack, open doors, and tear doors. Snatch keeps the switch, sling, and the yank speed and distance. Feedback keeps gamepad rumble and impact sounds; the per-cue haptic table and mixer constants become constants.

Weapons. Hold keeps one switch, long-gun grab anywhere, and the gun release evidence readout. Support keeps foregrip, recoil, one-handed weight, two-handed weight, separate pistols, and the brace rebuild thresholds. Aim and sights keeps gun aim, grapple aim, scope lenses, sight alignment, stock in shoulder, cheek weld, wall collision, and the sight tool. The per-weapon correction table is edited in both `aim.rs` and `sights.rs`; the sight tool becomes its only editor. Reload keeps flick eject, world magazines, belt reload, and physical action. Inventory is replaced by the inventory view below. Explosives keeps grenade throw, catch, C4 throw and press, and the wrist detonator. Selection keeps auto-switch when empty and the weapon wheel.

Movement. On foot keeps steady view, no turning animations, the neutral head measurement, run speed, smooth swim yaw, and swim turn rate. The movement overrides in `game.rs` become constants. Grapple keeps yank to zip, pull to reel, pump, and anchor hand. Climbing keeps climb, mantle, stunt, and fling scale. Vehicles keeps driver weapons by hand, passenger aim by hand, wheel, handlebars, stick, lever, nitrous button, twist throttle, twist rudder, wheel response, mounted aim by hands, and mounted grip. Air keeps the arm deadzone, full bank, parachute full pull, and brake threshold.

Rendering. Runtime (`vr.rs`) keeps OpenXR enabled, native resolution, mirror, mirror eye, mod-owned back buffer, and direct eye blit. Stereo keeps the stereo switch, FSR, and reload shaders; the image correction toggles in `render/corrections.rs` become constants. Foveation (`render/resolution.rs`) keeps inner, outer, maximum drop, and the debug mask. HUD keeps redirect, quad, world markers, atlas, weapon readout, selection cross, tether readout, wrist health, panel controller pointer, and dynamic distance. Previews and performance keeps preview size, cross-eyed, and the profiler readout. Render diagnostics moves to the render RE window, except the trace dump, frame count, and per-eye screenshots, which move to Tools.

Input. Devices and thresholds keeps controllers enabled, hand tracking, inject bindings, the press and release thresholds, the stick deadzone, and the device, mode, and map readouts. Wrist summary keeps the summary switch, show on glance, and hide the game's button hints. Self-check stays as it is.

Tools. The capture, replay, environment, and video features stay as they are. Recording compression becomes lossless only. The engine experiments in `game.rs` (the interpolation override and the sim-rate experiment) are deleted, and the barrel spawn moves to Director.

## HUD depth paths

Done. The HUD atlas (`hud.atlas.enabled`, on by default) is the only HUD depth path: markers receive their own cells and draw at the depth of the surface they label, and readouts move to the hands. The render-root split (`hud.split`), which wedged the UI on the first pause, and the single-panel marker warp (`hud.marker_warp`), which placed its depth bubbles vertically off their markers, are deleted with their shader, configuration, and UI. The atlas does not place the centre group (crosshair and centre notices) at aim depth; that gap and the work after it are tracked in [hud-depth-followup.md](hud-depth-followup.md). The aim-depth recording stays for the planned aim-depth cell, and HUD Live status shows its current value.

## Inventory view

The loadout sandbox (`payload/src/controllers/loadout/sandbox.rs`, `ui/controllers/loadout.rs`) drives a table of made-up objects and does not read or write the game. It predates the live table and is deleted.

The Inventory feature becomes a read-only view of three sources side by side, refreshed each frame from published snapshots:

1. The game's inventory: each native weapon slot with its weapon, class, and ammunition in the magazine and in reserve, the wielded weapon, the dual-wield state, and grenade and explosive stock.
2. The live loadout table (`payload/src/controllers/loadout/live/`): each hand, each body socket with its docked item, the carry set, the weapons lying in the world that the pins keep, and the safe items.
3. The hands: what each hand holds beyond the table, such as a loose prop, a held magazine, a door or handle grip, a parachute handle, or a climbing hold, with the grip source.

Rows where the game's slots and the table disagree are marked, because reconciliation is supposed to keep them equal. The existing live readout text in `ui/controllers/loadout.rs` describes the table as a shadow of the old weapon handling; the table is now the authoritative writer, and the new view replaces that text. Editing (placing a chosen weapon on a socket or in a hand, setting ammunition) is a follow-up built on the live table's events.

## Parity gate removal

The following are deleted: `payload/src/ui/inventory/historical/` (the baseline, mapping, and removals JSON, the scripts, the tests, and the README), `payload/src/ui/inventory.rs`, `inventory/sources.rs`, and `inventory/tests.rs`. `inventory/catalog.rs` moves to `ui/catalog.rs`. The historical-JSON tests in `visualizations/tests.rs` are deleted. The section-placement tests in `ui/dispatch/*_tests.rs` are replaced by one route-coverage test, and the section cases in `navigation/tests.rs` and `overview/tests.rs` are removed. `layout.rs` drops the persisted section and the legacy layout migration; old layout files still load because unknown feature keys fall back to Status.

Documentation changes: `docs/mod/debug-ui.md` loses its migration coverage section and its navigation section is rewritten. `docs/mod/settings.md` loses the debug UI verification foundation section. `docs/mod/hud.md` and `docs/mod/testing/recorder.md` are updated for the new page paths.

## Order of work

One commit per page, so each page's field deletions, constants, configuration validation, presets, and tests change together. The parity gate goes first, because it rejects every later removal. The HUD investigation precedes the Rendering page. The inventory view lands with the Weapons page. Formatting, Clippy, and the Wine test suite run at the end.
