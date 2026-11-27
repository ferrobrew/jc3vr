# Settings file

The runtime configuration (`Config` in `payload/src/config/`) is saved to one TOML file and loaded from it at startup. It holds every setting the debug UI, the pause menu, and the hotkeys change, and the player's body dimensions. Numbers that set how a feature feels are tuning, not settings ([tuning.md](tuning.md)).

## The aggregate and its slots

`Config` has one table per subsystem, each declared beside its type in the crate that owns it, and the payload's slot table (`config::slots`) binds each to its field. The configuration engine (`crates/jc3vr-core/src/config/`) loads and saves it through the schema the payload registers (`config::schema`), without naming any subsystem.

A subsystem reads and changes its settings through the `ConfigSlot` it declares (`HUD_CONFIG` beside `HudConfig`), so no layer names the aggregate: `lock()` and `edit()` return guards that deref to the group, and `query` holds the lock only for its closure. A slot can name a group nested in another (`WEAPONS_CONFIG` at `controllers.weapons`), and a group a lower layer reads is that layer's type, held by the upper group.

Every guard locks the whole root, so a guard reads one consistent snapshot, but the lock is not reentrant: a thread that locks while holding a guard deadlocks. Two guards in one statement do that, since a temporary guard lives to the end of its statement (`HUD.lock().distance > VR.lock().far_clip`), so copy the first value out before locking again. A debug build panics on the second lock instead of hanging.

## Where a setting is exposed

The pause menu's VR pages ([pause-menu.md](pause-menu.md)) hold only what a player reaches for in the headset: comfort, display, the core controls, and the body calibration. Everything else, each feature's switch, the developer internals, and the diagnostic readouts, is in the [debug UI](debug-ui.md). A feature ships enabled, with its switch there for turning it off, and a setting the pause menu exposes has no copy in the debug UI.

## Where the files live

The files live in the directory `JC3VR_CONFIG_DIR` names, or else the per-user configuration directory that the `directories` crate names for jc3vr (`jc3vr_core::config::paths`); under Proton that is inside the game's prefix, under `AppData/Roaming/philpax/jc3vr/config/`. Beside `settings.toml` the directory holds the debug UI's layout (`ui_layout_v2.json`), the session checklist's marks, the director's teleport bookmarks, and the VR inventory's save sidecar ([input/inventory-persistence.md](input/inventory-persistence.md)); none of them is a setting, and each is written on its own schedule.

## Loading

`config::init` loads the file once, before the hooks are installed, so the first frame's VR initialization sees the saved values. A missing file means defaults, and nothing is written until a setting changes. A file that cannot be read or parsed logs a warning and the defaults are used; the first save renames it to `settings.toml.bad-<session stamp>` so a broken hand edit is kept.

The load (`jc3vr_core::config::load`) is tolerant, so a file from another build or a hand edit costs at most the settings it gets wrong. The file's tree is merged over the serialized defaults, so a missing setting takes its default. Unknown keys are skipped, and a value that cannot be read is replaced by its default. Groups whose types admit values their subsystem cannot use then run their slot's check (`SlotOptions::validated`), which resets the failing setting, or its group when the default fails too. The log lists what was migrated, skipped, and reset.

## The file's format

A save writes only the settings that differ from their defaults, under a `version` key (`jc3vr_core::config::format`, with the payload's migrations in `config::format`), so a setting the player never changed follows a later build's new default. A file without a version, from a build that wrote every setting, is migrated once: numeric settings that hold a since-replaced default (`format::SUPERSEDED`) are removed so they take the current default, and the single-pass stereo switches reset as a group ([stereo/single-pass-stereo.md](stereo/single-pass-stereo.md#turning-on-and-off)). The old file is kept as `settings.toml.v0-<session stamp>`.

## Saving

A change the user meant goes through an edit guard (a slot's `edit()`, or `jc3vr_core::config::edit_parts()` across groups), which marks the configuration changed. Reads and changes the mod makes on its own take a plain lock and never cause a save. A fallback that switches off a user's choice (the mirror after a lost device, the HUD redirect at teardown) goes through `jc3vr_core::config::disable_automatically`: the live setting turns off, but saves keep the user's value until the user changes it, so the fallback never becomes permanent.

`config::tick` runs at the end of the payload's update. It saves after changes have been quiet for a while or a maximum delay has passed (the core tuning's `settings_save` table), writes only when the serialized text differs from the file's, and writes on a short-lived thread through a temporary file moved over the original, so a failed write never truncates the file. A failed save is retried with backoff. On shutdown, `config::shutdown` writes any change still settling.

Session state rather than choices is `#[serde(skip)]`: the director's time scale, pause, and clean view, the VR pose freeze, and the debug panel's visibility, so a launch never starts frozen or with the panel up.

TOML has no null, so an optional setting defaults to `None`, and a setting whose value is kept while switched off is a switch beside the value (`fsr.sharpen` and `fsr.sharpness`) rather than an `Option`.
