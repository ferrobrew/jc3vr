//! Input-side completion of button, flick, belt, and knock magazine requests.
//!
//! A belt push while the gun's magazine is in drops the old magazine first, through the same
//! request as an eject, and loads the fresh one only once the drop's result returns. A knock drops
//! it the same way and leaves the fresh magazine in the hand to seat. Either way the old magazine
//! leaves with its own rounds (or none under unlimited ammunition), so a load draws on the reserve
//! for the fresh magazine alone, and a refused drop loads nothing.

use crate::controllers::{
    config::ControllersConfig,
    frame::Hand,
    haptics::{self, Event},
    weapons::{
        pouch_models,
        reload::{
            GunTick, State, Tick, WeaponTick,
            chamber::Chamber,
            engine::{self, DetachOutcome},
            extraction::{DetachKind, DetachRequest, MagazineId},
            knock::{KNOCK, Linger, PendingImpulse},
            serial, steps,
            tuning::Reload,
        },
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
                self.knocks.strikes[hand.index()] = None;
                continue;
            }
            self.status.counts.ejects += 1;
            self.extraction_blocks_belt[hand.index()] = true;
            if request.kind == DetachKind::Knock {
                self.finish_knock(hand, outcome, weapon);
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
                // SAFETY: game thread before the world update; the weapon was read this tick.
                unsafe { steps::seat_sound(weapon.address, gun.index) };
                self.seated(
                    weapon,
                    gun,
                    hand,
                    Event::BeltSeat,
                    &Reload::of(&config.weapons.reload),
                );
            } else {
                self.event(format!("{hand:?} ejected"));
                haptics::play(hand, Event::Eject);
            }
        }
    }

    /// The knocked magazine is out: give its body the strike's impulse, world space. An unretained
    /// prop is not the mod's to move, so it flies with the gun's velocity alone. The gun's own
    /// magazine part stays drawn in the well while the prop appears, where the gun's spare magazine
    /// model draws the one in the hand, leaving the part free.
    fn finish_knock(&mut self, hand: Hand, outcome: DetachOutcome, weapon: &WeaponTick) {
        let strike = self.knocks.strikes[hand.index()].take();
        self.event(format!("{hand:?} magazine knocked out"));
        // SAFETY (both): game thread before the world update; the weapon was read this tick.
        if weapon.plan.spare_top_m.is_some() && unsafe { pouch_models::hand_draws(weapon.address) }
        {
            unsafe { engine::show_magazine_part(weapon.address, true) };
            self.knocks.linger = Some(Linger {
                weapon: weapon.address,
                ticks_left: KNOCK.linger_ticks,
            });
        }
        if let (DetachOutcome::Retained(magazine), Some(strike), Some(frames)) =
            (outcome, strike, self.frames)
        {
            self.knocks.impulse = Some(PendingImpulse {
                magazine,
                contact: frames.cockpit.offset_to_world(strike.contact),
                velocity: frames.cockpit.offset_to_world(strike.velocity),
                ticks_left: KNOCK.impulse_ticks,
            });
        }
    }

    pub(super) fn cancel_ejects(&mut self) {
        for request in self.pending_ejects.iter_mut().filter_map(Option::take) {
            serial::cancel(request);
        }
        self.knocks.cancel();
    }
}
