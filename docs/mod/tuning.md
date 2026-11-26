# Tuning

The tuning is the set of numbers that decide how the mod feels and behaves: gains, thresholds, timings, distances, angles, masses, and budgets. They live in TOML files kept in the repository beside the code that reads them and shipped with the mod, not in the Rust source. The loader is `jc3vr_core::tuning`.

Tuning is not the player's settings ([settings.md](settings.md)). Settings are the player's choices, saved per user, loaded tolerantly, and edited from the pause menu or the debug UI. Tuning is the mod's own values, versioned with the code, loaded strictly, never written back, and with no UI: it changes by editing the file.

## What is tuning

A number is tuning when changing it changes how something feels or behaves and nothing else in the code depends on its exact value: a deadzone, a spring's time constant, a grab radius, a dwell, a cooldown, a placement offset, a frame budget. It goes into a file whether it is written today as a `const`, a `static`, a field of a `const` struct, or a literal inline in an expression.

A number stays in code when it is structural: an array length or a count the types depend on, a bit mask or a flag value, a format or protocol version, a shader register slot or constant-buffer layout, a unit conversion or mathematical constant, a number a test asserts as a fixture, and anything the pyxis defs own (addresses, offsets, enum values, hashes). The bounds a `validate` checks against are code too: they say what the code can use, not what feels right.

A boolean or an enum that is part of a tuned spec (which way a lever travels, whether it holds where it was let go) goes into the file with the numbers beside it. A switch that exists only so a feature can be turned off is not tuning: hard-code the shipped behaviour and delete the switch.

## Where the files live

Each layer crate owns its files, in a `tuning/` directory beside its `src/`: `crates/jc3vr-vehicles/tuning/vehicles.toml`, for example. Ownership follows the crate layering, so a crate reads only its own files and those of crates below it, through their statics. A crate starts with one file named for its subject and splits it by subsystem when it grows past a few hundred lines.

A file is declared once, in the owning crate, with the `jc3vr_core::tuning_file!` macro, which makes a static `TuningFile<T>` whose location comes from the crate's package name and manifest directory:

```rust
jc3vr_core::tuning_file! {
    /// The vehicles' tuning file.
    pub static VEHICLES: VehiclesTuning = "vehicles.toml";
}
```

The root type and every struct under it derive `Deserialize` with `#[serde(deny_unknown_fields)]` and without `#[serde(default)]`: a file must name every key and nothing else, since it ships with the code that reads it, and a missing or misspelt key is a bug to catch at load rather than a value to fill in silently. The root implements `jc3vr_core::tuning::Tuning`, whose `validate` checks what the types admit but the code cannot use, with the configuration engine's range checks (`range`, `positive`, `nonnegative`, `vec3`), naming each value by its path in the file. The field documentation stays on the Rust struct, which is the one place a value's meaning is written down; the file carries values and a header pointing at its root type.

A value the player's settings also shape is composed at the read: the vehicles' `PhysicalControlsTuning::of` joins the wheel's tuning from the file with the lock and the tracking from the settings, through one generic struct (`PhysicalControls<W>`) whose file form and runtime form differ only in the wheel.

A pure crate the payload shares with the offline tools (`body-rig`, for one) does not depend on `jc3vr-core`. Its tuning structs derive `Deserialize` in the pure crate; the layer crate that runs it live owns the file and wraps the root in a newtype that implements `Tuning`, and an offline tool parses the same file from the repository with `toml`.

## Packaging

The payload's build script copies every `crates/<crate>/tuning/*.toml` to `tuning/<crate>/` beside the built DLL, replacing what an earlier build staged so a removed file does not linger. The build's profile directory is the mod's package: the DLL, the injector, the OpenXR loader and ffmpeg the fetch scripts stage there, and the tuning. The payload lists every file in `payload/src/tuning.rs` and loads exactly that list; its test fails when a file on disk is not listed, or when a crate's directory name and package name differ (the staging uses the first, the payload the second).

## Startup

The injector starts the payload with one of two exports, and the export picks where the tuning is read from (`jc3vr_core::tuning::Source`):

- `run` reads the packaged files beside the DLL. This is what a player's injector does.
- `run_source_tuning` reads each crate's `tuning/` directory in the source tree the payload was built from, by the manifest directory the build recorded; a build made on the Linux host names it by a host path, which wine converts. The injector calls it under `--source-tuning`, which `scripts/proton_run.sh` and `scripts/run.ps1` pass. An edit to the repository's file then applies live, and a build injected from a scratch worktree reads that worktree's files, since the path comes from the build itself.

The choice travels with each injection rather than with the game's launch, so a re-inject can switch, and the build and the files it reads always come from the same checkout.

The payload loads the tuning right after logging is installed and before the crash handler, the settings, and the hooks. Every file is read and validated before any is published, and every failure is logged, each as `tuning: <path>: <cause>` (the file is missing, cannot be read, does not match its root, with the TOML parser's position and message, or holds an invalid value). When any fails, the payload logs that its startup stopped and unloads itself from the injector's thread. The injector notices the module gone once its call returns and says so, pointing at the session log. There is no fallback to values compiled into the DLL.

## Reading

`TuningFile::get` returns `&'static T` and costs one acquiring atomic load and a null check, inlined at the call site: on x86 the load is a plain `mov`. The values are published by pointer and a replaced version is never freed, so a reference a caller holds stays valid for the rest of the run, whatever reloads follow. Each reload of a file leaks one root's worth of memory, and reloads are made by hand.

Reading before startup published the file panics in the payload: it is a programming error. The values are read freely from any thread. Each read sees one whole version of the file, and the game thread sees one version for a whole frame, since only its tick publishes; the render thread may see a new version partway through a frame's draws.

## Hot reload

After startup a watcher thread looks at each file a few times a second (`POLL_INTERVAL` in the loader's runtime). A changed file (its modification time or length differs, and its text differs from the last text it applied) is parsed and validated on the watcher. A valid edit is staged and published by `jc3vr_core::tuning::tick`, which the payload's update runs first each frame, and the log says `tuning: reloaded <crate>/<file>` and then `tuning: applied the edit`. An invalid one is logged as a warning in the same `tuning: <path>: <cause>` form and the previous values stay; the next save is tried again. A save caught half-written fails to parse and is retried at the next change. The watcher is stopped and joined at shutdown, before the module unmaps.

## Tests

A test reads the files the repository holds. In a build with `jc3vr-core`'s `test-support` feature, which every layer crate's tests enable, the first `get` of an unpublished file loads it from the source tree; a missing or invalid file panics with the same message the payload would log. Every crate with a file has a test that the shipped file loads and validates. A test that needs a variant copies the shipped root and overrides fields with struct update syntax (`WingsuitGesturesTuning { neutral_dwell_s: 0.125, ..VEHICLES.get().wingsuit_gestures }`); a test that needs a `'static` borrow of a composed value keeps it in a `LazyLock`.
