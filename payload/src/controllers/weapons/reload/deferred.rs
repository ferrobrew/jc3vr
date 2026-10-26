//! Input-side completion of button, flick, and belt magazine requests.

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
    },
};

impl State {
    pub(super) fn queue_eject(
        &mut self,
        weapon: &WeaponTick,
        gun: &GunTick,
        hand: Hand,
        kind: DetachKind,
        retain: bool,
    ) {
        if weapon.reading.out
            || self.pending_ejects[hand.index()].is_some()
            || self.extraction_hand() == Some(hand.other())
        {
            return;
        }
        let Some(expected) = gun.magazine else {
            return;
        };
        let Some(generation) = self.detach_generation.checked_add(1) else {
            return;
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
        if serial::submit_with_retention(request, retain) {
            self.pending_ejects[hand.index()] = Some(request);
        }
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
                continue;
            }
            self.status.counts.ejects += 1;
            self.extraction_blocks_belt[hand.index()] = true;
            if request.kind == DetachKind::Belt
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

    pub(super) fn cancel_ejects(&mut self) {
        for request in self.pending_ejects.iter_mut().filter_map(Option::take) {
            serial::cancel(request);
        }
    }
}
