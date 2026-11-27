//! The install-time check that every contribution a lower layer reads was registered.
//!
//! A lower crate reaches a higher one only through what the higher one registers with it
//! (`docs/architecture.md`, "Crate layering"). A contribution nobody registered is a broken
//! composition rather than a state the game can reach, so the install does not run without it: the
//! payload's composition hands one [`RegistrationCheck`] to every crate's registration, each crate
//! reports what it reads and finds unregistered, and [`RegistrationCheck::finish`] returns every
//! missing contribution at once. The payload logs the list and stops its startup before any hook is
//! installed. Nothing panics, and nothing is skipped silently.

use std::fmt;

/// The contributions found missing so far, collected across every crate's registration.
#[derive(Debug, Default)]
pub struct RegistrationCheck {
    missing: Vec<Missing>,
}

impl RegistrationCheck {
    pub fn new() -> Self {
        Self::default()
    }

    /// Report one contribution `reader` reads, missing unless `registered`.
    pub fn require(&mut self, reader: &'static str, contribution: &'static str, registered: bool) {
        if !registered {
            self.missing.push(Missing {
                reader,
                contribution: contribution.to_owned(),
            });
        }
    }

    /// Report each key of a keyed table `reader` reads (a stage, an observer) that nobody filled,
    /// as `kind` followed by the key.
    pub fn require_keys<K: fmt::Debug>(
        &mut self,
        reader: &'static str,
        kind: &'static str,
        missing: impl IntoIterator<Item = K>,
    ) {
        self.missing.extend(missing.into_iter().map(|key| Missing {
            reader,
            contribution: format!("{kind} {key:?}"),
        }));
    }

    /// The contributions reported missing so far, in report order.
    pub fn missing(&self) -> &[Missing] {
        &self.missing
    }

    /// `Ok` when nothing was reported missing, otherwise every missing contribution.
    pub fn finish(self) -> Result<(), MissingRegistrations> {
        if self.missing.is_empty() {
            Ok(())
        } else {
            Err(MissingRegistrations {
                missing: self.missing,
            })
        }
    }
}

/// One contribution a layer reads that nobody registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missing {
    /// The subsystem that reads the contribution.
    pub reader: &'static str,
    /// What is missing, in the reader's words.
    pub contribution: String,
}

impl fmt::Display for Missing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.reader, self.contribution)
    }
}

/// Every contribution the install left unregistered; never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingRegistrations {
    missing: Vec<Missing>,
}

impl MissingRegistrations {
    pub fn missing(&self) -> &[Missing] {
        &self.missing
    }
}

impl fmt::Display for MissingRegistrations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "registration: the install left {} contribution{} unregistered:",
            self.missing.len(),
            if self.missing.len() == 1 { "" } else { "s" },
        )?;
        for missing in &self.missing {
            write!(f, "\n- {missing}")?;
        }
        Ok(())
    }
}

impl std::error::Error for MissingRegistrations {}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn a_check_with_everything_registered_finishes() {
        let mut check = RegistrationCheck::new();
        check.require("aim", "the aim hooks", true);
        check.require_keys("hands", "hand stage", std::iter::empty::<u8>());
        assert_eq!(check.finish(), Ok(()));
    }

    #[test]
    fn every_missing_contribution_is_listed_in_report_order() {
        #[derive(Debug)]
        enum Stage {
            First,
            Second,
        }
        let mut check = RegistrationCheck::new();
        check.require("aim", "the aim hooks", false);
        check.require("aim", "the mounted-gun source", true);
        check.require_keys("hands", "hand stage", [Stage::First, Stage::Second]);
        let error = check.finish().unwrap_err();
        assert_eq!(error.missing().len(), 3);
        assert_eq!(
            error.to_string(),
            "registration: the install left 3 contributions unregistered:\n\
             - aim: the aim hooks\n\
             - hands: hand stage First\n\
             - hands: hand stage Second"
        );
    }
}
