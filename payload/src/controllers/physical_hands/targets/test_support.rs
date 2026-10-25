use std::{num::NonZeroUsize, time::Instant};

use glam::Vec3;
use jc3gi::vehicle::vehicle::DoorSlot;

use crate::controllers::{
    frame::Hand,
    grab::GripTarget,
    physical_hands::{
        config::PhysicalHandsConfig,
        contact::PointProbe,
        doors::DoorTarget,
        grasp::{AngularTracker, Reach, TargetKind, VelocityTracker},
        palm::PROBE_POINTS,
        pickup::{HeldReport, PickupState, Request},
        shove::Give,
        state::{STALE_AFTER, State},
        status::PhysicalHandsStatus,
        targets::provided_reach,
    },
};

/// An isolated source for the production provider and final validation helpers.
pub(crate) struct DoorProvider {
    pub(crate) config: PhysicalHandsConfig,
    state: State,
    now: Instant,
}

impl DoorProvider {
    pub(crate) fn new() -> Self {
        let now = Instant::now();
        Self {
            config: PhysicalHandsConfig::new(),
            state: State {
                status: PhysicalHandsStatus::default(),
                probes: [[PointProbe::new(); PROBE_POINTS]; 2],
                pressed: [false; 2],
                last_sample: [None; 2],
                clamp: [Vec3::ZERO; 2],
                motion: [(VelocityTracker::new(), AngularTracker::new()); 2],
                gives: [Give::new(); 2],
                at: Some(now),
                epoch: 1,
                pickup: PickupState::new(),
                reach_notes: [Vec::new(), Vec::new()],
            },
            now,
        }
    }

    pub(crate) fn target(&mut self, hand: Hand, kind: TargetKind) -> GripTarget {
        let reach = Reach {
            object: NonZeroUsize::new(0x1000).unwrap(),
            body: 1,
            point: Vec3::ZERO,
            distance: 0.02,
            normal: Vec3::Y,
            kind,
        };
        self.state.status.hands[hand.index()].reach = Some(reach);
        provided_reach(hand, &reach, 1.0, &self.config, &self.state, self.now)
    }

    pub(crate) fn door(&mut self, hand: Hand, latched: bool) -> GripTarget {
        self.target(
            hand,
            TargetKind::Door(DoorTarget {
                vehicle: NonZeroUsize::new(0x2000).unwrap(),
                vehicle_body: Some(1),
                part_index: 2,
                slot: DoorSlot::FRONT_LEFT,
                latched,
                outward: Vec3::X,
            }),
        )
    }

    pub(crate) fn current(&self, hand: Hand, target: GripTarget) -> bool {
        target
            .door_eligibility
            .is_some_and(|offer| offer.current_in(hand, &self.config, &self.state, self.now))
    }

    pub(crate) fn expire(&mut self) {
        self.now += STALE_AFTER + std::time::Duration::from_millis(1);
    }

    pub(crate) fn replace_source(&mut self) {
        self.state.epoch += 1;
    }

    pub(crate) fn replace_sample(&mut self) {
        self.state.at = Some(self.now + std::time::Duration::from_millis(1));
    }

    pub(crate) fn replace_door(&mut self, hand: Hand) {
        let reach = self.state.status.hands[hand.index()]
            .reach
            .as_mut()
            .unwrap();
        if let TargetKind::Door(door) = &mut reach.kind {
            door.part_index += 1;
        }
    }

    pub(crate) fn remove_reach(&mut self, hand: Hand) {
        self.state.status.hands[hand.index()].reach = None;
    }

    pub(crate) fn claim(&mut self, hand: Hand) {
        self.state.pickup.requests[hand.index()] = Some(Request::Release);
    }

    pub(crate) fn claim_same_door(&mut self, hand: Hand) {
        let reach = self.state.status.hands[hand.index()].reach.unwrap();
        self.state.pickup.requests[hand.other().index()] = Some(Request::Grab(reach));
    }

    pub(crate) fn latch(&mut self, hand: Hand) {
        self.state.status.hands[hand.index()].latched = true;
    }

    pub(crate) fn hold(&mut self, hand: Hand) {
        self.state.status.hands[hand.index()].held = Some(HeldReport {
            mass_kg: 1.0,
            liftable: true,
            shared: false,
            centre: Vec3::ZERO,
            anchor: Vec3::ZERO,
            target: Vec3::ZERO,
            lag_m: 0.0,
            load: 0.0,
            twist: 0.0,
            joined: true,
        });
    }
}
