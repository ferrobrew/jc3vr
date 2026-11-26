//! The tuning: the numbers that set how the mod feels and behaves (gains, thresholds, timings,
//! distances, angles, masses, and budgets), read from TOML files the mod ships rather than written
//! into the code. `docs/mod/tuning.md` has the design; this module is its loader.
//!
//! Each layer crate owns its files, kept in its `tuning/` directory beside its sources, and declares
//! each with [`tuning_file!`](crate::tuning_file) as a static [`TuningFile`] of a typed root that
//! implements [`Tuning`]. The payload lists every file and hands the list to [`init`] at startup,
//! which reads them all and refuses to start when any is missing or invalid. A file is strict: it
//! must name every key of its root and nothing else, since the files are versioned with the code
//! that reads them. Tuning is not the player's settings: nothing writes it back, and it has no UI.
//!
//! The files are read from one of two places ([`Source`]): the `tuning/` directory beside the
//! payload's DLL, where the build stages them, or, for development, the source tree the payload was
//! built from, so an edit to the repository's file applies live.
//!
//! After startup a watcher thread polls the files. A valid edit is parsed and validated on the
//! watcher and published at the game thread's next [`tick`]; an invalid one is logged and the
//! previous values stay. A read ([`TuningFile::get`]) is one atomic load: the values are published
//! by pointer and never freed, so the reference a read returns stays valid for the rest of the run.

mod error;
mod file;
mod runtime;
mod source;

pub use crate::tuning::{
    error::{FileCause, StartupError, TuningError},
    file::{AnyTuningFile, Location, Tuning, TuningFile},
    runtime::{init, shutdown, tick},
    source::Source,
};

/// Declares a static [`TuningFile`] for a file in the calling crate's `tuning/` directory.
///
/// ```text
/// jc3vr_core::tuning_file! {
///     /// The vehicles' tuning.
///     pub static VEHICLES: VehiclesTuning = "vehicles.toml";
/// }
/// ```
///
/// The file's owner and its place in the source tree come from the calling crate's package name
/// and manifest directory, so the macro must be invoked in the crate that owns the file.
#[macro_export]
macro_rules! tuning_file {
    ($(#[$meta:meta])* $vis:vis static $name:ident: $ty:ty = $file:literal;) => {
        $(#[$meta])*
        $vis static $name: $crate::tuning::TuningFile<$ty> =
            $crate::tuning::TuningFile::new($crate::tuning::Location {
                crate_name: env!("CARGO_PKG_NAME"),
                file_name: $file,
                manifest_dir: env!("CARGO_MANIFEST_DIR"),
            });
    };
}

#[cfg(test)]
mod tests;
