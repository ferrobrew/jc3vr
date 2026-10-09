# Controls summary

Under the controller scheme the game's own button hints are wrong: the mod pins the game to its gamepad rules, so the HUD prompts show the gamepad glyph of each action while the controllers drive that action with something else. The mod hides those hints and puts a panel on the off-hand wrist that lists what every controller input does in the current mode, with the game's own names for its actions and the Valve Index button glyphs. The code is `payload/src/controllers/summary/` and `payload/src/hud/summary_panel.rs`; the settings are `controllers.summary`. The engine side is in [button-hints-and-localisation.md](../../engine/button-hints-and-localisation.md).

## Hiding the game's hints

The HUD's two hint stages, the bottom-right stage of context and systemic hints and the centre stage of the big hint, join the clip groups the capture-seam detour holds invisible (`HINT_CLIPS` in `hud/scaleform.rs`, beside the issue #8 overlays and the reticle groups). They are hidden while `controllers.summary.hide_game_hints` is on and the controller scheme is in effect, with the same intent tracking as the reticles, so the game's own visibility comes back when the scheme ends. Nothing in the hint pipeline is hooked: the game keeps queueing and ageing its hints, so its cooldowns and mission scripts behave as they would.

The use-item and weapon-pickup prompt, the skip-cutscene prompt, HUD notifications, and tutorials still show gamepad glyphs.

## One table, two readers

The binding tables (`controllers/bindings/tables/`) are declared once over a semantic vocabulary (`sink.rs`): a `Press` names a semantic action of the mode's set (or a pad zone of one, or a latched menu stick direction) with a gate (the sight chord's withholds, the wingsuit's evade flick, and the menu's grip modifier), and an `Axis` names a vector action's axis or a float action's value. `declare.rs` hands each mode's declarations to a `Sink`. The injection's `EvalSink` resolves them against the tick's semantic actions. The `DescribeSink` records a row per input instead: the physical control each action is bound to (less what the table context masks, such as the pointer hand's trigger), the gesture, and the actions it drives. `describe.rs` adds what the mod handles outside the tables (the pause action's press and long press, the body turn, the seated-crouch flick, the sight chord, the stick click recenter chord, the debug panel chord, the yank on the grapple action's row, the grapple button's double tap that releases the tethers and its hold that tightens them, and, where the hand weapons are in use, the holsters, the foregrip, the reload's pouch, belt reload, flick eject, and action, and the grenade pouch; the wingsuit's arm steering; and the physical controls a driver holds, the wheel, the handlebars, or the cockpit's stick and lever; each only when its feature is on) and merges the rows that share an input.

## Glyphs from the bound sources

The controls are the ones the runtime reports, so a player's rebinding in the runtime's binding interface shows on the wrist. The game thread enumerates each semantic action's bound sources (`xrEnumerateBoundSourcesForAction`) and names each with `xrGetInputSourceLocalizedName`, with the user path and the component, inside the frame's input update, and publishes them for the summary as a table (`vr/input/sources.rs`); `vr/input/semantic/bound.rs` matches each source path, as text, against the components the mod knows on the shipped profiles, at the granularity of the glyphs: a stick's axis apart from the stick and its click, and a trackpad's touch and press apart from its surface. A recognised source is drawn with its glyph as before. A source the mod does not recognise (a runtime-specific input, or one under no hand) is drawn with the runtime's localized name, in amber, in place of the glyph, under the hand whose user path it is under. An action bound to nothing is listed in one "UNBOUND" row of either hand, last; on the shipped defaults that row holds `throw_grenade`, `release_tethers` (the grapple's double tap releases the tethers), and the grapple's analog `fire_grapple` (the grapple is on a button). An action whose sources have not been read yet, or whose enumeration failed, keeps its default binding. A context mask covers a recognised source by its input, and one it does not recognise only when the mask takes the whole hand, as for the bindings.

The table is read again in full after the interaction profiles change and on the frame after each rebuild of the summary, per set the first time the set is active, and the active sets once a second while the panel shows, because a rebind within one profile raises no event. The table carries a generation that moves only when its contents change, and the summary rebuilds when the generation does, so a refresh that finds the same bindings costs a comparison and nothing else.

Two tests keep the description honest. One evaluates every mode under every table context (handedness, dual-wield, weapon cycling, the menu pointer's hand) with every input pressed with the grips held and without, each stick pushed every way, each trackpad quadrant pressed, each stick click held alone past the chord window, an evade flick and none, and both system presses, and requires every injected action to appear in the description. The other requires every injected action to have a label.

## Labels

Each game action is labelled by the `buttonhint_*` key the game's own button-mapping table gives it in that context (`labels.rs`), resolved at runtime through [`Locator::GetLocalizedStringRaw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::locator::Locator##func-GetLocalizedStringRaw) on the input tick, so the summary reads in the game's language with the game's words. A label can carry a hold overload, prefixed with the game's own "HOLD": reload and holster, turbo jump and nitrous, the gunner seat, and the dominant stick click's detonation, which has no tap at all because the game detonates only once the action is held. The gamepad-button mirrors and the actions another label on the same input covers are hidden per mode, so an A press on foot reads "Jump" rather than every action the virtual pad button carries. Labels that read the same, ignoring case, appear once. The section heading is the controls screen's tab name for the mode.

The mod carries no game string and no hash: only the key names. A key the game does not have falls back to an English stand-in made from the key's name. The few things the game has no string for (the mod's gestures, among them the C4 pouch's grab and the wrist detonator's push, the weapon slots of the trackpad select, the falling and parachute headings, the gesture prefixes) are in one English table, `ModText`.

The summary is rebuilt only when the mode, the table context, the enabled gestures, the language, or the bound sources change.

## The panel

The panel is the third egui-to-texture panel, after the HUD and the debug panel. It has its own egui context and renderer, its own render target, and no input, and it draws through the shared quad pass on both eyes from corners cached on eye 0. It lies on the back of the off-hand's wrist, offset from the controller's grip pose (`placement.offset_m`, `placement.width_m`), and faces out of the back of the hand like a watch; with the forearm across the body its text runs left to right for either hand.

It shows while any of these holds, and fades in and out over `fade_s`:

- a glance: the panel is within `glance.view_cone_deg` of the head's forward and its face is within `glance.facing_deg` of the head, held for `glance.dwell_s`; both angles widen by `glance.hysteresis_deg` once it shows, and it lingers for `glance.linger_s` after the look ends;
- the off-hand's trackpad is touched (Index controllers only) for a fifth of a second without a press; a press during the touch, which pages the menu tabs, cancels the pin until the thumb lifts, and the delay keeps a touch that is about to press from flashing the panel;
- the mode changed within `show_on_mode_change_s`. Only a mode the player settles in counts, and only when its heading differs from the last such mode's: falling is passed through on every jump, ledge drop, and grapple launch, so it neither shows the panel nor counts as the mode left, and a jump that lands shows nothing while a fall that opens the wingsuit or the parachute shows that mode. Each change that shows the panel is logged at info (`controls summary: the mode changed`).

The rows are grouped under a controller glyph: the off-hand, then the dominant hand, then the inputs of either hand. A face button, stick, or trackpad row that both hands share (in the menus, the confirm and back buttons, the stick's navigation and its grip-held tab steps, and the trackpad's tab paging on the Index, prefixed with the half pressed) is folded into one row of either hand, drawn with the hand-less glyph where there is one (the face buttons and the trackpad) and with both hands' glyphs otherwise. The two chords are rows of either hand too: both stick clicks, held, and the left stick click with the right B. Each row shows the input's glyph (or the runtime's name for a source without one), the gesture's prefix in grey, and the label.

The panel is as tall as its content. Its texture is `width_px` wide, and each time the summary changes it is laid out once, unseen, at rows `row_px` tall with every wrapped label measured, and the texture takes the measured height; the panel's height on the wrist follows from the texture's aspect and `placement.width_m`. A summary taller than `max_height_px` at that row height is laid out again with its rows shrunk in proportion to fit, down to a readable minimum, below which it runs off the bottom. The Controllers tab shows the texture's size and the row height in use.

## Assets

The glyphs are from Kenney's Input Prompts 1.5 (CC0), in `payload/assets/glyphs/kenney-input-prompts/` with the pack's licence: the Valve Index set, plus the grip glyphs from the same pack's Steam Frame set, since the Index set has no grip glyph. Touch controllers get the Index glyphs too. The font is Libre Franklin (SIL Open Font License 1.1), regular and semibold, in `payload/assets/fonts/libre-franklin/` with its licence; egui's bundled fonts follow it as fallbacks and cover Cyrillic. Both are embedded in the payload.

Japanese and Arabic are not drawn: egui has no CJK font here and does not shape Arabic, so in those languages every game string falls back to its English stand-in. Adding a CJK font (loaded only for Japanese) would fix the first; Arabic would need a shaping renderer or the game's own Scaleform fonts.

## Settings

`controllers.summary`: `enabled` and `hide_game_hints` (both on, both acting only under the controller scheme, and both on the pause menu's `VR: CONTROLS` page), `show_on_glance`, `glance`, `show_on_trackpad_touch`, `show_on_mode_change_s`, `placement`, `width_px`, `row_px`, `max_height_px`, and `fade_s`, all on the Controllers debug tab, which also shows the current heading, the row count, the language, and why the panel is up.

## Status

Built and unit-tested (the describer, on the defaults and on the runtime's sources, the source matching, the labels, the layout model, the glance, the fade, and the fallback); not yet tried in the headset. Unknowns to check in game:

1. Whether the two hint-stage paths resolve in the clip registry (the scaleform probe logs it) and stay hidden through cinematics and missions.
2. Whether the prompts that keep their gamepad glyphs (use-item, skip cutscene, notifications, tutorials) need hiding too.
3. Whether the localisation lookup is sound from the input tick, and whether a language change in the options menu reaches the summary (it should, through the language field).
4. Whether any game string reads wrong for the controllers in its context (the gunner seat's key reads "Turret Position"; `TOGGLE_MAGNET_1` is labelled in every vehicle, as the game's generic vehicle section has it).
5. Whether the glance angles and the wrist offset are comfortable on both the Index and Touch controllers, and whether the panel's text is sharp enough at its size and resolution, and whether a long summary's panel is comfortable to read at its height on the wrist.
6. Whether the panel's placement and text direction are right for a left-handed player (off-hand on the right).
7. Whether the parachute heading's A row is right: the summary hides the open-parachute label under the parachute.
8. Whether wineopenxr passes the bound-source and localized-name calls through, and whether a rebind in SteamVR's binding interface reaches the panel within a second of glancing at it.
