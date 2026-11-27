# The VR menu in the pause menu

The VR settings a player reaches for in the headset live inside the game's own pause menu; the rest are in the debug UI ([settings.md](settings.md#where-a-setting-is-exposed)). One `VR` entry in the root menu, directly below `OPTIONS`, opens the game's settings screen with the VR pages as tabs on the screen's own tab bar. Each page is described in Rust and drawn by the pause movie's own tab bar, list, rows, option picker, slider, sounds, and gamepad and mouse handling. The mod ships no Flash and does not patch `pause.gfx`: it reaches the movie through the Scaleform value API from five detours on [`PauseUI`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI) and one on [`UIBase::InvokeImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_base::UIBase##func-InvokeImpl). The movie's structure, the commands it sends, and what code outside it can reach are in [engine/pause-menu.md](../engine/pause-menu.md). The code is `crates/jc3vr-ui/src/pause_menu/`, and the pages are in `crates/jc3vr-ui/src/controllers/`. The feature is always on.

## Route

The movie's folders (an entry that opens a sub-list) are the developer menu's, drawn on plain debug text fields, so they would not look like the shipped menu. The settings screen's tab bar is the shared navigation bar the game's other menus use ([engine doc](../engine/pause-menu.md#the-tab-bar)), with the movie's own selectors, shoulder-button hints, mouse handling, and sounds, so the VR pages are tabs on it. The movie's own code opens the settings screen, and the mod replaces the page the screen is about to read; the game's other pages stay as the game built them. The entry's label is plain text, since a label beginning with `&` or `#` is looked up as a localisation key, and the page registry is capped (`MAX_PAGES` in `pause_menu/pages.rs`) at the tab bar's selector count.

A movie of the mod's own would have to rebuild the widgets, layout, focus, navigation, sounds, and pointer support the settings screen already has ([engine/scaleform.md](../engine/scaleform.md) has what hosting one would take). Patching the shipped `.gfx` files would mean repacking the game's archives, since the injector attaches to a running game and does not control the command line the loose-file mount needs.

## The root entry

The entry is in the root menu the pause menu opens on: the in-game menu, or the front-end menu on the title screen (`model/root.rs` decides from [`PauseUI::m_IsValidMenu`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##field-m_IsValidMenu)). From outside, the movie can only append or prepend to a list, and the engine prepends its own entries and appends a few ([engine doc](../engine/pause-menu.md#menus-and-entries)), so the entry is appended while the movie's `InitMenu` builds the root menu, right after the options entry. In the in-game menu the movie adds the main-menu entry right after the options entry in the same call, and only when the main-menu list is already built. So H6 runs that `InitMenu` with the main-menu list marked invalid, appends the VR entry, runs `InitMenu` again (which now builds the main-menu list alone), and appends a main-menu entry as the movie builds its own (`&pause_main_menu`, of type [`OPTION_MAIN_MENU`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI)). The in-game menu then reads resume, the engine's other entries, `OPTIONS`, `VR`, `MAIN MENU`, and go online. On the title screen the membership link, when shown, is added in the same call as the options entry, so the VR entry follows it. When `InitMenu` could not run that way (it was queued, or the screen was not active), H1 appends the entry to the end of the root menu and logs a warning.

The root menu's selection opens nothing on the settings screen: only the options menu's dispatch does, and the document's menu state is private. So the entry is an event entry ([`OPTION_EVENT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI)) posting `jc3vr.pause_menu.open` (`ENTRY_EVENT`). H3 takes that command while the document is still inside its selection handler (`runtime/root.rs`): it switches the entry's type to the graphics entry's ([`OPTION_GRAPHICS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI)), calls `ShowPauseFromHideCredits`, which puts the document in the options menu's state without laying out another list, and calls `OnSelect`, whose options-menu dispatch opens the settings screen on the graphics page; then it switches the type back. The entry is reached through a member the mod sets on the main-button container (`jc3vr_vr_entry`), since the lists are private. H3 drops the nested selection's second sound, and without a gamepad the opening shows the options menu's back button.

Backing out sends `persist_changes`, after which H3 calls `SwitchToMenu` with the root menu, which lays it out with the VR entry highlighted and puts the document back in the root menu's state. When an opening never reached the populate (the movie refused a call), H3 does the same at once.

## Tabs

The settings screen's own tab switch keeps each tab's scroll position in a private array keyed by page slot, which the native pages share and which cannot be read or cleared from outside ([engine doc](../engine/pause-menu.md#the-settings-screen)); a VR tab switch through it would leave a VR page's position for the native page opened next. So every VR tab is shown in the graphics page's slot, and the mod switches tabs itself and keeps the VR tabs' positions apart (`model/tabs.rs`).

- Opening: the screen sets its tab bar up for the native pages and hides it, so on the next update H4 names one selector per VR page (`SetButtonVariables`), unlocks those and locks the rest (`SetButtonLocked`, which also lays the bar out), sets the bar's select callback to the document's own `Invoke` method, selects the shown tab, and shows the bar.
- Choosing: a selector or shoulder-button hint calls the callback with the tab's index, and `Invoke` sends the index, as text, as a command. H3 recognises the hashes of those names (`TabCommands`, hashed with the engine's command hash), never passes them to the game, and records the tab. A gamepad's shoulder buttons reach the document as `OnNextTabButton` and `OnPrevTabButton`, which it ignores on the settings screen; H6 steps the bar's own cycling for them, which ends in the same callback.
- Switching: when the list is in its root state, the update reads the old tab's position (`GetTopIndex`, `GetSelectedRow`), leaves its page (running its `on_close`), writes the new page's rows into the shown page object and redraws (`RefreshCurrentScreen`, which also rewrites the list title), restores the new tab's saved position fitted to its rows (`DeHighlightCurrent`, then `RestoreSavedState`), and moves the bar's selection. There is no slide animation.
- Leaving: backing out saves the tab's position, hides the bar, restores the row clips' label style, and returns to the root menu. The VR entry reopens on the last tab, at its position, until the menu closes.

The callback stays set after the VR screen closes, until the settings screen next sets its own; the only other caller of the hidden bar is the controls screen's shoulder buttons, whose tab commands H3 drops while no VR tab is shown.

## Section headers

The settings list has no header, separator, or group title row ([engine doc](../engine/pause-menu.md#row-objects-and-widgets)), and an empty row cannot stand between groups because the list's up and down stop at one. The closest native presentation is the list's own title, so a header is a row clip styled as the title: its label in the title's text format (read from `MCI_title` with `getTextFormat` and applied with `setTextFormat` and `defaultTextFormat`, since the list sets the label's text on every redraw), over the separator every row clip draws. The row is a key-binding row with no key text, so selecting it only plays the refusal sound.

The list reuses its clips as it scrolls, so each frame the row pass gives each clip the style of the row it now shows; `HeaderStyles` (`model/headers.rs`) changes a clip only when the kind of row it shows changes. A clip's own format is kept on its row container the first time it is styled and restored when the player leaves the VR screen or a native page is put in place, so the native pages never see a header style.

A header is not selectable: when the highlight lands on one, the update moves it on with the list's `HighlightNext` or `HighlightPrev` in the direction it was going (`skip_header`), or the other way at the top or bottom. A page opening on a header moves to the row below.

## The session

`model/state.rs` holds the state machine and `runtime/session.rs` the shared session. `Closed` has no VR entry in the loaded movie; `EntryAdded { pages }` has it; `Shown { pages, page }` shows the VR tabs with `page` selected. Activation resets the session and adds the entry; opening the VR screen moves to `Shown` on the last tab shown this pause; a tab choice moves to that tab; leaving the screen returns to `EntryAdded`; the menu deactivating or its movie unloading returns to `Closed`.

A page's `on_close` (which cancels a calibration in progress and drops a pending reset confirmation) runs whenever a transition leaves the page: a tab switch, backing out, the menu deactivating (H5), the movie unloading, a stale session at the next activation, a failed reinstall, or a refresh off the capture thread. The pause key, the UI manager forcing the menu closed, and game-state changes deactivate the menu without `persist_changes`, which is why H5 exists. Every path goes through one transition that compares the state before and after (`left_page`), so `on_close` runs once per stay, after the session lock is released.

The session lock is never held across a call into the movie or into an item's functions: a call into the movie can re-enter the command hook synchronously (a widget playing a sound sends an event command), and the lock is not re-entrant. Each hook copies what it needs, releases the lock, works, and writes back; a generation counter tells a write-back whether the page changed in between.

## Hooks

H1 to H4 and H6 run on the UI capture thread: H1 to H3 inside the root movie's advance, H4 in the pause screen's per-frame update, and H6 in its input, whose threads are checked before any value is touched. H5 runs on whichever thread requests the deactivation and never touches the movie.

H1, on [`PauseUI::OnActiveImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-OnActiveImpl), closes any stale session, records the screen as activating for H6, and calls the game, which builds the menus; H6 adds the entry from inside that and records the screen's address. When the session is still closed afterwards, H1 appends the entry to the end of the root menu.

H2, after [`PauseUI::PopulateSettings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-PopulateSettings) (called by the movie's `populate_settings` and by the update's refresh), checks a flag H3 raises around the VR entry's opening. If the VR entry opened the screen, H2 builds the last tab shown this pause, writes a page object `{m_Name, m_Settings}` over the graphics page's slot ([`PAGE_GRAPHICS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::SettingsUI)) of `m_SettingsArray`, and asks the next update to put the VR tabs on the bar. A refresh while a VR tab is shown reinstalls that tab. When a native entry opened the screen, H2 leaves any VR page, hides the tab bar, and restores any header-styled clip. A failed install, or a populate off the capture thread, leaves the native page in the slot, so the session leaves the VR page.

H3, on [`PauseUI::HandleCommandImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-HandleCommandImpl), decodes the command (`model/command.rs`) and routes it:

| Command | Condition | Handling |
|---|---|---|
| `populate_settings` | always | calls through with the populate flag raised |
| `event` `jc3vr.pause_menu.open` | entry added, no VR page shown | opens the VR screen; the game never sees it |
| `event` `jc3vr.pause_menu.open` | otherwise | dropped |
| `event` `jc3_gui_msg_box_show_hide` | the VR entry is opening the screen | dropped once (the nested selection's sound) |
| `set_setting_value` | id in the VR band, row of the shown page | applies the value; the game never sees it |
| `get_key_for_action` | id in the VR band, row of the shown page | runs the button's action; the game never sees it |
| either of those two | id in the VR band, otherwise | dropped |
| a tab command | a VR tab is shown | records the tab for the next update |
| a tab command | otherwise | dropped |
| `restore_default_keymap` | a VR page is shown | resets the page's value items and rebuilds it |
| `persist_changes` | a VR page is shown | calls through, then saves the tab's position, leaves the page, hides the tab bar, restores the clips' style, and returns to the root menu |
| the base `inactive` | always | calls through, then closes the session |
| anything else | | calls through |

The movie's reset-all sends one `set_setting_value` per row, headers and buttons included; items that hold no value ignore it.

H4, after [`PauseUI::UpdateImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-UpdateImpl), runs while a VR page is shown and the screen is active: it finishes opening the VR tabs, switches to a chosen tab, runs the page's `on_frame`, runs a deferred rebuild, moves the highlight off a header, and last runs the row pass (the header styles and the live values). Rebuilds are deferred to here because the command hook runs inside the movie's handler, which redraws the selected row after the command returns. They also wait while the list is not in its root state (`IsInRootState` false: an option picker is open), because the list sends the picker's answer to whichever row is highlighted when it closes. A rebuild writes the new rows into the shown page object, calls `RefreshCurrentScreen`, rewrites the list title, and focuses the first button with both `MoveSelectionHighlight` and `SetHighlightedRow`, since highlighting alone would leave the selection behind. The session records the new page as soon as the refresh has run, since the rows' ids refer to it from then on.

H6, on [`UIBase::InvokeImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_base::UIBase##func-InvokeImpl), sees every AS3 method the engine calls on a screen, among them `OnActiveImpl`'s `InitMenu` and the input handlers [`UIBase::BaseOnManageInput`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_base::UIBase##func-BaseOnManageInput) calls by name. An `InitMenu` on the screen H1 is activating, with a root menu valid, on the capture thread, and the screen active, runs as described under [the root entry](#the-root-entry). After an `OnNextTabButton` or `OnPrevTabButton` on the pause screen while a VR tab is shown, it calls the tab bar's method of the same name. Every other call pays a name comparison and an atomic load.

H5, after [`PauseUI::OnDeactivateImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-OnDeactivateImpl), which [`UIBase::Deactivate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_base::UIBase##func-Deactivate) calls for every deactivation of the loaded menu before the movie unloads, closes the session and runs the shown page's `on_close`.

## Values and threads

`pause_menu/gfx/` wraps the Scaleform value API ([`ValueObjectInterface`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::ValueObjectInterface)). `CaptureThread::current()` returns a token, neither `Send` nor `Sync`, only on the thread [`UIManager::m_CurrentCaptureThread`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_CurrentCaptureThread) names. `GfxValue<'t>` is a boxed value borrowing the token: the box pins a managed value (a node on the movie's external-references list) and dropping it releases the reference, so no managed value outlives the hook call. The object calls accept only an object, an array, or a display object; a managed string or closure is refused.

The hooks hold the pause screen as a pointer (`gfx::Screen`), never a `&mut PauseUI`, because a call into the movie can re-enter the game's command handler, which writes the screen's fields through its own pointer. Every access starts again from the screen's [`m_Clip`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_base::UIBase##field-m_Clip), copied with an added reference. Arguments are written as the engine's own [`SetIntField`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/ui::ui_base##func-SetIntField) and [`SetStringField`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/ui::ui_base##func-SetStringField) write theirs, and row objects and arrays are created with [`UIBase::CreateObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_base::UIBase##func-CreateObject) and `CreateArray`, as [`SettingsUI::AppendSettingToggle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::SettingsUI##func-AppendSettingToggle) does. The game reads `set_setting_value`'s arguments as ints without checking their type; the mod decodes ints and integral numbers and passes anything else through.

## The page model

A page is data, defined beside the settings it edits (`pause_menu/model/page.rs`) and registered in `PAGES` (`pause_menu/pages.rs`):

- `PageDef { tab, build, on_close, on_frame }`: the tab's label; a `fn() -> Page` that builds the current contents, on open and on every rebuild; an optional `fn()` run once when the page's stay ends, by any path; and an optional `fn() -> ButtonOutcome` run every frame while shown, returning `Rebuild` to rebuild.
- `Page::new(title)` with `.header(text)`, `.text(text)`, and `.item(item)`.
- `Toggle` over a `bool`, `Choice` over an index into a static list of labels, `Slider` over an `f32`, `Readout` (a `fn() -> SmolStr` refreshed every frame), and `Button` (whose `action` returns `ButtonOutcome::Stay` or `Rebuild`). Value items carry a `default`.
- `SliderRange::new(min, max, steps)` maps `[min, max]` onto the notches `[0, steps]`. The movie's left and right step a tenth of the notches, truncated, so the registered pages use notch counts that are multiples of ten, which a registry test checks. `SliderFormat::new(decimals, suffix)` says how the value reads in the label.

The accessors are plain function pointers, so non-capturing closures work. They run on the capture thread inside the hooks, without the session lock, and must not call into the movie.

A step-by-step page (the body calibration) keeps its step in a static beside its definition, lists the current step's rows from `build`, and returns `Rebuild` when the step changes; H4 then reinstalls the page and focuses its first button, so each step puts its main action first. A page built with `Page::keeping_focus` keeps the highlight on its row instead, for rows that change with a value being changed: `VR: CONTROLS` rebuilds from its per-frame hook when the wingsuit's pitch measure changes, so the neutral-tilt calibration shows only for the wrist tilt. Text rows are one upper-cased line each, so instructions span several rows. The message box is not used for steps, because the list routes its input only while its own private state says it opened it.

## Pages

Four pages are registered, in this order: `BODY` (`controllers/body/`, titled `VR: BODY`), `CONTROLS` (`controllers/menu/controls/`), `COMFORT` (`controllers/menu/comfort.rs`), and `DISPLAY` (`controllers/menu/display.rs`). Labels are written in upper case because the movie upper-cases row labels and key text but shows option labels as given; a registry test checks it.

### Settings pages

The pages hold only the comfort settings, the display, the core controls, and the body calibration. Feature switches, their tuning, the diagnostic readouts, and the status rows are in the debug UI ([settings.md](settings.md#where-a-setting-is-exposed), [status.md](status.md)). The flatscreen IPD is not offered: it is the view tuning's `camera.flatscreen_ipd_m` and spaces the eyes only for the flatscreen stereo, since under a headset the runtime supplies each eye's view.

The value items are built by the `config_toggle!`, `config_slider!`, and `config_choice!` macros in `controllers/menu/mod.rs` from a slot and a place in its group (`CONTROLLERS_CONFIG, |c| c.aim.assist_strength`): the item reads and writes that place and takes its default from the configuration's defaults, so the page's reset (Y) restores them. A change applies at once through the slot's edit guard, so the settings file saves it ([settings.md](settings.md)); browsing without changing anything saves nothing.

`VR: CONTROLS` has these sections:

- `CONTROLLERS`: `MOTION CONTROLLERS`, the dominant hand (with a note that a change takes effect on restart), `MOVE DIRECTION`, and `MOVE STICK DEADZONE`.
- `HANDS AND AIM`: `AIM ASSIST`, `GRIP MARKERS` ([grip.md](input/grip.md)), and `HAND STRENGTH`.
- `WEAPONS`: `HOLD GRIP TO HOLD GUNS`, `STICK CLICK CYCLES WEAPONS`, `TRUE RELOAD` ([reload.md](input/reload.md)), and `RECOIL STRENGTH`.
- `HAPTICS`: `HAPTICS`, `HAPTIC STRENGTH`, and `GAME RUMBLE` ([haptics.md](input/haptics.md)).
- `VEHICLES`: `WHEEL TURNED BY`, `WHEEL FULL LOCK`, `WINGSUIT ARM STEERING`, `WINGSUIT PITCH BY` with, for the wrist tilt, `CALIBRATE NEUTRAL` (the wrists' tilt when pressed becomes the tilt that pitches nothing), `INVERT WINGSUIT PITCH`, and `PARACHUTE HANDLES`.
- `SAVED SETTINGS`: a note that changes apply at once and save automatically, and `RESET TO DEFAULTS`, which replaces the page with a confirmation whose first, focused button keeps the settings. The second resets every setting `VR: CONTROLS` and `VR: COMFORT` show, through the pages' own items, and keeps the dominant hand (it describes the player and takes a restart), the per-weapon sight calibration, the body page's settings, and the debug UI's switches; the page's own Y reset does reset the dominant hand. A choice the reset changes can show rows its old value hid, so the pages are reset again until the rows stop changing. The display settings are left to `VR: DISPLAY`'s Y reset.

`VR: COMFORT` has these sections:

- `VIEW`: `RECENTER VIEW`, focused when the page opens, and an instruction to look ahead first. It asks for the recenter F7 does (`vr::request_recenter` with `roomscale::manual_recenter_kind`; [body/rig.md](body/rig.md#recentering)). The request is serviced at the start of the next frame, because the menu runs on the capture thread mid-frame while the frame in flight holds the runtime lock; a headset with no head pose yet drops it with a log line, and so does a missing VR session.
- `POSTURE`: `POSTURE` (`AUTO`, `SEATED`, or `STANDING`), `BODY FOLLOWS YOU` ([body/rig.md](body/rig.md#settings-and-diagnostics)), and `FADE WHEN HEAD IS IN WALLS`.
- `TURNING`: `TURN MODE`, `SMOOTH TURN SPEED`, and the snap angle (a choice of 15 to 90 degrees; a saved angle between them shows as the nearest).
- `MOVEMENT`: `SMOOTH STICK MOVEMENT` ([body/locomotion.md](body/locomotion.md)) and `VIEW ON THE GRAPPLE` ([body/grapple-comfort.md](body/grapple-comfort.md)).
- `VEHICLES`: `VEHICLE ENTRY` (the seat transition) and `FIT VIEW TO THE WHEEL` ([input/steering-wheel.md](input/steering-wheel.md)).

`VR: DISPLAY` has a `RENDERING` section with `RESOLUTION` (`vr.resolution_scale`, on the runtime's recommendation; it acts only while `vr.native_resolution` is on), `FSR SHARPENING` and `SHARPNESS` ([rendering/fsr.md](rendering/fsr.md)), and `FOVEATED RENDERING` ([stereo/foveation.md](stereo/foveation.md)); a `FLOATING PANEL` section with `PANEL DISTANCE` and `PANEL SIZE` ([hud.md](hud.md)); and a `HUD` section with `HUD ON THE HANDS`, the HUD atlas. Each applies on the next frame, except that the OpenXR swapchain keeps the size it was created at: a resolution change resizes the scene at once and the headset image after a restart, which two text rows say.

A test puts every value item through its whole range, checking that it reads back what it wrote, that the file stays valid, and that the page's reset returns the defaults.

### Body calibration

`VR: BODY` (`controllers/body/`) opens on a summary: `YOUR BODY` with the player's saved dimensions (the `[body]` section of the settings file) and whether they are still the generic defaults; `CALIBRATION` with the `STANCE` choice (`STANDING` or `SEATED`) and the button that starts the calibration; and `SCALING` with `MATCH CHARACTER HEIGHT` and `MATCH CHARACTER REACH` (`controllers.body.remap_head` and `remap_hands`). `STANCE` defaults to `SEATED` in the seated posture, and in the automatic posture while it detects the player seated, and to `STANDING` otherwise; the choice lasts until the page is left and is not saved.

The calibration is a three-step flow (`jc3vr_body::controllers::body::calibration`, with the flow in `flow.rs` and the derivation in `derive.rs`). Each step is under a header naming it (`STEP 1 OF 3: HEIGHT`), with a short instruction, its main action first, a `STATUS` readout, and its live readouts. Standing:

1. Height: standing straight, looking ahead. `EYE HEIGHT, LIVE` is the head's height above the STAGE floor and `HEIGHT, ESTIMATED` the height derived from it.
2. Arm span: a T-pose with the controllers held. `CONTROLLER GAP, LIVE` is the distance between the grip poses and `ARM SPAN, ESTIMATED` the fingertip span derived from it.
3. Review: `SAVE` first, then the result under `MEASURED` and `ESTIMATED` (the shoulders and the arm segments), the saved seated eye height under `KEPT FROM THE SAVED BODY` when there is one, and `MEASURE AGAIN` (the arm span only, or both steps). `SAVE` validates the dimensions and applies them through an edit guard, so the settings file saves them.

A standing calibration keeps the saved seated eye height, unless the new standing eye height is not above it. Seated, the player sits upright in the chair they play in:

1. Seated eyes: the live eye height becomes the saved `seated_eye_height`, which the seated eye reference and the automatic posture's thresholds sit against ([controllers.md](input/controllers.md#body-dimensions)).
2. Arm span: as standing, with the height estimated from the span.
3. Review: `ESTIMATED` starts with a `HEIGHT` slider set to the estimate, for the player to correct; the standing eye height, the shoulders, the hand, and the arm segments follow it at once, and its Y reset returns the estimate. `SAVE` writes every value.

Each step has `BACK` and `CANCEL` buttons to the previous step and the summary. A T-pose leaves no hand free to point at the menu, so `MEASURE` starts a countdown and then samples for a window (the body tuning's `calibration.countdown_ms` and `calibration.sample_window_ms`); the page's per-frame hook feeds the capture. Step 1 takes the mean of the samples and step 2 the widest. A frame without a measurement is skipped, and the window needs `calibration.min_samples` samples. A finished capture moves to the next step and rebuilds the page. Leaving the page abandons a calibration in progress.

`STATUS` shows the countdown, the sampling, the last problem, or whether the measurement is available. The problems are a missing measurement (no headset frame, no STAGE space or its location, no controller snapshot, or a controller lost), a result outside the accepted ranges in `derive.rs`, and on the review a failed save. A span far over the height is accepted but flagged (`CHECK: SPAN OVER HEIGHT`): the controllers cannot be further apart than the arms reach, so it most likely means a standing height was measured seated. Every problem is also logged, rate-limited (target `controllers`). A grip's position counts whether the runtime measured or inferred it, because arms spread in a T-pose are outside an inside-out headset's cameras.

## Row encoding

`model/rows.rs` encodes each item as the row object one of the movie's widgets draws. Every row sets `m_Type` (its id), `m_Format`, `m_Name`, `m_Value`, `m_DefaultValue` (the reset-all reads it for every row), and `m_NeedConfirm` (false).

| Item | Format | Other fields | Behaviour |
|---|---|---|---|
| Header | key | as Text | drawn in the list title's style and skipped by the highlight |
| Text | key | `m_KeyText` and `m_Image` empty, plain `m_Interaction`, not customisable | selecting plays the refusal sound |
| Readout | key | `m_KeyText` is the value, not customisable | as Text; the value is live |
| Button | key | `m_KeyText` is the caption, customisable, `m_Action` is the id | selecting sends `get_key_for_action` |
| Toggle | toggle | `m_Labels` `OFF`/`ON`, `m_Values` 0/1 | the option picker |
| Choice | toggle | `m_Labels` the options, `m_Values` their indices | the option picker |
| Slider | slider | `m_MinValue` 0, `m_MaxValue` the notch count | the value is written into the label |

Row ids (`model/ids.rs`) live in a band of their own far above the game's setting types and key actions, and encode the page's registry index and the item's index, so a command for a page no longer shown is recognised and dropped. Values are never negative, because the list uses negative values as its own context-menu codes.

## Live values

The list shows a scrolling window of the page on a fixed set of row clips ([`SettingsUI`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::SettingsUI)'s `ROW_CLIP_COUNT`) under `MCI_settings_screen.MCI_panel.MCI_container.MCI_root.MCI_manager`. Each frame, H4's row pass asks each clip for the row object it shows (`GetData()`), decodes its id, sets the clip's label style, and computes the item's live text: a readout's value, a slider's label with its value, or a button's caption (the list replaces it with `...` when the button is selected, expecting a key capture). `LiveSlots` (`model/live.rs`) remembers what was last written per clip and writes only when the clip's item or text changed; a write sets both the row object's field (`m_KeyText` or `m_Name`, so the list's own redraws show it) and the clip's text field, upper-cased as the list draws it. An applied value or pressed button invalidates the diff, because the list has redrawn the row. The pass never calls the row's `FillData`, which would restart the highlighted row's animation. Toggle and choice rows are not refreshed when their value changes elsewhere (from the debug UI, say) while the page is shown; they show it the next time the page opens.

## Unverified in game

The pure logic (ids, row encoding, slider mapping, the live diff, the state machine, command routing, the tabs' memory, the header bookkeeping, the calibration flow, and the pages' value mappings) is unit-tested. The movie-facing assumptions to check in game are chiefly: the split `InitMenu` build and the busiest root menu still fitting the main buttons; the opening from inside the command and the native graphics page still working afterwards in the same pause; `PauseUI::UpdateImpl` and the screen's input running on the capture thread (otherwise H4 logs one warning and live values and rebuilds never run); the tab bar, never shown in the shipped game, laying out over the settings screen and accepting the document's `Invoke` as its callback; the title format reading as a title on a row label; and the VR frame loop running while paused often enough for the calibration's sampling window.
