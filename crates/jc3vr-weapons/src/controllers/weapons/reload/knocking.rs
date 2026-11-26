//! The input tick's side of the tactical knock ([`crate::controllers::weapons::reload::knock`]):
//! the held pouch magazine's tip against the inserted magazine of the gun in the other hand, the
//! drop a strike queues, and the knocked magazine's impulse once it is in the world.

use jc3vr_body::controllers::haptics;
use jc3vr_core::published::Stamp;
use jc3vr_input::{frame::Hand, haptics::cues::Event};

use crate::controllers::weapons::{
    config::WeaponSettings,
    reload::{
        FinishInputs, State, Tick, engine,
        extraction::{DetachKind, MagazineId},
        frames, insert,
        insert::{InsertStage, InsertionSource},
        knock,
        knock::{GunMagazine, KnockView, Strike},
        loose,
        loose::Operation,
        tuning::{RELOAD, Reload},
        wells,
    },
};

impl State {
    /// Test the held pouch magazine's tip against the inserted magazine of the gun it is for, and
    /// queue that magazine's drop on a strike. The zone is published for the grab view.
    pub(super) fn knock(&mut self, tick: &Tick, inputs: &FinishInputs, config: &WeaponSettings) {
        let reload = &Reload::of(&config.weapons.reload);
        let holding = matches!(self.status.insertion.stage, InsertStage::Holding { .. })
            && self.insertion_source == InsertionSource::Pouch;
        let Some((hand, gun_hand)) = self.status.insert_hands.filter(|_| holding) else {
            self.knocks.lose_tip();
            self.knock_guns(tick, inputs);
            return;
        };
        self.knocks.lose_guns();
        let Some((weapon, gun)) = tick.gun(gun_hand) else {
            self.knocks.lose_tip();
            knock::publish(None);
            return;
        };
        // A launcher's spent round is replaced, not knocked out.
        let zone = knock::zone_for(
            weapon.plan.load,
            weapon.plan.well,
            !weapon.occupied(),
            gun.well.map(|well| (well.mouth, well.axis)),
            knock::below_m(wells::lookup(weapon.reading.name_hash)),
            tick.world_scale,
        );
        let tip = self
            .grip_motion(tick, hand, inputs.dt)
            .and_then(|(pose, velocity)| {
                insert::held_round(
                    frames::held_line(pose, hand, reload.held_palm_gap_m),
                    frames::pointing(pose),
                    reload.held_top_m,
                    velocity,
                )
                .map(|held| held.top)
            });
        let (Some(zone), Some(tip)) = (zone, tip) else {
            self.knocks.lose_tip();
            knock::publish(None);
            return;
        };
        let view = |fast, knocked_at| KnockView {
            holder: gun_hand,
            zone,
            tip: Some(tip),
            fast,
            knocked_at,
        };
        // A drop already queued takes the magazine out; the tip waits for the next zone.
        if self.pending_ejects[gun_hand.index()].is_some() {
            self.knocks.lose_tip();
            knock::publish(Some(view(false, self.knocks.knocked_at)));
            return;
        }
        let previous = self.knocks.previous;
        let min_speed_mps = reload.knock.min_speed_mps;
        let (strike, local) = knock::strike(&zone, previous, tip, inputs.dt, min_speed_mps);
        self.knocks.previous = local;
        let fast = previous
            .zip(local)
            .is_some_and(|(p, l)| inputs.dt > 0.0 && p.distance(l) / inputs.dt >= min_speed_mps);
        if let Some(strike) = strike {
            self.queue_knock(tick, gun_hand, strike);
        } else if let (Some(touch), _) = knock::strike(&zone, previous, tip, inputs.dt, 0.0) {
            // A meeting too slow to knock, logged so a threshold that turns strikes away shows.
            tracing::info!(
                target: "controllers",
                hand = ?hand,
                closing_mps = touch.closing_mps,
                min_speed_mps,
                "reload: the held magazine met the inserted one too slowly to knock it out"
            );
        }
        knock::publish(Some(view(fast, self.knocks.knocked_at)));
    }

    /// Knock the magazine of `gun_hand`'s gun out with a push of the held round into its well at
    /// `speed_mps`: the old magazine leaves straight out of the well, struck at the mouth, as a
    /// strike on its base sends it. Returns whether the drop was queued; a gun whose magazine
    /// cannot be knocked out refuses the push instead.
    pub(super) fn push_knock(&mut self, tick: &Tick, gun_hand: Hand, speed_mps: f32) -> bool {
        let Some((weapon, gun)) = tick.gun(gun_hand) else {
            return false;
        };
        let Some(zone) = knock::zone_for(
            weapon.plan.load,
            weapon.plan.well,
            !weapon.occupied(),
            gun.well.map(|well| (well.mouth, well.axis)),
            knock::below_m(wells::lookup(weapon.reading.name_hash)),
            tick.world_scale,
        ) else {
            return false;
        };
        let strike = knock::push_strike(&zone, speed_mps);
        tracing::info!(target: "controllers", hand = ?gun_hand.other(), speed_mps, "reload: the held magazine was pushed into the occupied well fast enough to knock");
        self.queue_knock(tick, gun_hand, strike)
    }

    fn knock_guns(&mut self, tick: &Tick, inputs: &FinishInputs) {
        if self.status.insertion.holding()
            || self.pending_detach.is_some()
            || self.pending_ejects.iter().any(Option::is_some)
            || self.knocks.impulse.is_some()
            || self.knocks.linger.is_some()
        {
            self.knocks.lose_guns();
            knock::publish(None);
            return;
        }
        let magazines = Hand::BOTH.map(|holder| {
            let (weapon, gun) = tick.gun(holder)?;
            Some(GunMagazine {
                id: MagazineId {
                    weapon: weapon.address,
                    gun: gun.index,
                    holder,
                },
                expected: gun.magazine?,
                zone: knock::zone_for(
                    weapon.plan.load,
                    weapon.plan.well,
                    !weapon.occupied(),
                    gun.well.map(|well| (well.mouth, well.axis)),
                    knock::below_m(wells::lookup(weapon.reading.name_hash)),
                    tick.world_scale,
                )?,
                world_scale: tick.world_scale,
            })
        });
        let strikes =
            self.knocks
                .gun_strikes(magazines, inputs.dt, RELOAD.get().knock.min_speed_mps);
        let candidate = Hand::BOTH
            .into_iter()
            .filter_map(|holder| Some((holder, strikes[holder.index()]?)))
            .max_by(|(_, a), (_, b)| a.closing_mps.total_cmp(&b.closing_mps));
        let Some((holder, strike)) = candidate else {
            knock::publish(None);
            return;
        };
        if self.queue_knock(tick, holder, strike) {
            self.knocks.lose_guns();
            knock::publish(magazines[holder.index()].map(|magazine| KnockView {
                holder,
                zone: magazine.zone,
                tip: magazines[holder.other().index()].map(|source| source.zone.ends().1),
                fast: true,
                knocked_at: self.knocks.knocked_at,
            }));
        } else {
            knock::publish(None);
        }
    }

    fn queue_knock(&mut self, tick: &Tick, holder: Hand, strike: Strike) -> bool {
        let Some((weapon, gun)) = tick.gun(holder) else {
            return false;
        };
        if !self.queue_eject(weapon, gun, holder, DetachKind::Knock) {
            return false;
        }
        self.knocks.strikes[holder.index()] = Some(strike);
        self.knocks.knocked_at = Some(Stamp::now());
        self.status.counts.knocks += 1;
        self.event(format!(
            "{:?} knocked the magazine out at {:.1} m/s",
            holder.other(),
            strike.closing_mps
        ));
        tracing::info!(target: "controllers", hand = ?holder, closing_mps = strike.closing_mps, "reload: knocked the magazine out");
        haptics::play_hands(Hand::BOTH, Event::MagazineKnock);
        true
    }

    /// Give the knocked magazine's body its impulse, once its body is readable, and hide the gun's
    /// magazine part a few ticks after it lands.
    pub(super) fn push_knocked(&mut self, tick: &Tick) {
        self.push_impulse();
        let pending = self.knocks.impulse.is_some();
        let Some(mut linger) = self.knocks.linger else {
            return;
        };
        // A magazine seated meanwhile owns the part; a gun gone from the hands has its part hidden
        // at once, as its magazine is still out.
        let weapon = tick.weapon(linger.weapon);
        if weapon.is_some_and(|w| !w.reading.out) {
            self.knocks.linger = None;
            return;
        }
        if weapon.is_none() || linger.step(pending) {
            self.knocks.linger = None;
            // SAFETY: game thread before the world update. A gun gone from the hands was not read
            // this tick, but the part's lookup resolves the address only through the inventory's
            // slots and the carry set, so a gun the player no longer owns is left alone.
            unsafe { engine::show_magazine_part(linger.weapon, false) };
        } else {
            self.knocks.linger = Some(linger);
        }
    }

    fn push_impulse(&mut self) {
        let Some(mut pending) = self.knocks.impulse.take() else {
            return;
        };
        // SAFETY: the input tick runs on the game thread before the world update.
        let outcome = unsafe {
            loose::knock(pending.magazine, |body| {
                knock::knocked(
                    body,
                    body.pose.centre + pending.contact,
                    pending.velocity,
                    &RELOAD.get().knock,
                )
            })
        };
        match outcome {
            Operation::Ready(()) => {}
            Operation::Deferred if pending.ticks_left > 1 => {
                pending.ticks_left -= 1;
                self.knocks.impulse = Some(pending);
            }
            Operation::Deferred | Operation::Gone => {
                tracing::info!(target: "controllers", "reload: the knocked magazine's body was not readable, so it flies with the gun's velocity alone");
            }
        }
    }
}
