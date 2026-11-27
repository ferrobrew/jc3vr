# Tuning

The tuning is the numbers that decide how the mod feels and behaves: gains, thresholds, timings, distances, masses, and budgets. They live in TOML files beside the code that reads them and ship with the mod. The loader is `jc3vr_core::tuning`.

Unlike the player's settings ([settings.md](settings.md)), tuning is versioned with the code, loaded strictly, never written back, and changed only by editing the file.

## What is tuning

A number is tuning when changing it changes how something feels or behaves and nothing else depends on its exact value. A structural number stays in code: an array length, a bit mask, a format version, a register slot, a test fixture, anything the pyxis defs own, and the bounds a `validate` checks. A boolean or enum that is part of a tuned spec goes in the file beside its numbers; a switch that exists only to turn a feature off is not tuning, so hard-code the shipped behaviour and delete it.

## Where the files live

Each layer crate owns its files in a `tuning/` directory beside its `src/` (`crates/jc3vr-vehicles/tuning/vehicles.toml`), and reads only its own and those of crates below it. A file is declared once with `jc3vr_core::tuning_file!`, which makes a static `TuningFile<T>`:

```rust
jc3vr_core::tuning_file! {
    /// The vehicles' tuning file.
    pub static VEHICLES: VehiclesTuning = "vehicles.toml";
}
```

Every struct in the file derives `Deserialize` with `deny_unknown_fields` and no `default`, so a missing or misspelt key fails the load instead of being filled in silently. The root implements `jc3vr_core::tuning::Tuning`, whose `validate` range-checks what the types admit but the code cannot use. A value's meaning is documented on the Rust field, not in the file.

A pure crate shared with the offline tools (`body-rig`) takes its values as parameters, and the layer crate that runs it live owns the file; offline tools parse the repository's file at run time. Recordings do not carry tuning, so `tracking-synth` replays a recording against the tuning as the repository has it now ([body/rig.md](body/rig.md#offline-evaluation)).

## Packaging

The payload's build script copies every `crates/<crate>/tuning/*.toml` to `tuning/<crate>/` beside the built DLL. The payload lists every file in `payload/src/tuning.rs` and loads exactly that list; a test fails when a file on disk is unlisted.

## Startup

The injector's export picks the source (`jc3vr_core::tuning::Source`). `run` reads the packaged files beside the DLL, as a player's install does. `run_source_tuning`, which the injector calls under `--source-tuning` (the scripts in `scripts/` pass it), reads each crate's `tuning/` directory in the checkout the payload was built from, so edits apply live and a worktree build reads its own worktree.

The payload loads the tuning before the crash handler, the settings, and the hooks ([lifecycle.md](lifecycle.md#startup)). Each failure is logged as `tuning: <path>: <cause>`. Any failure stops startup and unloads the payload; there is no fallback to compiled-in values.

## Reading and hot reload

`TuningFile::get` returns `&'static T` for one atomic load, so hot paths read it freely from any thread. Replaced versions are never freed, so a held reference stays valid, and the game thread sees one version per frame. Reading before startup published the file is a programming error and panics.

A watcher thread polls the files (`jc3vr_core::tuning::runtime`), parses and validates a changed one, and stages it for `jc3vr_core::tuning::tick`, which the payload's update runs first each frame. An invalid edit is logged as a warning and the previous values stay.

## Tests

With `jc3vr-core`'s `test-support` feature, which every layer crate's tests enable, the first `get` loads the file from the source tree. A test that needs a variant overrides fields of the shipped root with struct update syntax.
