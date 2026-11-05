//! The input tick's side of the tactical knock ([`crate::controllers::weapons::reload::knock`]):
//! the held pouch magazine's tip against the inserted magazine of the gun in the other hand, the
//! drop a strike queues, and the knocked magazine's impulse once it is in the world.

use std::time::Instant;

use crate::controllers::{
    config::ControllersConfig,
    frame::Hand,
    haptics::{self, Event},
    weapons::reload::{
        FinishInputs, State, Tick,
        extraction::DetachKind,
        frames,
        insert::{self, InsertStage, InsertionSource},
        knock::{self, KNOCK, KnockView},
        loose::{self, Operation},
        tuning::Reload,
        wells,
    },
};

impl State {
    /// Test the held pouch magazine's tip against the inserted magazine of the gun it is for, and
    /// queue that magazine's drop on a strike. The zone is published for the grab view.
    pub(super) fn knock(&mut self, tick: &Tick, inputs: &FinishInputs, config: &ControllersConfig) {
        let reload = &Reload::of(&config.weapons.reload);
        let min_speed_mps = config.weapons.reload.knock.min_speed_mps;
        let holding = matches!(self.status.insertion.stage, InsertStage::Holding { .. })
            && self.insertion_source == InsertionSource::Pouch;
        let Some((hand, gun_hand)) = self.status.insert_hands.filter(|_| holding) else {
            self.knocks.lose_tip();
            knock::publish(None);
            return;
        };
        let Some((weapon, gun)) = tick.gun(gun_hand) else {
            self.knocks.lose_tip();
            knock::publish(None);
            return;
        };
        let zone = knock::zone_for(
            weapon.plan.load,
            weapon.plan.well,
            weapon.reading.out,
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
        let (strike, local) = knock::strike(&zone, previous, tip, inputs.dt, min_speed_mps);
        self.knocks.previous = local;
        let fast = previous
            .zip(local)
            .is_some_and(|(p, l)| inputs.dt > 0.0 && p.distance(l) / inputs.dt >= min_speed_mps);
        if let Some(strike) = strike
            && self.queue_eject(
                weapon,
                gun,
                gun_hand,
                DetachKind::Knock,
                reload.loose.enabled,
            )
        {
            self.knocks.strikes[gun_hand.index()] = Some(strike);
            self.knocks.knocked_at = Some(Instant::now());
            self.status.counts.knocks += 1;
            self.event(format!(
                "{hand:?} knocked the magazine out at {:.1} m/s",
                strike.closing_mps
            ));
            tracing::info!(target: "controllers", hand = ?gun_hand, closing_mps = strike.closing_mps, "reload: knocked the magazine out");
            haptics::play_hands(Hand::BOTH, Event::MagazineKnock);
        }
        knock::publish(Some(view(fast, self.knocks.knocked_at)));
    }

    /// Give the knocked magazine's body its impulse, once its body is readable.
    pub(super) fn push_knocked(&mut self) {
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
                    &KNOCK,
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
