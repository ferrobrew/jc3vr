//! A tuning file's typed root, its location, and the published values a read returns.

use std::{
    fmt,
    path::{Path, PathBuf},
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use serde::de::DeserializeOwned;

use crate::{config::engine::Invalid, tuning::error::FileCause};

/// A tuning file's root: the values one file holds.
///
/// The root derives `Deserialize` with `#[serde(deny_unknown_fields)]` on it and every struct
/// beneath it, and without `#[serde(default)]`, so a file that misses a key or names one the code
/// does not know fails to load rather than silently taking a value from somewhere else.
pub trait Tuning: DeserializeOwned + Send + Sync + 'static {
    /// Checks what the types admit but the code cannot use: a value out of range, or two values
    /// inconsistent with each other. The message names the value by its path in the file.
    fn validate(&self) -> Result<(), Invalid>;
}

/// Where a tuning file lives: its owning crate, its name, and the owning crate's directory in the
/// source tree the payload was built from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location {
    /// The owning crate's package name, which names its directory under the packaged `tuning/`.
    pub crate_name: &'static str,
    /// The file's name in the owning crate's `tuning/` directory.
    pub file_name: &'static str,
    /// The owning crate's manifest directory at build time.
    pub manifest_dir: &'static str,
}

impl Location {
    /// The file under a packaged tuning directory: `<root>/<crate>/<file>`.
    pub fn packaged(&self, root: &Path) -> PathBuf {
        root.join(self.crate_name).join(self.file_name)
    }

    /// The file in the source tree, as the build named it: `<manifest dir>/tuning/<file>`. Under
    /// wine this is a host path, which [`crate::tuning::Source::path`] converts.
    pub fn source_tree(&self) -> String {
        let separator = if self.manifest_dir.starts_with('/') {
            '/'
        } else {
            '\\'
        };
        format!(
            "{}{separator}tuning{separator}{}",
            self.manifest_dir, self.file_name
        )
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.crate_name, self.file_name)
    }
}

/// A tuning file's published values.
///
/// The values are published by pointer: a read is one acquiring load, and a reload swaps the
/// pointer. A replaced value is never freed, so the reference a read returns is valid for the rest
/// of the run whatever reloads follow; the cost is one root's size per applied edit, and edits are
/// made by hand.
pub struct TuningFile<T: Tuning> {
    location: Location,
    /// The values reads return; null until [`crate::tuning::init`] publishes the first.
    current: AtomicPtr<T>,
    /// Values the watcher parsed and validated, waiting for the game thread's next tick.
    pending: AtomicPtr<T>,
}

impl<T: Tuning> TuningFile<T> {
    /// A file with nothing published yet. Declared through [`crate::tuning_file!`].
    pub const fn new(location: Location) -> Self {
        Self {
            location,
            current: AtomicPtr::new(ptr::null_mut()),
            pending: AtomicPtr::new(ptr::null_mut()),
        }
    }

    pub const fn location(&self) -> &Location {
        &self.location
    }

    /// The published values.
    ///
    /// Reading before startup published them is a programming error and panics, except in a build
    /// with the `test-support` feature, where the first read loads the file from the source tree.
    #[inline]
    pub fn get(&'static self) -> &'static T {
        let current = self.current.load(Ordering::Acquire);
        if current.is_null() {
            return self.unpublished();
        }
        // SAFETY: a published pointer came from `Box::into_raw` and is never freed.
        unsafe { &*current }
    }

    #[cold]
    #[inline(never)]
    fn unpublished(&'static self) -> &'static T {
        #[cfg(feature = "test-support")]
        {
            self.load_for_tests()
        }
        #[cfg(not(feature = "test-support"))]
        {
            panic!(
                "tuning: {}: read before startup loaded the tuning",
                self.location
            )
        }
    }

    /// Loads the file from the source tree and publishes it, for tests, which read the tuning the
    /// repository holds without a startup.
    #[cfg(feature = "test-support")]
    fn load_for_tests(&'static self) -> &'static T {
        let path = crate::tuning::Source::SourceTree
            .path(&self.location, Path::new(""))
            .unwrap_or_else(|error| panic!("{error}"));
        let text = std::fs::read_to_string(&path)
            .map_err(|error| FileCause::read(error).at(&path))
            .unwrap_or_else(|error| panic!("{error}"));
        let value = parse::<T>(&text)
            .map_err(|cause| cause.at(&path))
            .unwrap_or_else(|error| panic!("{error}"));
        let fresh = Box::into_raw(Box::new(value));
        match self.current.compare_exchange(
            ptr::null_mut(),
            fresh,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            // SAFETY: just published, and published pointers are never freed.
            Ok(_) => unsafe { &*fresh },
            Err(winner) => {
                // SAFETY: `fresh` was never shared; `winner` is published and never freed.
                unsafe {
                    drop(Box::from_raw(fresh));
                    &*winner
                }
            }
        }
    }
}

/// A tuning file whatever its root's type: what the startup and the watcher need to load one.
pub trait AnyTuningFile: Sync {
    fn location(&self) -> &Location;
    /// Parses and validates `text` and holds the values for the next [`AnyTuningFile::commit`],
    /// replacing any values still waiting.
    fn stage(&self, text: &str) -> Result<(), FileCause>;
    /// Publishes the staged values, if any. Returns whether it published.
    fn commit(&self) -> bool;
}

impl<T: Tuning> AnyTuningFile for TuningFile<T> {
    fn location(&self) -> &Location {
        &self.location
    }

    fn stage(&self, text: &str) -> Result<(), FileCause> {
        let value = parse::<T>(text)?;
        let staged = Box::into_raw(Box::new(value));
        let replaced = self.pending.swap(staged, Ordering::AcqRel);
        if !replaced.is_null() {
            // SAFETY: a staged pointer is only published through `commit`'s swap, which takes it
            // out of `pending` first, so the one this swap returned was never shared.
            drop(unsafe { Box::from_raw(replaced) });
        }
        Ok(())
    }

    fn commit(&self) -> bool {
        if self.pending.load(Ordering::Relaxed).is_null() {
            return false;
        }
        let staged = self.pending.swap(ptr::null_mut(), Ordering::AcqRel);
        if staged.is_null() {
            return false;
        }
        // The replaced values stay allocated: readers may still hold references to them.
        self.current.store(staged, Ordering::Release);
        true
    }
}

/// Parses and validates a file's text.
pub(crate) fn parse<T: Tuning>(text: &str) -> Result<T, FileCause> {
    let value: T = toml::from_str(text).map_err(FileCause::Syntax)?;
    value.validate().map_err(FileCause::Invalid)?;
    Ok(value)
}
