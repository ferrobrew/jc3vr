# The VR menu in the pause menu

The mod's VR settings live inside the game's own pause menu. Each VR page is an entry at the end of the options menu that opens the game's settings screen on a page the mod describes in Rust, so the page is drawn by the pause movie's own list, rows, option picker, slider, message box, sounds, and gamepad and mouse handling. The mod ships no Flash of its own and does not patch `pause.gfx`: it reaches the movie through the Scaleform value API from four detours on [`PauseUI`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI). The engine side (the movie's structure, the commands it sends, and what code outside it can reach) is in [`engine/pause-menu.md`](../engine/pause-menu.md). The code is `payload/src/pause_menu/`.

The feature is behind `pause_menu.enabled`, off by default, with a checkbox in the debug UI's HUD tab (under "Pause menu", which also shows the menu state). The setting is read when the pause menu opens, so a change takes effect the next time it does. With it off, no entries are added and every hook calls through.

## Route

The pause movie has no tabs in its settings screen: its options menu is a column of entries, each opening the screen on one fixed page. The closest native equivalent of a VR tab is therefore one options entry per VR page. Each VR entry is added with the graphics entry's type ([`OPTION_GRAPHICS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI)), so the movie's own code opens the settings screen for it, and the mod replaces the page the screen is about to read while the game fills the pages. The rest of the game's pages stay as the game built them.

Tabs inside one VR entry were considered and deferred. Every VR page opens as the graphics page's index, and switching tabs inside the settings screen would write the screen's private per-tab scroll memory for indices the native pages share, so a native page opened later in the same pause would restore a VR page's scroll position. The memory cannot be cleared from outside the movie.

The movie lays a menu out on a fixed number of main buttons ([`MAIN_BUTTON_COUNT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI)) and drops the entries past the last. The VR entries go after the options menu's own entries, which keeps the native entries at their positions, so the menu still opens with its first native entry highlighted (the movie remembers the highlighted position per menu and starts at the first). The registry is capped (`MAX_PAGES` in `pause_menu/pages/mod.rs`) at the buttons the options menu's own entries leave free on PC; a page past the cap is not added and a warning is logged. Entry labels are plain text: a label beginning with `&` or `#` would be looked up as a localisation key.

## The session

`pause_menu/model/state.rs` holds the state machine and `pause_menu/runtime/session.rs` the shared session:

- `Closed`: no VR entries in the loaded movie.
- `EntriesAdded { count }`: the last `count` options entries are the VR pages, in registry order.
- `Shown { count, page }`: the settings screen shows VR page `page`.

Activation resets the session to `Closed` and adds the entries (`EntriesAdded`). A VR entry opening the settings screen moves it to `Shown`; leaving the screen returns it to `EntriesAdded`; the movie unloading returns it to `Closed`. The session also keeps the shown page's contents (to resolve its rows' commands), the live-value diff, a deferred-rebuild flag, and the populate flag described under H3.

The session lock is never held across a call into the movie or into an item's functions: a call into the movie can re-enter the command hook synchronously (a widget playing a sound sends an event command), and the lock is not re-entrant. Each hook copies what it needs, releases the lock, works, and writes back; a generation counter tells a write-back whether the page changed in between.

## Hooks

All four run on the UI capture thread. H1 to H3 run inside the root movie's advance; H4 is the pause screen's per-frame update, whose thread is checked before any value is touched.

H1 is a post-detour on [`PauseUI::OnActiveImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-OnActiveImpl). The game has built the fresh movie's menus; when the feature is on and the options menu is valid ([`m_IsValidMenu`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##field-m_IsValidMenu)), the hook calls the movie's `AddOption` once per page, appending, and sets each returned entry's `m_Type` to the graphics entry's type. The options menu is laid out only when the player switches to it, so the entries show.

H2 is a post-detour on [`PauseUI::PopulateSettings`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-PopulateSettings), which the movie's `populate_settings` command and the update's refresh both call. When the screen is opening (the populate flag is up), the hook reads every main button (`MCI_button_container.MCI_root.MCI_button_NN.MCI_button`): whether it is visible, whether it is selected, and its label. `pause_menu/model/entries.rs` takes the first visible selected button, as the movie's own `GetCurrentOption` does (a hidden button keeps whatever selection it last had), and matches its label against the VR entries' labels; any other label is a native entry. When no label can be read, the VR entries are taken to be the last visible buttons. When a VR entry opened the screen, or a VR page is shown and is being refreshed, the hook builds the page, encodes its rows, and writes a page object `{m_Name, m_Settings}` over the graphics page's slot ([`PAGE_GRAPHICS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::SettingsUI)) of `m_SettingsArray`. The movie then shows it: the panel title is the page title. When a native entry opened the screen, the session leaves any VR page and the native pages are untouched. A failed install logs an error and leaves the native page.

H3 is a detour on [`PauseUI::HandleCommandImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-HandleCommandImpl). It decodes the command and its arguments (`model/command.rs`) and routes it:

| Command | Condition | Handling |
|---|---|---|
| `populate_settings` | always | calls through with the populate flag raised, so H2 knows the screen is opening |
| `set_setting_value` | id in the VR band, row of the shown page | applies the value to the item; the game never sees it |
| `get_key_for_action` | id in the VR band, row of the shown page | runs the button's action; the game never sees it |
| either of the two | id in the VR band, any other state | dropped, never reaches the settings manager or the key capture |
| `restore_default_keymap` | a VR page is shown | resets the page's value items to their defaults and rebuilds the page |
| `persist_changes` | a VR page is shown | calls through, then leaves the page and runs its `on_close` |
| the base `inactive` | always | calls through, then closes the session |
| anything else | | calls through |

The command detour routes regardless of the toggle; with no page installed it passes everything through except a VR-band id, which cannot occur. The movie's reset-all sends one `set_setting_value` per row, headers and buttons included; items that hold no value ignore it.

H4 is a post-detour on [`PauseUI::UpdateImpl`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::PauseUI##func-UpdateImpl). While a VR page is shown and the screen is active, it first runs the page's per-frame hook (`on_frame`, which may ask for a rebuild), then a deferred rebuild, then the live-value pass. Rebuilds are deferred to here because the command hook runs inside the movie's own handler, which redraws the selected row after the command returns. A rebuild writes the new rows into the shown page object, calls the settings screen's `RefreshCurrentScreen`, rewrites the list's title, and focuses the page's first button with the list's `MoveSelectionHighlight` and `SetHighlightedRow` (both are needed: the refresh leaves the first row highlighted and selected, and highlighting alone would leave the selection behind).

## Values and threads

`pause_menu/gfx/` wraps the Scaleform value API ([`ValueObjectInterface`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::ValueObjectInterface)). `CaptureThread::current()` returns a token only on the thread [`UIManager::m_CurrentCaptureThread`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_manager::UIManager##field-m_CurrentCaptureThread) names; it is neither `Send` nor `Sync`. `GfxValue<'t>` is an owned, boxed value borrowing the token: the box pins a managed value (it is a node on the movie's external-references list), and dropping it releases the reference through [`ObjectRelease`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::ValueObjectInterface##func-ObjectRelease). Because the value borrows the token, it cannot be stored past the hook call, so no managed value is held across frames. Every access starts again from the screen's [`m_Clip`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_base::UIBase##field-m_Clip), copied with an added reference ([`ObjectAddRef`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::ValueObjectInterface##func-ObjectAddRef)).

Arguments and fields are written as unmanaged values laid out the way the engine's own [`SetIntField`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/ui::ui_base##func-SetIntField) and [`SetStringField`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/ui::ui_base##func-SetStringField) build theirs (a C string pointer for a string, an int for an int), which the movie copies. Row objects and arrays are created with [`UIBase::CreateObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_base::UIBase##func-CreateObject) and [`CreateArray`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::ui_base::UIBase##func-CreateArray), as [`SettingsUI::AppendSettingToggle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::SettingsUI##func-AppendSettingToggle) does. The command handler reads `set_setting_value`'s arguments as plain ints without checking their type; the mod decodes ints, and numbers with an exact integer value, and treats anything else as a malformed command it passes through.

## The page model

A page is data, defined beside the settings it edits (`pause_menu/model/page.rs`) and registered in `PAGES` (`pause_menu/pages/mod.rs`):

- `PageDef { entry, build, on_close, on_frame }`: the options entry's label, a `fn() -> Page` that builds the page's current contents (called on open and on every rebuild), an optional `fn()` run when the player leaves the page, to save, and an optional `fn() -> ButtonOutcome` run every frame while the page is shown, for work that runs over time; returning `Rebuild` rebuilds the page.
- `Page::new(title)` with the builders `.header(text)`, `.text(text)`, and `.item(item)`.
- `Toggle { label, get, set, default }` over a `bool`.
- `Choice { label, options, get, set, default }` over an index into a static list of labels.
- `Slider { label, range, format, get, set, default }` over an `f32`: `SliderRange::new(min, max, steps)` maps `[min, max]` onto the integer notches `[0, steps]` (the movie's left and right step a tenth of the notches, truncated to an integer, so the registered pages use notch counts that are multiples of ten, which a registry test checks), and `SliderFormat::new(decimals, suffix)` says how the value reads in the label.
- `Readout { label, read }`: a `fn() -> SmolStr` shown on the right and refreshed every frame.
- `Button { label, caption, action }`: selecting it runs `action`, which returns `ButtonOutcome::Stay` or `ButtonOutcome::Rebuild`.

The accessors are plain function pointers, so non-capturing closures work (`get: || CONFIG.lock().x`). They run on the capture thread inside the hooks, without the session lock, and must not call into the movie.

A step-by-step page (the body calibration) keeps its step in a static beside its definition. Its `build` lists the current step's rows, and its buttons and per-frame hook return `Rebuild` when the step changed; H4 then reinstalls the page and focuses its first button, so each step puts its main action first. Text rows are one upper-cased line each, so instructions are split over several rows. The message box is not used for steps: the list only routes its input while its own private state says it opened it.

## Pages

Three pages are registered, in this order: `VR: BODY` (`controllers/body/menu.rs`), `VR: CONTROLS` (`controllers/menu/controls.rs`), and `VR: COMFORT` (`controllers/menu/comfort.rs`). Labels are written in upper case in the source because that is how the movie shows them: it upper-cases row labels and key text, and shows option labels as given. A registry test checks it.

### Settings pages

`VR: CONTROLS` and `VR: COMFORT` edit the settings `controllers.local.json` holds, and only those, so that everything they change can be saved: settings outside the file (the swimming level view, the grapple comfort settings, the mouse sensitivity) stay in the debug UI. Their items are built by the `config_toggle!`, `config_slider!`, and `config_choice!` macros in `controllers/menu/mod.rs` from a place in the configuration (`|c| c.controllers.aim.gun`): the item reads and writes that place and takes its default from `Config::new`, so the page's reset (Y) restores the configuration's defaults. A change applies at once and marks the settings changed. Leaving any VR page (`on_close`) queues a save through the persistence worker when they changed; when the worker is busy, they stay marked and the next page left saves them. Nothing is saved when nothing changed, so browsing the menu does not create the file.

`VR: CONTROLS` has the master switch, the control scheme, the dominant hand, the move direction and the move stick's deadzone; the turn mode, the smooth-turn speed, and the snap angle (a choice of 15, 30, 45, 60, or 90 degrees; an angle saved between them shows as the nearest); the hand IK, hand aiming for the guns and the grapple, the aim assist strength, the gun aim marker, and the finger curls; the holsters, the foregrip, recoil and its strength, scopes, the magazine reload, thrown grenades, and melee strikes; haptics, their strength, and the game's rumble; and the vehicle switches (the driver's weapons and mounted guns by hand, the six physical controls as one switch, and the wingsuit's and parachute's arm steering). Its last section shows the file's state (`SETTINGS FILE`) and offers `RELOAD SAVED SETTINGS`, which queues a load on the worker and rebuilds the page from its per-frame hook once the load has finished, and `RESET TO DEFAULTS`, which replaces the page with a confirmation. The confirmation's first, focused button keeps the settings; the second resets everything the file holds except the per-weapon sight calibration, which is measured rather than chosen.

`VR: COMFORT` has the posture, the roomscale chase and its deadzone, the crouch body fold and collision, the seated crouch depth, the head-in-geometry fade, the comfort fade's out and in times, and the vehicle seat transition.

A test puts every value item of these pages through its whole range against a per-thread configuration, checks that each reads back what it wrote and that the file stays valid, and checks that the page's reset returns the defaults.

### Body calibration

`VR: BODY` shows the body file's dimensions and where they came from, the body-scaling switches (`controllers.body.remap_head` and `remap_hands`, saved with the controller settings), a button to reload `body.local.json`, and one to start the calibration, a three-step flow:

1. Height: standing straight and looking ahead. Live readouts show the head's height above the STAGE floor and the height estimated from it.
2. Arm span: a T-pose with the controllers held. Live readouts show the distance between the two grip poses and the arm span estimated from it. A back button returns to step 1.
3. Review: the measured and derived dimensions. `SAVE` writes `body.local.json` (`body::save`) and applies it at once; `RETRY` starts again from step 1; `CANCEL`, on every step, returns to the summary.

A T-pose leaves no hand free to point at the menu, so `MEASURE` starts a countdown of three seconds, then samples for one second; the page's per-frame hook feeds the capture. Step 1 takes the mean of the samples and step 2 the widest. A finished capture moves to the next step and rebuilds the page. The `STATUS` readout shows the countdown, the sampling, the last problem, or whether the measurement is available. `MEASURE` refuses, and the readouts say so, when no VR frame is rendering (`NO HEADSET`), the session has no STAGE space (`NO FLOOR HEIGHT`), the STAGE space cannot be located (`HEADSET NOT TRACKED`), no controller snapshot is published (`NO CONTROLLERS`), or a grip's position is not measured this frame (`LEFT CONTROLLER NOT TRACKED`, and the right). Losing the measurement while sampling abandons the capture; the countdown tolerates it. A result outside the accepted ranges is refused with a message and the step stays. How the dimensions are derived is in [the controllers doc](input/controllers.md#body-dimensions). Leaving the page abandons a calibration in progress. The flow's state machine is `controllers/body/calibration.rs` and is unit-tested.

## Row encoding

`pause_menu/model/rows.rs` encodes each item as the row object one of the movie's widgets draws. Every row sets `m_Type` (its id), `m_Format`, `m_Name`, `m_Value`, `m_DefaultValue` (the reset-all reads it for every row), and `m_NeedConfirm` (false).

| Item | Format | Other fields | Behaviour |
|---|---|---|---|
| Header, Text | key | `m_KeyText` empty, `m_Image` empty, plain `m_Interaction`, not customisable | selecting plays the refusal sound |
| Readout | key | `m_KeyText` is the value, not customisable | as above; the value is live |
| Button | key | `m_KeyText` is the caption, customisable, `m_Action` is the id | selecting sends `get_key_for_action` |
| Toggle | toggle | `m_Labels` `OFF`/`ON`, `m_Values` 0/1 | the option picker |
| Choice | toggle | `m_Labels` the options, `m_Values` their indices | the option picker |
| Slider | slider | `m_MinValue` 0, `m_MaxValue` the notch count | the value is written into the label |

Row ids (`pause_menu/model/ids.rs`) live in a band of their own far above the game's setting types and key actions, and encode the page's registry index and the item's index, so a command for a page no longer shown is recognised and dropped. Values are never negative: the list uses negative values as its own context-menu codes.

## Live values

The list shows a scrolling window of the page on a fixed set of row clips ([`ROW_CLIP_COUNT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::pause_ui::SettingsUI)) under `MCI_settings_screen.MCI_panel.MCI_container.MCI_root.MCI_manager`. Each frame, H4 asks each clip for the row object it shows (`GetData()`), decodes its id, and computes the item's live text: a readout's value, a slider's label with its value, or a button's caption (the list replaces it with `...` when the button is selected, expecting a key capture). `LiveSlots` (`pause_menu/model/live.rs`) remembers what was last written per clip and asks for a write only when the clip's item or text changed; a write sets both the row object's field (`m_KeyText` or `m_Name`, so the list's own redraws show it) and the clip's text field, upper-cased as the list draws it. Any applied value or pressed button invalidates the diff, because the list has redrawn the row from its object. The pass never calls the row's `FillData`, which would restart the highlighted row's animation.

## Status

Nothing here has run in game yet. The pure logic (ids, row encoding, slider mapping, the live diff, the state machine, command routing, the entry selection, the calibration flow and derivation, and the settings pages' value mappings) is unit-tested. Each of the following needs checking in game, in roughly this order:

1. Invoking `AddOption` on the pause clip returns the entry object, and setting its `m_Type` sticks (the entry class is file-private to the movie, with a public field).
2. The VR entries render and highlight like the native ones (label width, the button mask resize).
3. When the settings screen populates, the chosen VR entry's main button reports itself selected, its label reads back as set, and the buttons' `visible` flags read through `GetMember`.
4. Writing the VR page over the graphics slot during the populate is what the screen then shows, and the native graphics page still works afterwards in the same pause.
5. Row objects created as plain objects behave like the native rows, and a key-format row with an empty image and plain interaction draws its key text on the right.
6. The intercepted `set_setting_value` and `get_key_for_action` leave the movie consistent: no key-capture prompt, and the button's caption restored by the next live pass.
7. Y on a VR key-format row shows the reset box, ends in `restore_default_keymap`, and the mod's page reset leaves the game's key bindings alone.
8. Direct text-field writes on row clips survive scrolling and do not fight the rows' own redraws.
9. `PauseUI::UpdateImpl` runs on the capture thread. If it does not, H4 logs one warning and live values and rebuilds never run.
10. The controller pointer drives the VR rows, the slider drag, and the option picker as it drives the native settings.
11. The front-end options menu (from the title screen) gets the entries too, and the pages make sense there.
12. A refresh while a VR page is shown reinstalls it.
13. H1 to H3 pass the capture-thread check (they are expected to; a failure is logged and leaves the menu native).
14. Copying the pause clip with `ObjectAddRef` and releasing the copy leaves the clip's own reference intact (the mod's first use of `ObjectAddRef`).
15. After a rebuild, `MoveSelectionHighlight` and `SetHighlightedRow` leave the first button both highlighted and selected, so the next select activates it.
16. The live pass costs a few dozen value calls per frame while a VR page is shown; check that it does not show in the pause menu's frame time.
17. The VR entries, appended after the native entries, fit within the main buttons, and the options menu opens with graphics highlighted.
18. The page cap assumes the PC options list; the front-end and console layouts were not checked.
19. If `AddOption` fails part-way, the entries already added stay and open the native graphics page (the session stays closed); this is only logged.
20. Toggle and choice rows are not refreshed when their value changes elsewhere (for example from the debug UI) while the page is shown; they show the new value the next time the page opens.
21. The pages' labels fit their rows beside the widgets (the longest are about 26 characters, and slider labels carry their value), the option labels fit the option picker, and the text rows (up to about 37 characters) fit one line.
22. The VR frame loop and the controller snapshot keep running while the pause menu is up, which the calibration's readouts and captures need, and the per-frame hook runs often enough to take the capture's samples (at least five in its one-second window).
23. The eye height the calibration measures is right on the runtimes in use (it depends on the runtime's STAGE floor). The STAGE head height keeps its last value on a frame without a head pose, so a capture during such frames would sample a stale height.
24. After `RELOAD SAVED SETTINGS` or a reset, the rebuild moves the highlight to the page's first row, since the page's first button is past the first screenful.
25. `body::save` writes the body file on the capture thread; it is small, but a slow disk would show as a hitch in the menu.
