# Settings file

The runtime configuration (`Config` in `payload/src/config/`, behind the `CONFIG` mutex) is saved to one TOML file and loaded from it at startup. Every setting the debug UI, the pause menu's VR pages, and the hotkeys change lives there, along with the player's body dimensions and the director's key bindings.

## Where a setting is exposed

A setting a player would change belongs in the pause menu's VR pages ([pause-menu.md](pause-menu.md)): comfort, controls, handedness, the switch for each gameplay feature, and the tuning of a feature that depends on the player's body or taste, such as a steering lock, a haptic strength, or how far the arms move to steer the wingsuit. The headset is on while the game is played, so a setting that only the debug UI reaches is out of a player's reach. The debug UI holds the internals: offsets, gains, thresholds that need a developer to judge, and diagnostics. A feature lands with its pause menu entries, the same way it lands with its documentation. Experiments are the exception: the sim-rate experiment (`sim_rate.rate`, [performance/sim-rate.md](performance/sim-rate.md)) defaults to the game's own behaviour and only the debug UI reaches it.

## Where the files live

The payload keeps its files in the per-user configuration directory that the `directories` crate names for jc3vr (`ProjectDirs::from("me", "philpax", "jc3vr")`, see `config::paths`). On Windows that is `%APPDATA%\philpax\jc3vr\config`. Under Proton it is inside the game's prefix, `steamapps/compatdata/<app id>/pfx/drive_c/users/steamuser/AppData/Roaming/philpax/jc3vr/config/`. The directory is created on first use. The startup log names the settings file's path, and the debug window shows it under the build stamp as selectable text, with whether the file is up to date.

| File | Holds | Written |
| --- | --- | --- |
| `settings.toml` | The whole runtime configuration | When a setting changes, as described below |
| `ui_layout.json` | The debug UI's dock layout | When the layout changes, checked every 60 UI frames |
| `teleports.toml` | The director's teleport bookmarks | When a bookmark is saved or removed |
| `inventory.toml` | The VR inventory's save sidecar: the weapons carried beyond the slots, tied to the game's save ([input/vr-inventory.md](input/vr-inventory.md#across-resets-saves-and-loads)) | Each time the game stores the player's save data |

The dock layout, the bookmarks, and the inventory sidecar stay in files of their own: none is a setting, and each is rewritten on its own schedule. The per-session output (logs, traces, screenshots, recordings) goes to the session folder, not here.

The body dimensions are the `[body]` section of `settings.toml` and the director's key bindings are `director.keymap`. The `body.local.json`, `controllers.local.json`, `director_keymap.json`, and `director_bookmarks.json` files earlier builds kept beside the payload DLL are no longer read; copy any values worth keeping into the new files by hand.

## Loading

`config::init` loads the file once, from `initialize_startup` before the hooks are installed, so the first frame's VR initialization (the dominant hand and the controller layout among the settings it reads) sees the saved values. A missing file is the first run: the defaults are used and nothing is written until a setting changes. A file that cannot be read or is not TOML logs a warning, the defaults are used, and the file is left alone until a setting changes. The first save then renames it to `settings.toml.bad-<timestamp>` beside it (the session stamp, logged with the path) before writing the new file, so a hand edit that broke the file is kept rather than replaced by the defaults; if the rename fails, the save fails and is retried, and the file stays as it is.

The load is tolerant, so a file written by an older or newer build, or edited by hand, costs at most the settings it gets wrong (`config::load`):

1. The file is parsed into a TOML value tree and merged over the defaults serialized the same way. A setting the file lacks takes its default. A key the defaults lack is kept for the next step, since it is either an optional setting or one this build does not know.
2. The tree is deserialized. Keys this build does not know are skipped. When a value cannot be read (the wrong type, an unknown enum variant, a list of the wrong length), the error's path names it; that one value is replaced by its default, or removed when it has none, and the deserialization is tried again, up to 64 times before the whole file gives way to the defaults.
3. The settings whose types admit values the subsystems cannot use are validated (`config::validate`): the controller settings, the per-weapon sight corrections, the VR turn settings, and the body dimensions. The controller and VR turn checks name the setting that fails, and that one setting is reset to its default and the check run again; when the default fails too (a setting checked against another, such as a far reach below the near one), the group that holds it is reset instead, and only a failure that names nothing resettable resets the whole controller or turn settings. The sight corrections and the body dimensions are reset as a group. The sight corrections are measured rather than chosen, so a failure elsewhere in the controller settings keeps them.

The log lists what the load defaulted (info), skipped, and reset with the reason (warnings), against the file's path. There is no version field and no migration: renaming a setting loses its saved value once, which the log reports.

Lists merge element by element when the file's list has the default's length, and otherwise replace the default whole, so a list that can grow or shrink, such as the key bindings, loads as written.

## Saving

A change the user meant goes through `config::edit()`, a guard that derefs to the `Config` and marks the configuration changed when it drops. Every debug UI tab that binds controls to the configuration, the pause menu's VR pages, the hotkeys (F4, F8, F11, and the director's key bindings), the body calibration's save, the sight tuning tool's capture, and the director tab's key binding reset use it. Reads, and changes the payload makes on its own, take the plain `CONFIG` lock and are never a reason to save: a value discovered at runtime, a diagnostic that disarms itself, or a fallback.

A fallback that switches off a setting the user chose (the mirror after a removed device, native resolution after a resize fault, the mod-owned back buffer after a failed substitution, and the HUD redirect at teardown) goes through `config::disable_automatically`. The live setting turns off at once, but a save writes the user's value until the user changes that setting through `edit()`; otherwise a later save for an unrelated change would make the fallback permanent.

The save runs from `config::tick`, called once per frame on the game thread at the end of `crate::update`, after the debug UI. It saves once the changes have been quiet for a second, or five seconds after the first unsaved change, whichever comes first; an open debug UI tab edits every frame, so the second bound is what saves while one is open. A save serializes a snapshot and compares it with the text the file last received or was loaded as, and writes only a difference, so an open tab costs about one serialization a second (with the readout's comparison below) and no disk traffic. The write itself runs on a short-lived writer thread, which writes a temporary file beside the settings file and moves it over the original, so a failed write never truncates the file. The writes are serialized and numbered, and a writer that finds a newer save already written skips its own. A failed save is logged and tried again two seconds later, then after a delay that doubles with each further failure up to a minute, whether or not anything changes in between.

On shutdown (`shutdown_startup`), `config::shutdown` stops new saves, waits for the writers (keeping the payload mapped if one does not finish), and writes from the shutdown thread a change still waiting to settle, or the settings whose last save failed.

The pause menu's `VR: CONTROLS` page shows the file's state (`UP TO DATE`, `SAVING`, `FAILED, SEE LOG`, or `NOT SAVED` without a configuration directory), as does the debug window. A change still settling reads as `SAVING` only while it differs from the file: `config::tick` compares the settings with the file's text at most once a second while a change is waiting, so an open debug UI tab, which marks a change every frame, reads as `UP TO DATE` when nothing it shows has moved. A failure outranks a waiting change, so the state reads as failed until a save succeeds.

## What is not saved

Fields that hold state for the session rather than a choice are `#[serde(skip)]`: they load as their defaults and a key for them in the file is skipped and reported. They are the director's time scale, pause, and cinematic mode; the HUD split's discovered clip-path prefix (`hud.split_path_prefix`); the VR pose freeze (`vr.freeze_mode`), so a launch never starts with the view locked; the sim-rate experiment's tick rate (`sim_rate.rate`), so a session never starts at an experimental rate; and the render diagnostics armed for one capture (`stereo.skip_pass_range_enabled`, `stereo.auto_trace_brightness_step`, and `stereo.single_pass.dump_vs_name_census`).

## TOML and the configuration types

TOML has no null, so a `None` is left out of the file. An optional setting therefore defaults to `None`: its key is absent from the defaults, so a missing key loads as `None` and a present one as `Some`. A setting that is on by default with a value is a switch beside the value instead of an `Option` (`fsr.sharpen` and `fsr.sharpness`), since a saved `None` would otherwise load as the default `Some`.

A list cannot hold an empty slot either, so the per-weapon sight table, a fixed array of optional entries in memory, is saved as a list of the corrections it holds. A constant default that needs owned data (the far field's gated type list, the key bindings) is a `Cow` that borrows a constant until it is edited, which keeps `Config::new` a `const fn`.

Floats keep their non-finite values (`nan`, `inf`) through the file, and integers are TOML's signed 64-bit; no setting comes near that range.
