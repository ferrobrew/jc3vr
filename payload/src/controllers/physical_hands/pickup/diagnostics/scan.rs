//! Fixed-size copies of branches the production reach paths actually evaluate.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::vehicle::vehicle::DoorSlot;

use crate::controllers::physical_hands::{
    bodies::Unclassified, doors::acquisition::PanelRefusal, grasp::Reach,
};

pub(crate) const SCAN_CAPACITY: usize = 12;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Branch {
    Ray,
    Press,
    DoorPanel,
    DoorHandle,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum ScanResult {
    Offered,
    StaticWorld,
    StandingOn,
    CharacterUnavailable,
    Unclassified(Unclassified),
    TooHeavy,
    PanelRefused(PanelRefusal),
    NoBox,
    NoHandle,
    OutOfReach,
    NoBody,
    Missed,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct ScanEntry {
    pub branch: Branch,
    pub object: Option<NonZeroUsize>,
    pub body: Option<u32>,
    pub door: Option<(NonZeroUsize, DoorSlot, u32)>,
    pub point: Option<Vec3>,
    pub distance: Option<f32>,
    pub mass_kg: Option<f32>,
    pub result: ScanResult,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Scan {
    pub entries: [Option<ScanEntry>; SCAN_CAPACITY],
    pub omitted: usize,
    pub ray_ran: bool,
    pub handles_ran: bool,
    pub ray_selected: Option<Reach>,
    pub handle_selected: Option<Reach>,
    pub selected: Option<Reach>,
}

impl Scan {
    pub(crate) const fn new() -> Self {
        Self {
            entries: [None; SCAN_CAPACITY],
            omitted: 0,
            ray_ran: false,
            handles_ran: false,
            ray_selected: None,
            handle_selected: None,
            selected: None,
        }
    }

    pub(crate) fn push(&mut self, entry: ScanEntry) {
        if let Some(slot) = self.entries.iter_mut().find(|entry| entry.is_none()) {
            *slot = Some(entry);
        } else {
            self.omitted = self.omitted.saturating_add(1);
        }
    }
}
