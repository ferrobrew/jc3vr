# The VR menu in the pause menu

The VR settings a player reaches for in the headset live inside the game's own pause menu; the rest are in the debug UI ([settings.md](settings.md#where-a-setting-is-exposed)). A `VR` entry in the root menu, directly below `OPTIONS`, opens the game's settings screen with the VR pages as tabs on its tab bar. Each page is described in Rust and drawn by the pause movie's own widgets. The mod ships no Flash and does not patch `pause.gfx`: it reaches the movie through the Scaleform value API from five detours on [`PauseUI`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI) and one on [`UIBase::InvokeImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_base::UIBase##func-InvokeImpl). The movie itself is described in [engine/pause-menu.md](../engine/pause-menu.md). The code is `crates/jc3vr-ui/src/pause_menu/`, and the pages are in `crates/jc3vr-ui/src/controllers/`.

## Route

The movie's own folders use the developer menu's debug text, so the VR pages are tabs on the settings screen's tab bar, the navigation bar the game's other menus share ([engine doc](../engine/pause-menu.md#the-tab-bar)). The movie's own code opens the settings screen and the mod replaces the page it is about to read. A movie of the mod's own would have to rebuild all of that ([engine/scaleform.md](../engine/scaleform.md)), and patching `.gfx` files would mean repacking the archives. The page registry is capped (`MAX_PAGES` in `pause_menu/pages.rs`) at the tab bar's selector count.

## The root entry

From outside, the movie can only append or prepend to a list, and the engine prepends and appends its own entries ([engine doc](../engine/pause-menu.md#menus-and-entries)), so the entry is appended while the movie's `InitMenu` builds the root menu (`model/root.rs` picks the in-game or front-end menu from [`PauseUI::m_IsValidMenu`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##field-m_IsValidMenu)). In the in-game menu `InitMenu` adds the main-menu entry right after options in the same call, so H6 runs it with the main-menu list marked invalid, appends `VR`, runs it again to build the main-menu list, and appends the main-menu entry as the movie would. When `InitMenu` could not run that way, H1 appends the entry at the end and logs a warning. The label is plain text, since a label beginning with `&` or `#` is looked up as a localisation key.

Only the options menu's dispatch opens the settings screen, and the document's menu state is private. So the entry is an [`OPTION_EVENT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI) posting `jc3vr.pause_menu.open`; H3 takes that command inside the selection handler (`runtime/root.rs`), switches the entry's type to [`OPTION_GRAPHICS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI), calls `ShowPauseFromHideCredits` to enter the options menu's state and `OnSelect` to open the settings screen, then switches the type back. Backing out sends `persist_changes`, after which H3 returns to the root menu with the VR entry highlighted.

## Tabs

The settings screen's own tab switch keeps scroll positions in a private array keyed by page slot, shared with the native pages ([engine doc](../engine/pause-menu.md#the-settings-screen)), so every VR tab is shown in the graphics page's slot and the mod switches tabs itself, keeping the VR tabs' positions apart (`model/tabs.rs`). On opening, H4 names one selector per VR page and sets the bar's callback to the document's own `Invoke`, so a selector sends its index as a command that H3 recognises (`TabCommands`). A switch saves the old tab's position, runs its `on_close`, writes the new page in, redraws, and restores the new tab's position. The VR entry reopens on the last tab until the menu closes.

## Section headers

The list has no header row ([engine doc](../engine/pause-menu.md#row-objects-and-widgets)), and an empty spacer row would stop navigation. So a header is a key-binding row with no key text, its label in the list title's text format. The list reuses its clips as it scrolls, so each frame `HeaderStyles` (`model/headers.rs`) restyles a clip when the kind of row it shows changes, and restores native styles on leaving. When the highlight lands on a header, H4 moves it on in the direction it was going (`skip_header`).

## The session

`model/state.rs` holds the state machine (`Closed`, `EntryAdded`, `Shown { page }`) and `runtime/session.rs` the shared session. A page's `on_close` runs once per stay, after the lock is released, by whatever path leaves the page. H5 exists because the pause key, a forced close, and game-state changes deactivate the menu without `persist_changes`.

The session lock is never held across a call into the movie or an item's functions: the movie can re-enter the command hook synchronously (a widget's sound sends a command), and the lock is not re-entrant. Each hook copies what it needs, releases the lock, works, and writes back.

## Hooks

H1 to H4 and H6 run on the UI capture thread, which is checked before any value is touched; H5 runs on whichever thread deactivates the menu and never touches the movie.

- H1, on [`PauseUI::OnActiveImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-OnActiveImpl), closes any stale session and lets the game build the menus, with H6 adding the entry inside.
- H2, after [`PauseUI::PopulateSettings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-PopulateSettings), writes the VR page object over the graphics slot ([`PAGE_GRAPHICS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::SettingsUI)) of `m_SettingsArray` when the VR entry opened the screen, and otherwise leaves any VR page and restores the native presentation.
- H3, on [`PauseUI::HandleCommandImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-HandleCommandImpl), decodes the command (`model/command.rs`). Commands for VR rows (`set_setting_value`, `get_key_for_action`), tab commands, the open event, `restore_default_keymap` on a VR page, and the nested selection's second sound are handled and never reach the game; a command for a VR row not shown is dropped; `persist_changes` and `inactive` call through and then close the VR screen or the session.
- H4, after [`PauseUI::UpdateImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-UpdateImpl), finishes opening the tabs, switches tabs, runs the page's `on_frame` and any deferred rebuild, skips headers, and runs the row pass. Rebuilds wait for it because the command hook runs inside the movie's handler, which redraws the selected row afterwards, and they wait while an option picker is open, since the list sends the picker's answer to whichever row is highlighted.
- H5, after [`PauseUI::OnDeactivateImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-OnDeactivateImpl), which [`UIBase::Deactivate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_base::UIBase##func-Deactivate) calls before the movie unloads, closes the session.
- H6, on `UIBase::InvokeImpl`, sees every AS3 method the engine calls on a screen: it splits the root menu's `InitMenu` and forwards the shoulder-button tab steps.

## Values and threads

`pause_menu/gfx/` wraps the Scaleform value API ([`ValueObjectInterface`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::ValueObjectInterface)). `CaptureThread::current()` returns a token, neither `Send` nor `Sync`, only on the thread [`UIManager::m_CurrentCaptureThread`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_CurrentCaptureThread) names, and `GfxValue<'t>` borrows it, pinning a managed value until dropped, so no managed value outlives the hook call. The hooks hold the pause screen as a pointer (`gfx::Screen`), never a `&mut PauseUI`, because a call into the movie can re-enter the game's command handler, which writes the screen through its own pointer.

## The page model

A page is data, defined beside the settings it edits (`pause_menu/model/page.rs`) and registered in `PAGES`. `PageDef { tab, build, on_close, on_frame }` gives the tab label, a builder run on open and every rebuild, a hook run when the stay ends, and a per-frame hook that can ask for a rebuild. `Page` holds headers, text rows, and items: `Toggle`, `Choice`, `Slider`, `Readout` (refreshed every frame), and `Button`. The accessors run on the capture thread without the session lock and must not call into the movie. The movie's left and right step a tenth of a slider's notches, so `SliderRange` notch counts are multiples of ten. Labels are upper case because the movie upper-cases row labels but not option labels; registry tests check both.

A step-by-step page (the body calibration) keeps its step in a static, builds the current step's rows, and asks for a rebuild when the step changes, which focuses its first button; `Page::keeping_focus` keeps the highlight in place instead. The message box is not used for steps, because the list routes input to it only when its own private state opened it.

The value items are built by `config_toggle!`, `config_slider!`, and `config_choice!` (`controllers/menu/mod.rs`) from a configuration slot and field; they apply at once through the slot's edit guard, so the settings file saves them ([settings.md](settings.md)), and take their defaults from the configuration's, so a page's reset (Y) restores them.

## Pages

Four pages are registered, in order: `BODY` (`controllers/body/`), `CONTROLS` (`controllers/menu/controls/`), `COMFORT` (`controllers/menu/comfort.rs`), and `DISPLAY` (`controllers/menu/display.rs`). They hold only comfort, display, core controls, and the body calibration; feature switches and readouts are in the [debug UI](debug-ui.md). `VR: CONTROLS` ends with `RESET TO DEFAULTS`, which resets what `CONTROLS` and `COMFORT` show but keeps the dominant hand, the sight calibration, the body, and the debug UI's switches. `VR: COMFORT`'s `RECENTER VIEW` requests the F7 recenter ([body/rig.md](body/rig.md#recentering)), serviced next frame because the menu runs while the frame holds the runtime lock. On `VR: DISPLAY` (which includes the [floating panel](hud.md)'s distance and size), a resolution change resizes the headset image only after a restart, since the swapchain keeps its size.

### Body calibration

`VR: BODY` opens on a summary of the saved dimensions (`[body]` in the settings file), the `STANCE` choice (defaulting to the detected posture, not saved), the button that starts calibration, and `MATCH CHARACTER HEIGHT` and `MATCH CHARACTER REACH`. The calibration (`jc3vr_body::controllers::body::calibration`: `flow.rs` and `derive.rs`) has three steps, each with its main action first, a `STATUS` readout, live readouts, and `BACK` and `CANCEL`:

1. Height standing (eye height above the STAGE floor, from which height is derived), or seated eye height seated, which the seated reference and the automatic posture sit against ([roomscale.md](input/roomscale.md#body-dimensions)).
2. Arm span: a T-pose with the controllers held, measured as the grip gap.
3. Review, with `SAVE` first; seated, a `HEIGHT` slider lets the player correct the height estimated from the span. A standing calibration keeps the saved seated eye height unless the new standing one is not above it.

A T-pose leaves no hand to point with, so `MEASURE` starts a countdown and samples for a window (`calibration.countdown_ms`, `calibration.sample_window_ms`, `calibration.min_samples` in the body tuning); height takes the mean and span the widest. `STATUS` shows the countdown, sampling, or the last problem (no measurement, out of range, failed save); a span far over the height is flagged.

## Rows and live values

`model/rows.rs` encodes each item as a movie row object: headers, text, readouts, and buttons as key-binding rows (a button sends `get_key_for_action`), toggles and choices as option-picker rows, and sliders as slider rows with the value written into the label. Row ids (`model/ids.rs`) sit in a band far above the game's setting types and encode the page and item, so a command for a page no longer shown is dropped. Values are never negative, because the list uses negative values as its own codes.

The list shows a scrolling window on a fixed set of row clips ([`SettingsUI`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::SettingsUI)'s `ROW_CLIP_COUNT`). H4's row pass asks each clip for its row (`GetData()`) and writes live text (a readout, a slider label, a button caption) only when it changed (`LiveSlots`, `model/live.rs`), into both the row object and the clip, never calling `FillData`, which would restart the highlight animation. Toggles and choices changed elsewhere while the page is shown update on the next opening.

## Unverified in game

To check in game: the split `InitMenu` with the busiest root menu, the native graphics page after a VR opening in the same pause, `UpdateImpl` running on the capture thread (otherwise H4 warns once), the never-shown tab bar's layout, the title format on a row label, and the frame loop running while paused often enough for calibration sampling.
