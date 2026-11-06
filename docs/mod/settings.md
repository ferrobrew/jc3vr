# Settings file

The runtime configuration (`Config` in `payload/src/config/`, behind the `CONFIG` mutex) is saved to one TOML file and loaded from it at startup. Every setting the debug UI, the pause menu's VR pages, and the hotkeys change lives there, along with the player's body dimensions and the director's key bindings.

## Where a setting is exposed

The pause menu's VR pages ([pause-menu.md](pause-menu.md)) hold only the essentials a player reaches for in the headset: comfort (the recenter, the posture, turning, the stick movement, the view on the grapple, the fade when the head is in a wall, and the view in vehicles), the display (the resolution, the sharpening, the foveation, the panel), the core controls (the hand, movement, and the few weapon, haptic, and vehicle settings that depend on the player's body or taste, such as a steering lock or a haptic strength), and the body calibration. Everything else is in the debug UI: each feature's switch, its tuning, the internals (offsets, gains, thresholds that need a developer to judge), and the diagnostic readouts, the status rows among them ([status.md](status.md)). A feature ships enabled by default, so a player meets it without reaching for the debug UI, and its switch is there for turning it off.

## Debug UI gameplay overlay

The debug window's top bar contains `Allow gameplay input`. Opening the panel requests Editing. Enabling gameplay input requests a read-only overlay: live values remain visible, but pointer clicks, controller triggers, panel grip dragging, keyboard input, text, clipboard input, and scroll cannot edit controls. The game does not automatically pause or unpause. Hiding and reopening through the existing panel shortcut returns to Editing; there is no separate return-to-edit chord. F4 toggles the clean view, which at its desktop scope controls desktop-overlay visibility ([hud.md](hud.md)), and F8 or the controller panel shortcut controls the floating panel.

The header reports the requested mode separately from effective input ownership. A focus, tracking/recovery, neutral-input, or game-device acknowledgement blocker remains visible instead of reporting that gameplay is active. Editing saves the prior game-device state, and releasing editing restores that state, including a previously disabled device. Restoration never forces the device enabled while the application lacks focus. Ownership transitions clear queued events and egui's remembered pointer/key state, end panel dragging, and wait for a destination-device acknowledgement followed by a neutral input sample. The input tick must consume the held source episodes during that wait; filtering egui events alone does not establish gameplay safety.

The overlay state belongs to the current editing session. `Allow gameplay input` is not a saved configuration field, does not enable motion controllers or bindings, and does not override native menus or input recovery. The feature navigation and detached views share the disabled editing boundary. Visualization settings remain unchanged by input-mode and page changes. Layout persistence is described in [debug-ui.md](debug-ui.md).

The capture adapter and header are implemented in `payload/src/egui_impl.rs`, `controllers/mode.rs`, and `ui/mod.rs`. The production adapter notifies the input owner, observes neutral input, gates gameplay and controller-pointer input, and handles desktop F4 visibility separately from the VR panel. Renderer-free tests cover these ownership transitions. No headset verification is recorded.

## Where the files live

The payload keeps its files in the per-user configuration directory that the `directories` crate names for jc3vr (`ProjectDirs::from("me", "philpax", "jc3vr")`, see `config::paths`). On Windows that is `%APPDATA%\philpax\jc3vr\config`. Under Proton it is inside the game's prefix, `steamapps/compatdata/<app id>/pfx/drive_c/users/steamuser/AppData/Roaming/philpax/jc3vr/config/`. The directory is created on first use. The startup log names the settings file's path, and the debug window shows it under the build stamp as selectable text, with whether the file is up to date.

| File | Holds | Written |
| --- | --- | --- |
| `settings.toml` | The whole runtime configuration | When a setting changes, as described below |
| `ui_layout_v2.json` | The debug UI's feature selection and dock layout | One owned coalescing writer performs atomic replacement outside UI locks; snapshots are checked every 60 editing UI frames |
| `teleports.toml` | The director's teleport bookmarks | When a bookmark is saved or removed |
| `inventory.toml` | The VR inventory's save sidecar: the weapons carried beyond the slots, tied to the game's save ([input/two-handed-inventory.md](input/two-handed-inventory.md#saves-resets-and-loads)) | Each time the game stores the player's save data |

The dock layout, the bookmarks, and the inventory sidecar stay in files of their own: none is a setting, and each is rewritten on its own schedule. The per-session output (logs, traces, screenshots, recordings) goes to the session folder ([sessions.md](sessions.md)), not here.

The body dimensions are the `[body]` section of `settings.toml` and the director's key bindings are `director.keymap`.

## Loading

`config::init` loads the file once, from `initialize_startup` before the hooks are installed, so the first frame's VR initialization (the dominant hand and the controller layout among the settings it reads) sees the saved values. A missing file is the first run: the defaults are used and nothing is written until a setting changes. A file that cannot be read or is not TOML logs a warning, the defaults are used, and the file is left alone until a setting changes. The first save then renames it to `settings.toml.bad-<timestamp>` beside it (the session stamp, logged with the path) before writing the new file, so a hand edit that broke the file is kept rather than replaced by the defaults; if the rename fails, the save fails and is retried, and the file stays as it is.

The load is tolerant, so a file written by an older or newer build, or edited by hand, costs at most the settings it gets wrong (`config::load`):

1. The file is parsed into a TOML value tree, and its format's `version` is taken out ([below](#the-files-format)); a file of format zero loses the defaults a later build replaced. The tree is merged over the defaults serialized the same way. A setting the file lacks takes its default. A key the defaults lack is kept for the next step, since it is either an optional setting or one this build does not know.
2. The tree is deserialized. Keys this build does not know are skipped. When a value cannot be read (the wrong type, an unknown enum variant, a list of the wrong length), the error's path names it; that one value is replaced by its default, or removed when it has none, and the deserialization is tried again, up to 64 times before the whole file gives way to the defaults.
3. The settings whose types admit values the subsystems cannot use are validated (`config::validate`): the controller settings, the per-weapon sight corrections, the VR turn settings, the HUD surfaces and panel geometry, and the body dimensions. HUD axes, distances, and total crop dimensions must be finite and positive, and HUD and egui texture dimensions must fit the Direct3D 11 axis limit. Crop sides are bounded and nonnegative. Supported smoothing and fade durations may be zero for immediate motion or no fade; load validation preserves those zero values. The controller, HUD, and VR turn checks name the setting that fails, and that one setting is reset to its default and the check run again; when the default fails too (a setting checked against another, such as a far reach below the near one), the group that holds it is reset instead, and only a failure that names nothing resettable resets the whole controller or turn settings. The sight corrections and the body dimensions are reset as a group. The sight corrections are measured rather than chosen, so a failure elsewhere in the controller settings keeps them.

The log lists what a file of format zero defaulted (info), each setting the load migrated with the value it removed, and what it skipped and reset with the reason (warnings), against the file's path. A file a newer build wrote loads as this build's format, with a warning. Renaming a setting loses its saved value once, which the log reports.

Lists merge element by element when the file's list has the default's length, and otherwise replace the default whole, so a list that can grow or shrink, such as the key bindings, loads as written.

The input threshold editor changes each press/release pair together. Lowering press below release lowers release; raising release above press raises press. The existing threshold validator admits the result before publication. Chord-window sliders keep a positive duration, matching load validation.

## The file's format

A save writes only the settings that differ from their defaults, under a `version` key at the top of the file (`config::format`, format 1). A setting left at its default is absent from the file, so when a later build changes the default, the setting follows it, while a value the player set stays as written. A list that differs from its default is written whole. An unchanged configuration is a file holding only the version.

A file without a version is format zero, from a build that wrote every setting. Its values that equal their defaults are indistinguishable from choices, so a default changed since would stay stuck at its old value: the player's file from 6 November held `controllers.body.rig.yaw.moving_deadzone_deg` at 15, the default before it became 25, and the legs flipped while walking. The load removes each numeric tuning of such a file that holds a default a later build replaced (`format::SUPERSEDED`, gathered from the history of the defaults since the file was introduced, and the collider view's old key binding), so it takes the current default, and logs each with the value it removed. Matching an exact old number is strong evidence that the player never touched it. A switch or a mode is kept as written even when it holds an old default, since a player who set it back on purpose (the weapon wheel turned on after its default became off, say) cannot be told from one who never touched it, and removing it would lose the choice at the next save. A value that differs from both the old and the new default is kept. The first tick after the load rewrites the file in the current format without waiting for a change, so the migration runs once; before that write a copy of the old file is kept beside it as `settings.toml.v0-<timestamp>` (the session stamp, logged with the path), and if the copy fails, the save fails and is retried, and the file stays as it is. A default changed from now on needs no entry, since the current format writes no defaults; the price is that a setting the player sets back to its default follows a later change of the default too.

## Saving

A change the user meant goes through `config::edit()`, a guard that derefs to the `Config` and marks the configuration changed when it drops. Every debug UI feature that binds controls to the configuration, the pause menu's VR pages, the hotkeys (F4, F8, F10, F11, and the director's key bindings), the body calibration's save, the sight tuning tool's capture, and the director tab's key binding reset use it. Reads, and changes the payload makes on its own, take the plain `CONFIG` lock and are never a reason to save: a value discovered at runtime, a diagnostic that disarms itself, or a fallback.

A fallback that switches off a setting the user chose (the mirror after a removed device, native resolution after a resize fault, the mod-owned back buffer after a failed substitution, and the HUD redirect at teardown) goes through `config::disable_automatically`. The live setting turns off at once, but a save writes the user's value until the user changes that setting through `edit()`; otherwise a later save for an unrelated change would make the fallback permanent.

The save runs from `config::tick`, called once per frame on the game thread at the end of `crate::update`, after the debug UI. It saves once the changes have been quiet for a second, or five seconds after the first unsaved change, whichever comes first; an open debug UI feature edits every frame, so the second bound is what saves while one is open. A save serializes a snapshot in the file's format and compares it with the text the file last received or was loaded as, and writes only a difference, so an open tab costs about one serialization a second (with the readout's comparison below) and no disk traffic. The write itself runs on a short-lived writer thread, which writes a temporary file beside the settings file and moves it over the original, so a failed write never truncates the file. The writes are serialized and numbered, and a writer that finds a newer save already written skips its own. A failed save is logged and tried again two seconds later, then after a delay that doubles with each further failure up to a minute, whether or not anything changes in between.

On shutdown (`shutdown_startup`), `config::shutdown` stops new saves, waits for the writers (keeping the payload mapped if one does not finish), and writes from the shutdown thread a change still waiting to settle, or the settings whose last save failed.

The debug window shows the file's state (`UP TO DATE`, `SAVING`, `FAILED, SEE LOG`, or `NOT SAVED` without a configuration directory), as does the Status page's `SETTINGS FILE` row. A change still settling reads as `SAVING` only while it differs from the file: `config::tick` compares the settings with the file's text at most once a second while a change is waiting, so an open debug UI feature, which marks a change every frame, reads as `UP TO DATE` when nothing it shows has moved. A failure outranks a waiting change, so the state reads as failed until a save succeeds.

## What is not saved

Fields that hold state for the session rather than a choice are `#[serde(skip)]`: they load as their defaults and a key for them in the file is skipped and reported. They are the director's time scale, pause, and clean view scope; the VR pose freeze (`vr.freeze_mode`), so a launch never starts with the view locked; and the render diagnostics armed for one capture (`stereo.skip_pass_range_enabled`, `stereo.auto_trace_brightness_step`, and `stereo.single_pass.dump_vs_name_census`).

## TOML and the configuration types

TOML has no null, so a `None` is left out of the file. An optional setting therefore defaults to `None`: its key is absent from the defaults, so a missing key loads as `None` and a present one as `Some`. A setting that is on by default with a value is a switch beside the value instead of an `Option` (`fsr.sharpen` and `fsr.sharpness`), since a saved `None` would otherwise load as the default `Some`.

A list cannot hold an empty slot either, so the per-weapon sight table, a fixed array of optional entries in memory, is saved as a list of the corrections it holds. A constant default that needs owned data (the far field's gated type list, the key bindings) is a `Cow` that borrows a constant until it is edited, which keeps `Config::new` a `const fn`.

Floats keep their non-finite values (`nan`, `inf`) through the file, and integers are TOML's signed 64-bit; no setting comes near that range.
