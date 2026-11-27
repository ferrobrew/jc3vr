# Settings file

The runtime configuration (`Config` in `payload/src/config/`) is saved to one TOML file and loaded from it at startup. Every setting the debug UI, the pause menu's VR pages, and the hotkeys change lives there, along with the player's body dimensions and the director's key bindings. Numbers that set how a feature feels are not settings: they are tuning, shipped beside the DLL ([tuning.md](tuning.md)).

## The aggregate and its slots

`Config` has one table per subsystem, each declared beside its type in the crate that owns it, and the payload's slot table (`config::slots`) binds each slot to its field: `stereo`, `foveation`, `fsr`, `camera`, `headpose`, and `vr` (view); `exposure` and `post_fx` (render); `controllers` and `online` (gameplay); `hud` (HUD); `director`, `recorder`, and `testing` (tooling); and `body`, the body dimensions (body).

The configuration engine (`crates/jc3vr-core/src/config/`) holds the configuration, loads it, and saves it without naming any subsystem's settings. It keeps one type-erased root behind one lock and works through the schema the payload registers for `Config` (`config::schema`): the defaults, the serialization, and the migrations of older files. The payload registers it on the first access through any of its own paths, so the configuration reads as its defaults from the first lock.

A `ConfigSlot` names one group of the configuration. Each subsystem reads and changes its own settings through the slot it declares (`HUD_CONFIG` beside `HudConfig`), so no layer names the aggregate: `lock()` and `edit()` return guards that deref to the group, and `query(|hud| ...)` holds the lock only for its closure. A slot can name a group nested in another: the controllers' settings are one `controllers` table, each controller subsystem declares the slot for its own group in it (`WEAPONS_CONFIG` at `controllers.weapons`), and only the bindings, the controls summary, and the layers above them read the whole table (`CONTROLLERS_CONFIG`). A group a lower layer reads is that layer's type, held by the upper group (`DRIVING_VIEW_CONFIG` at `controllers.vehicles.driving_view` is the body crate's). Where the file keeps one setting directly in an upper group, the lowest reader declares a slot onto that setting alone (`CONTROLLERS_ENABLED` at `controllers.enabled`, in the view crate). A group binds the slots nested in it relative to its own (`SlotOptions::with_children` with `slot_children!`), so binding a slot binds everything under it.

A crate's tests register a root of their own with `test_root!`, listing only the slots the crate reads; each part takes its slot's default, and a payload test asserts that every slot's default is what the aggregate holds there.

### Lock nesting

Every guard locks the whole root, so what one guard reads is one consistent snapshot. The lock is not reentrant and the slots share it, so a thread that locks the configuration while it holds a guard deadlocks. Two guards in one statement do that, since a temporary guard lives to the end of its statement: `HUD.lock().distance > VR.lock().far_clip` never finishes. Copy the first value out (`let distance = HUD.lock().distance;`) before locking again, or lock a slot that holds both. A tick that reads several groups copies them one at a time into its own snapshot (`WeaponSettings::read`). A debug build turns a second lock on the same thread into a panic that says this, rather than a hang.

## Where a setting is exposed

The pause menu's VR pages ([pause-menu.md](pause-menu.md)) hold only the essentials a player reaches for in the headset: comfort, display, the core controls, and the body calibration. Everything else is in the [debug UI](debug-ui.md): each feature's switch, the internals that need a developer to judge, and the diagnostic readouts. A feature ships enabled by default, and its switch is there for turning it off. A setting the pause menu exposes has no copy in the debug UI.

## Where the files live

The payload keeps its files in the per-user configuration directory that the `directories` crate names for jc3vr (`jc3vr_core::config::paths`). On Windows that is `%APPDATA%\philpax\jc3vr\config`; under Proton it is inside the game's prefix, `steamapps/compatdata/<app id>/pfx/drive_c/users/steamuser/AppData/Roaming/philpax/jc3vr/config/`. The directory is created on first use. The startup log names the settings file's path, and the debug window shows it under the build stamp with whether the file is up to date.

| File | Holds | Written |
| --- | --- | --- |
| `settings.toml` | The runtime configuration | When a setting changes ([saving](#saving)) |
| `ui_layout_v2.json` | The debug UI's feature selection and dock layout | By the layout writer ([debug-ui.md](debug-ui.md#layout-persistence)) |
| `session_checklist.toml` | The marks on the session checklist | When a check is marked ([debug-ui.md](debug-ui.md#session-checklist)) |
| `teleports.toml` | The director's teleport bookmarks | When a bookmark is saved or removed, unless the file could not be read or parsed: then it is left untouched and the error shows in the Director feature |
| `inventory.toml` | The VR inventory's save sidecar, tied to the game's save ([input/two-handed-inventory.md](input/two-handed-inventory.md#saves-resets-and-loads)) | Each time the game stores the player's save data |

None of the other files is a setting, and each is rewritten on its own schedule. The learned body estimator's weights are read from `controllers.body.estimator.weights_path`, or by default from `egoposer.safetensors` beside the payload DLL ([body/body-estimator.md](body/body-estimator.md#the-weights)). Per-session output goes to the session folder ([sessions.md](sessions.md)).

## Loading

`config::init` loads the file once at startup, before the hooks are installed, so the first frame's VR initialization (which reads the dominant hand and the controller layout) sees the saved values. A missing file is the first run: the defaults are used and nothing is written until a setting changes. A file that cannot be read or is not TOML logs a warning and the defaults are used; the first save then renames it to `settings.toml.bad-<session stamp>` before writing the new one, so a hand edit that broke the file is kept. If the rename fails, the save fails and is retried.

The load (`jc3vr_core::config::load`) is tolerant, so a file written by another build or edited by hand costs at most the settings it gets wrong:

1. The file is parsed into a TOML tree, its format `version` is taken out, and a format-zero file is migrated ([below](#the-files-format)). The tree is merged over the serialized defaults, so a setting the file lacks takes its default. Lists merge element by element when the file's list has the default's length and otherwise replace the default whole, so a list that grows or shrinks, such as the key bindings, loads as written.
2. The tree is deserialized. Unknown keys are skipped. A value that cannot be read (the wrong type, an unknown enum variant) is replaced by its default, or removed when it has none, and the deserialization is retried, up to a fixed limit before the whole file gives way to the defaults.
3. The groups whose types admit values their subsystem cannot use run the checks their slots carry (`SlotOptions::validated`), in the slot table's order: the controller settings (`controllers::config::validate` in the gameplay crate, with the per-weapon sight corrections checked on their own), the VR turn settings in `headpose`, the HUD, and the body dimensions. A check names the setting that fails, that setting is reset to its default, and the check runs again; when the default fails too (a setting checked against another), the group that holds it is reset. The sight corrections and the body dimensions reset as a group, and a failure elsewhere in the controller settings keeps the sight corrections, since they are measured rather than chosen.

The log lists what the load migrated, skipped, and reset, with the reason, against the file's path. A file a newer build wrote loads as this build's format, with a warning. Renaming a setting loses its saved value once, which the log reports.

## The file's format

A save writes only the settings that differ from their defaults, under a `version` key (`jc3vr_core::config::format`; the payload's migrations are in `config::format`). A setting left at its default is absent from the file, so when a later build changes the default, the setting follows it, while a value the player set stays as written. A list that differs from its default is written whole. The price is that a setting the player sets back to its default follows a later change of the default too.

A file without a version is format zero, from a build that wrote every setting, so its values that equal an old default cannot be told from choices and would stay stuck when the default changed (a walking leg-flip came from exactly that). The load removes each numeric setting that holds a default a later build replaced (`format::SUPERSEDED`, and the director's old key bindings), so it takes the current default; matching an exact old number is strong evidence that the player never touched it. A switch or a mode is kept as written even when it holds an old default, since a player who set it back on purpose cannot be told from one who never touched it. The single-pass stereo group is the exception: its switches only work together, so the load removes the ones a format-zero file holds off (`format::migrate_single_pass_group`) and the group takes its default as a whole ([stereo/single-pass-stereo.md](stereo/single-pass-stereo.md#turning-on-and-off)). The first tick after such a load rewrites the file in the current format, after keeping a copy of the old file as `settings.toml.v0-<session stamp>`; if the copy fails, the save fails and is retried. A default changed from now on needs no migration entry.

## Saving

A change the user meant goes through an edit guard: a slot's `edit()`, or `jc3vr_core::config::edit_parts()` for a change that spans several groups. The guard marks the configuration changed when it drops. Debug UI controls, the pause menu's VR pages, the hotkeys, the body calibration, the sight tuning tool's capture, and the director's key binding reset all use one. Reads, and changes the payload makes on its own (a value discovered at runtime, a diagnostic that disarms itself), take a plain lock (`lock()`, `query()`, `lock_parts()`, or the payload's `CONFIG.lock()`) and are never a reason to save. The capture harness, which pins a profile across groups and puts the settings back afterwards, copies the configuration into a `jc3vr_core::config::Snapshot` and restores it.

A fallback that switches off a setting the user chose (the mirror after a removed device, native resolution after a resize fault, the mod-owned back buffer after a failed substitution, and the HUD redirect at teardown) goes through `jc3vr_core::config::disable_automatically`, naming the setting by its slot and field (`switch!(VR_CONFIG, mirror)`). The live setting turns off at once, but a save writes the user's value until the user changes that setting through an edit guard, so a later save for an unrelated change does not make the fallback permanent.

`config::tick` runs once per frame on the game thread at the end of `crate::update`, after the debug UI. It saves once the changes have been quiet for `settings_save.quiet_ms`, or `settings_save.max_delay_ms` after the first unsaved change, whichever comes first (the core tuning, [tuning.md](tuning.md)); an open debug UI feature edits every frame, so the second bound is what saves while one is open. A save serializes a snapshot and compares it with the text the file last received, at most once per `settings_save.compare_interval_ms`, and writes only a difference, so an open feature costs a serialization now and then and no disk traffic. The write runs on a short-lived writer thread that writes a temporary file beside the settings file and moves it over the original, so a failed write never truncates the file. The writes are numbered, and a writer that finds a newer save already written skips its own. A failed save is retried after `settings_save.retry_first_ms`, then after a delay that doubles up to `settings_save.retry_max_ms`.

The debug window's header shows the file's state: saved, saving, save failed, or not saved without a configuration directory. A change still settling reads as saving only while it differs from the file, and a failure outranks a waiting change.

On shutdown, `config::shutdown` stops new saves, waits for the writers (keeping the payload mapped if one does not finish), and writes from the shutdown thread a change still settling or the settings whose last save failed.

## What is not saved

Fields that hold state for the session rather than a choice are `#[serde(skip)]`: they load as their defaults, and a key for them in the file is skipped and reported. They are the director's time scale, pause, and clean view; the VR pose freeze (`vr.freeze_mode`), so a launch never starts with the view locked; and the debug panel's visibility (`hud.egui_panel.enabled`), so a session that left the panel up does not open it at the next launch.

## TOML and the configuration types

TOML has no null, so a `None` is left out of the file and an optional setting defaults to `None`: a missing key loads as `None` and a present one as `Some`. A setting whose value is kept while it is switched off is a switch beside the value instead of an `Option` (`fsr.sharpen` and `fsr.sharpness`), since a saved `None` would otherwise load as a default `Some`. A list cannot hold an empty slot either, so the per-weapon sight table, a fixed array of optional entries in memory, is saved as a list of the corrections it holds. A constant default that needs owned data (the key bindings) is a `Cow` that borrows a constant until it is edited, which keeps `Config::new` a `const fn`. Floats keep their non-finite values through the file.
