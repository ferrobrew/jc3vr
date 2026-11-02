//! Input-side completion of button, flick, belt, and swap magazine requests.
//!
//! A belt push or a pouch magazine seated while the gun's magazine is in drops the old magazine
//! first, through the same request as an eject, and loads the fresh one only once the drop's result
//! returns. The old magazine leaves with its own rounds (or none under unlimited ammunition), so
//! the load draws on the reserve for the fresh magazine alone, and a refused drop loads nothing.

use crate::controllers::{
    config::ControllersConfig,
    frame::Hand,
    haptics::{self, Event},
    weapons::reload::{
        GunTick, State, Tick, WeaponTick,
        chamber::Chamber,
        engine::{self, DetachOutcome},
        extraction::{DetachKind, DetachRequest, MagazineId},
        serial,
        steps::ready_after_seat,
        tuning::Reload,
    },
};

impl State {
    /// Queue a drop of the magazine of `gun` of `weapon`, in `hand`, for the serial callback.
    /// Returns whether it was queued.
    pub(super) fn queue_eject(
        &mut self,
        weapon: &WeaponTick,
        gun: &GunTick,
        hand: Hand,
        kind: DetachKind,
        retain: bool,
    ) -> bool {
        if weapon.reading.out
            || self.pending_ejects[hand.index()].is_some()
            || self.extraction_hand() == Some(hand.other())
        {
            return false;
        }
        let Some(expected) = gun.magazine else {
            return false;
        };
        let Some(generation) = self.detach_generation.checked_add(1) else {
            return false;
        };
        self.detach_generation = generation;
        let request = DetachRequest {
            id: MagazineId {
                weapon: weapon.address,
                gun: gun.index,
                holder: hand,
            },
            generation,
            expected,
            kind,
        };
        let queued = serial::submit_with_retention(request, retain);
        if queued {
            self.pending_ejects[hand.index()] = Some(request);
        }
        queued
    }

    pub(super) fn finish_ejects(&mut self, tick: &Tick, config: &ControllersConfig) {
        for hand in Hand::BOTH {
            let Some(request) = self.pending_ejects[hand.index()] else {
                continue;
            };
            let target = tick.gun(hand).filter(|(weapon, gun)| {
                weapon.address == request.id.weapon
                    && gun.index == request.id.gun
                    && gun.magazine.is_some_and(|id| request.expected.same_gun(id))
            });
            let Some((weapon, gun)) = target else {
                serial::cancel(request);
                self.pending_ejects[hand.index()] = None;
                continue;
            };
            let Some(outcome) = serial::poll(request) else {
                continue;
            };
            self.pending_ejects[hand.index()] = None;
            if outcome == DetachOutcome::NoThrow {
                self.event(format!("{hand:?} magazine request refused"));
                if request.kind == DetachKind::Swap {
                    // The fresh magazine was never paid for, so letting it go loses no round.
                    haptics::play(hand.other(), Event::PouchEmpty);
                }
                continue;
            }
            self.status.counts.ejects += 1;
            self.extraction_blocks_belt[hand.index()] = true;
            if request.kind == DetachKind::Swap {
                self.finish_swap(weapon, gun, hand, config);
            } else if request.kind == DetachKind::Belt
                && config.weapons.reload.belt.enabled
                && self.load(weapon, gun)
            {
                self.status.counts.belt_reloads += 1;
                let record = self.records.get(weapon.key);
                record.chambers[gun.entry()] =
                    engine::feed_chamber(weapon.address, gun.entry()).unwrap_or(Chamber::Chambered);
                record.travels[gun.entry()].close();
                self.event(format!("{hand:?} belt reload"));
                haptics::play(hand, Event::BeltSeat);
            } else {
                self.event(format!("{hand:?} ejected"));
                haptics::play(hand, Event::Eject);
            }
        }
    }

    /// Load the pouch magazine that pushed the old one out of `gun` of `weapon`, in `hand`, as a
    /// seat from the hand: no action is worked, and a chamber that kept its round stays ready.
    fn finish_swap(
        &mut self,
        weapon: &WeaponTick,
        gun: &GunTick,
        hand: Hand,
        config: &ControllersConfig,
    ) {
        if !self.load(weapon, gun) {
            self.event(format!("{hand:?} ejected, the fresh magazine did not load"));
            haptics::play(hand, Event::Eject);
            return;
        }
        self.status.counts.seats += 1;
        self.event(format!("{hand:?} seated, the old magazine dropped"));
        for hand in Hand::BOTH {
            haptics::play(hand, Event::MagazineSeat);
        }
        let chamber = self.records.get(weapon.key).chambers[gun.entry()];
        if ready_after_seat(&weapon.plan, chamber) {
            let delay_s = Reload::of(&config.weapons.reload).ready_cue_delay_s;
            self.ready_cue.arm(hand, delay_s);
        }
    }

    pub(super) fn cancel_ejects(&mut self) {
        for request in self.pending_ejects.iter_mut().filter_map(Option::take) {
            serial::cancel(request);
        }
    }
}
