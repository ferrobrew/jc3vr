//! Pure ownership of caller-supplied magazine and pair ammunition allocations.
//!
//! The engine's allocation policy is not inferred here. Chamber readiness remains the existing
//! reload model's state; it is not counted as an extra numeric round in this contract.

use std::fmt;

/// One verified member allocation. Moving this value transfers its rounds without copying them.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct GunAmmo {
    magazine: Option<MagazineRounds>,
    reserve: u32,
}

impl GunAmmo {
    pub fn new(magazine: u32, reserve: u32) -> Self {
        Self {
            magazine: Some(MagazineRounds(magazine)),
            reserve,
        }
    }

    pub fn total(&self) -> u64 {
        u64::from(self.reserve)
            + self
                .magazine
                .as_ref()
                .map_or(0, |magazine| u64::from(magazine.0))
    }

    pub fn extract(&mut self) -> Result<MagazineRounds, AmmoError> {
        self.magazine.take().ok_or(AmmoError::MagazineAbsent)
    }

    /// Return the magazine unchanged if the gun already has one. No rounds go to the reserve.
    pub fn insert(&mut self, magazine: MagazineRounds) -> Result<(), MagazineRounds> {
        if self.magazine.is_some() {
            return Err(magazine);
        }
        self.magazine = Some(magazine);
        Ok(())
    }
}

/// Rounds owned by a loaded or extracted magazine, without a second chamber-round count.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct MagazineRounds(u32);

impl MagazineRounds {
    pub fn rounds(&self) -> u32 {
        self.0
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum AmmoError {
    MagazineAbsent,
}

impl fmt::Display for AmmoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("loadout ammunition: the magazine is absent")
    }
}

impl std::error::Error for AmmoError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loaded_magazine_extraction_and_reinsertion_conserve_rounds() {
        let mut gun = GunAmmo::new(7, 21);
        let total = gun.total();
        let magazine = gun.extract().unwrap();
        assert_eq!(gun.total() + u64::from(magazine.rounds()), total);
        assert_eq!(gun.extract(), Err(AmmoError::MagazineAbsent));
        gun.insert(magazine).unwrap();
        assert_eq!(gun.total(), total);
    }

    #[test]
    fn occupied_insertion_returns_the_same_magazine_without_refunding_rounds() {
        let mut donor = GunAmmo::new(3, 0);
        let mut receiver = GunAmmo::new(5, 10);
        let magazine = receiver.insert(donor.extract().unwrap()).unwrap_err();
        assert_eq!(magazine.rounds(), 3);
        assert_eq!(receiver.total(), 15);
        assert_eq!(donor.total(), 0);
    }
}
