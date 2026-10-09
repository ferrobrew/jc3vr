//! The action's work by hand, on the reload's input tick: the physical action, each gun's part on
//! its travel ([`crate::controllers::weapons::reload::travel`]), or the gesture
//! ([`crate::controllers::weapons::reload::chamber::ActionWork`]); the chamber a worked action
//! loads; and the parts published for drawing.

use glam::Vec3;

use crate::controllers::{
    config::ControllersConfig,
    frame::Hand,
    grab::GrabVolume,
    haptics::{self, Event},
    weapons::{
        foregrip::{self, set_pump_slide},
        reload::{
            FinishInputs, GunTick, State, Tick,
            chamber::{self, ActionKind, Chamber, WorkEvent, WorkInputs},
            config::ReloadConfig,
            engine,
            plan::ReloadPlan,
            render,
            travel::{ActionTravel, TravelEvents, TravelInputs},
        },
    },
};

impl State {
    /// The action's work by the free hand.
    pub(super) fn work(
        &mut self,
        tick: &Tick,
        inputs: &FinishInputs,
        config: &ControllersConfig,
        chambering: bool,
    ) {
        let reload = &config.weapons.reload;
        if reload.action.physical {
            self.status.work.reset();
            self.work_travel(tick, inputs, reload, chambering);
            self.publish_render(tick, reload, chambering);
            return;
        }
        set_pump_slide(None);
        let record = *self.record(tick);
        let kind = tick.plan.action;
        let gun_hand = Hand::BOTH
            .into_iter()
            .find(|h| {
                tick.guns[h.index()].is_some() && record.chambers[h.index()] == Chamber::Empty
            })
            .unwrap_or(tick.holder);
        let worker = self.free.filter(|h| {
            self.status
                .insert_hands
                .is_none_or(|(holding, _)| holding != *h)
        });
        let hold = foregrip::status().hold.filter(|h| h.engaged);
        let inputs = (chambering && kind != ActionKind::None)
            .then_some(())
            .and(worker)
            .zip(tick.guns[gun_hand.index()])
            .and_then(|(worker, gun)| {
                let hand = tick.hands[worker.index()]?.grip.position;
                let (handle, rearward) = match kind {
                    // The pump rides the foregrip, so its rest is wherever the hand holds it.
                    ActionKind::Pump => (gun.frame?.pose.position, gun.frame?.rearward()),
                    _ => gun.handle?,
                };
                Some(WorkInputs {
                    kind,
                    handle,
                    rearward,
                    travel_m: tick.plan.travel_m,
                    hand,
                    grip_pressed: inputs.grabs[worker.index()] == Some(GrabVolume::WeaponAction),
                    grip_held: inputs.grips[worker.index()].held(),
                    on_foregrip: hold.is_some_and(|h| h.holder.other() == worker),
                    needed: record.chambers[gun_hand.index()] == Chamber::Empty,
                })
            });
        let event = self.status.work.step(inputs, &reload.action);
        match event {
            Some(WorkEvent::Pulled) => {
                if let Some(worker) = worker {
                    haptics::play(worker, Event::Rack);
                }
            }
            Some(WorkEvent::Worked) => {
                let rounds = match record.split.filter(|_| tick.split) {
                    Some(split) => split.hands[gun_hand.index()].rounds,
                    None => tick.reading.magazine,
                };
                let magazine_in = match record.split.filter(|_| tick.split) {
                    Some(split) => !split.hands[gun_hand.index()].out,
                    None => !tick.reading.out,
                };
                let chamber = chamber::after_work(rounds, magazine_in);
                self.record(tick).chambers[gun_hand.index()] = chamber;
                self.status.counts.racks += 1;
                self.event(format!("{} worked", kind.label()));
                // SAFETY: main thread, outside the world update; the weapon was read this tick.
                unsafe { engine::cycled(tick.weapon, kind, true) };
                if chamber == Chamber::Chambered {
                    haptics::play(gun_hand, Event::Chambered);
                }
            }
            _ => {}
        }
        self.publish_render(tick, reload, chambering);
    }

    /// The physical action's work: each gun's part on its travel, the worked gun's taken by the
    /// free hand, or by the hand on the foregrip for a pump.
    fn work_travel(
        &mut self,
        tick: &Tick,
        inputs: &FinishInputs,
        reload: &ReloadConfig,
        chambering: bool,
    ) {
        let action = &reload.action;
        let kind = tick.plan.action;
        if !chambering || kind == ActionKind::None {
            set_pump_slide(None);
            return;
        }
        let record = *self.record(tick);
        let worker = self.free.filter(|h| {
            self.status
                .insert_hands
                .is_none_or(|(holding, _)| holding != *h)
        });
        let has_gun = |h: &Hand| tick.guns[h.index()].is_some();
        let worked = Hand::BOTH
            .into_iter()
            .filter(has_gun)
            .find(|h| record.travels[h.index()].grip.is_some())
            .or_else(|| {
                Hand::BOTH.into_iter().filter(has_gun).find(|h| {
                    let i = h.index();
                    record.chambers[i] == Chamber::Empty || !record.travels[i].in_battery(action)
                })
            })
            .unwrap_or(tick.holder);
        let foregrip_hand = foregrip::status()
            .hold
            .filter(|h| h.engaged && h.weapon == tick.weapon)
            .map(|h| h.holder.other());
        let travel_m = tick.plan.travel_m / tick.world_scale;
        let mut pump_slide = None;
        for gun_hand in Hand::BOTH {
            let i = gun_hand.index();
            let Some(gun) = tick.guns[i] else {
                continue;
            };
            let (rounds, magazine_in) = self.magazine(tick, gun_hand);
            let worker = worker.filter(|_| gun_hand == worked);
            let hand = worker.and_then(|w| Some(tick.hands[w.index()]?.grip.position));
            let mut travel = record.travels[i];
            let Some(geometry) = travel_geometry(tick, &gun) else {
                travel.release();
                self.record(tick).travels[i] = travel;
                continue;
            };
            let travel_inputs = TravelInputs {
                kind,
                handle: geometry.handle,
                rearward: geometry.rearward,
                side: geometry.side,
                up: geometry.up,
                travel_m,
                hand,
                grip_pressed: worker
                    .is_some_and(|w| inputs.grabs[w.index()] == Some(GrabVolume::WeaponAction)),
                grip_held: worker.is_some_and(|w| inputs.grips[w.index()].held()),
                on_foregrip: worker.is_some_and(|w| foregrip_hand == Some(w)),
                needed: record.chambers[i] == Chamber::Empty,
                magazine_in,
                feeds: magazine_in && rounds > 0,
                dt: inputs.dt,
            };
            let events = travel.step(&travel_inputs, action);
            self.record(tick).travels[i] = travel;
            self.on_travel(
                tick,
                gun_hand,
                worker.filter(|_| travel.grip.is_some() || events.let_go),
                events,
                (rounds, magazine_in),
            );
            if kind == ActionKind::Pump && travel.position > 0.0 {
                pump_slide = Some((tick.weapon, travel.position * travel_m));
            }
        }
        set_pump_slide(pump_slide);
    }

    /// Act on what a gun's action did this tick: the haptic stops, the slide's catch, and the
    /// chamber loaded by a cycle. `worker` is the hand on the handle, when one is.
    fn on_travel(
        &mut self,
        tick: &Tick,
        gun_hand: Hand,
        worker: Option<Hand>,
        events: TravelEvents,
        (rounds, magazine_in): (i32, bool),
    ) {
        let kind = tick.plan.action;
        let felt = worker.unwrap_or(gun_hand);
        if events.grabbed {
            self.event(format!("{} taken", kind.label()));
            tracing::debug!(
                target: "controllers",
                action = kind.label(),
                hand = gun_hand.label(),
                "reload: took the action's handle"
            );
        }
        if events.unlocked {
            self.event("the slide came off its catch".to_owned());
            tracing::info!(
                target: "controllers",
                hand = gun_hand.label(),
                "reload: took the slide off its catch"
            );
        }
        if events.lifted {
            haptics::play(felt, Event::ActionDetent);
        }
        if events.rear {
            haptics::play(felt, Event::Rack);
            self.event(format!("{} at the rear", kind.label()));
        }
        if events.locked_back {
            self.status.counts.lock_backs += 1;
            haptics::play(gun_hand, Event::ActionDetent);
            self.event("the slide caught on the empty magazine".to_owned());
            tracing::info!(
                target: "controllers",
                hand = gun_hand.label(),
                "reload: the slide caught at the rear on an empty magazine"
            );
        }
        if events.home {
            haptics::play(felt, Event::ActionDetent);
        }
        if events.let_go {
            self.event(format!("{} let go", kind.label()));
        }
        if events.cycled {
            let chamber = chamber::after_work(rounds, magazine_in);
            self.record(tick).chambers[gun_hand.index()] = chamber;
            self.status.counts.racks += 1;
            self.event(format!("{} worked", kind.label()));
            // The hand moved the part already, so the gun's own clip is not played over it.
            // SAFETY: main thread, outside the world update; the weapon was read this tick.
            unsafe { engine::cycled(tick.weapon, kind, false) };
            if chamber == Chamber::Chambered {
                haptics::play(gun_hand, Event::Chambered);
            }
            tracing::info!(
                target: "controllers",
                action = kind.label(),
                hand = gun_hand.label(),
                chambered = chamber == Chamber::Chambered,
                "reload: the action cycled"
            );
        }
    }

    /// The rounds in `hand`'s gun's magazine, and whether the magazine is in.
    fn magazine(&mut self, tick: &Tick, hand: Hand) -> (i32, bool) {
        let record = *self.record(tick);
        match record.split.filter(|_| tick.split) {
            Some(split) => {
                let h = split.hands[hand.index()];
                (h.rounds, !h.out)
            }
            None => (tick.reading.magazine, !tick.reading.out),
        }
    }

    /// Let go of every remembered weapon's actions, leaving the parts where they are.
    pub(super) fn release_actions(&mut self) {
        for (_, record) in &mut self.records {
            record.travels.iter_mut().for_each(ActionTravel::release);
        }
    }

    /// Publish the action parts and the held magazine part for the render thread.
    fn publish_render(&mut self, tick: &Tick, reload: &ReloadConfig, chambering: bool) {
        let record = *self.record(tick);
        let physical = reload.action.physical;
        let working = matches!(self.status.work.stage, chamber::WorkStage::Held { .. });
        let worked_gun = Hand::BOTH.into_iter().find(|h| {
            tick.guns[h.index()].is_some() && record.chambers[h.index()] == Chamber::Empty
        });
        let kind = tick.plan.action;
        let lift_rad = side_sign(&tick.plan) * reload.action.bolt_lift_deg.to_radians();
        let actions = Hand::BOTH.map(|hand| {
            let i = hand.index();
            let gun = tick.guns[i]?;
            if !chambering {
                return None;
            }
            let (share, lift) = if physical {
                (record.travels[i].position, record.travels[i].lift)
            } else if kind != ActionKind::Slide {
                return None;
            } else if working && worked_gun == Some(hand) {
                (self.status.work.progress, 0.0)
            } else if record.chambers[i] == Chamber::Empty {
                (1.0, 0.0)
            } else {
                return None;
            };
            Some(render::ActionPart {
                gun: gun.index as u32,
                // SAFETY: main thread, outside the world update; the weapon was read this tick.
                gun_address: unsafe { engine::gun_address(tick.weapon, gun.index) }.unwrap_or(0),
                kind,
                share,
                lift,
                travel_m: tick.plan.travel_m,
                lift_rad,
            })
        });
        let held =
            self.status
                .insert_hands
                .zip(tick.plan.part_top_m)
                .map(|((hand, _), part_top_m)| render::HeldPart {
                    hand,
                    top_world_m: reload.held_top_m * tick.world_scale,
                    part_top_m,
                });
        render::publish(render::Published {
            weapon: tick.weapon,
            actions,
            magazine_empty: tick.reading.magazine <= 0,
            held,
        });
    }
}

/// Whether the plan's slide catches at the rear on an empty magazine.
pub(super) fn locks_back(plan: &ReloadPlan, reload: &ReloadConfig) -> bool {
    plan.action == ActionKind::Slide && reload.action.physical && reload.action.lock_back
}

/// Where a gun's action lies in the cockpit frame.
struct TravelGeometry {
    /// The handle at rest: the table's handle, or for a pump without one, the gun's origin.
    handle: Vec3,
    rearward: Vec3,
    /// From a bolt's axis toward its handle, across the gun.
    side: Vec3,
    up: Vec3,
}

/// The tick's gun's action geometry, or `None` while the gun's frame or handle is unknown.
fn travel_geometry(tick: &Tick, gun: &GunTick) -> Option<TravelGeometry> {
    let frame = gun.frame?;
    let (handle, rearward) = match (tick.plan.action, gun.handle) {
        (_, Some(handle)) => handle,
        (ActionKind::Pump, None) => (frame.pose.position, frame.rearward()),
        _ => return None,
    };
    Some(TravelGeometry {
        handle,
        rearward: rearward.try_normalize()?,
        side: frame
            .direction(Vec3::X * side_sign(&tick.plan))
            .try_normalize()?,
        up: frame.direction(Vec3::Y).try_normalize()?,
    })
}

/// Which side of the gun the action's handle is on: `1` for the right (the model's +X), `-1`
/// for the left.
fn side_sign(plan: &ReloadPlan) -> f32 {
    match plan.handle {
        Some(handle) if handle.x < 0.0 => -1.0,
        _ => 1.0,
    }
}
