//! The action's work by hand, on the reload's input tick: the physical action, each gun's part on
//! its travel ([`crate::controllers::weapons::reload::travel`]), or the gesture
//! ([`crate::controllers::weapons::reload::chamber::ActionWork`]); the chamber a worked action
//! loads; and the parts published for drawing.

use glam::{Quat, Vec3};

use crate::controllers::{
    config::ControllersConfig,
    frame::{Hand, Pose},
    grab::GrabVolume,
    haptics::{self, Event},
    weapons::{
        foregrip::{self, set_pump_slide},
        reload::{
            FinishInputs, GunTick, State, Tick, WeaponTick,
            chamber::{self, ActionKind, Chamber, WorkEvent, WorkInputs},
            config::ReloadConfig,
            engine,
            insert::InsertStage,
            plan::ReloadPlan,
            render,
            steps::chambering,
            travel::{ActionTravel, TravelEvents, TravelInputs},
        },
    },
};

pub(super) trait ActionEngine {
    fn pump_hand(&self, weapon: usize) -> Option<Hand>;
    fn ammo(&mut self, weapon: usize) -> Option<(i32, bool)>;
    fn eject_round(&mut self, weapon: usize) -> Option<(i32, bool)>;
    fn cycled(&mut self, weapon: usize, kind: ActionKind, clip: bool);
}

struct NativeAction;

impl ActionEngine for NativeAction {
    fn pump_hand(&self, weapon: usize) -> Option<Hand> {
        foregrip::status()
            .hold
            .filter(|h| h.engaged && h.weapon == weapon)
            .map(|h| h.holder.other())
    }

    fn ammo(&mut self, weapon: usize) -> Option<(i32, bool)> {
        // SAFETY: action work uses the current input tick's owned weapon.
        unsafe { engine::action_ammo(weapon) }
    }

    fn eject_round(&mut self, weapon: usize) -> Option<(i32, bool)> {
        // SAFETY: action work uses the current input tick's owned weapon.
        unsafe { engine::eject_chamber_round(weapon) }
    }

    fn cycled(&mut self, weapon: usize, kind: ActionKind, clip: bool) {
        // SAFETY: action work uses the current input tick's owned weapon.
        unsafe { engine::cycled(weapon, kind, clip) };
    }
}

impl State {
    /// The action's work by the free hand.
    pub(super) fn work(&mut self, tick: &Tick, inputs: &FinishInputs, config: &ControllersConfig) {
        let reload = &config.weapons.reload;
        if reload.action.physical {
            self.status.work.reset();
            self.work_travel(tick, inputs, reload, &mut NativeAction);
            self.publish_render(tick, reload);
            return;
        }
        set_pump_slide(None);
        // The gesture works one gun: the first whose chamber is empty, or else the first with an
        // action to work.
        let workable_gun = |hand: Hand| {
            tick.gun(hand)
                .filter(|(w, _)| workable(&w.plan, reload))
                .map(|(w, g)| (hand, w, g))
        };
        let target = Hand::BOTH
            .into_iter()
            .filter_map(workable_gun)
            .find(|(_, w, g)| self.records.get(w.key).chambers[g.entry()] == Chamber::Empty)
            .or_else(|| Hand::BOTH.into_iter().find_map(workable_gun));
        let worker = self.free.filter(|h| {
            self.status
                .insert_hands
                .is_none_or(|(holding, _)| holding != *h)
                && self.extraction_hand() != Some(*h)
        });
        let hold = foregrip::status().hold.filter(|h| h.engaged);
        let work_inputs = target.zip(worker).and_then(|((_, weapon, gun), worker)| {
            let kind = weapon.plan.action;
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
                travel_m: weapon.plan.travel_m,
                hand,
                grip_pressed: inputs.grabs[worker.index()] == Some(GrabVolume::WeaponAction),
                grip_held: inputs.grips[worker.index()].held(),
                on_foregrip: hold.is_some_and(|h| h.holder.other() == worker),
                needed: self.records.get(weapon.key).chambers[gun.entry()] == Chamber::Empty,
            })
        });
        let event = self.status.work.step(work_inputs, &reload.action);
        match (event, target) {
            (Some(WorkEvent::Pulled), _) => {
                if let Some(worker) = worker {
                    haptics::play(worker, Event::Rack);
                }
            }
            (Some(WorkEvent::Worked), Some((gun_hand, weapon, gun))) => {
                let kind = weapon.plan.action;
                let (rounds, magazine_in) = self.magazine(weapon);
                let chamber = chamber::after_work(rounds, magazine_in);
                self.records.get(weapon.key).chambers[gun.entry()] = chamber;
                self.status.counts.racks += 1;
                self.event(format!("{} worked", kind.label()));
                // SAFETY: main thread, outside the world update; the weapon was read this tick.
                unsafe { engine::cycled(weapon.address, kind, true) };
                if chamber == Chamber::Chambered {
                    haptics::play(gun_hand, Event::Chambered);
                }
            }
            _ => {}
        }
        self.publish_render(tick, reload);
    }

    /// The physical action's work: each gun's part on its travel, with its own weapon's plan, the
    /// worked gun's taken by the free hand, or by the hand on the foregrip for a pump.
    pub(super) fn work_travel(
        &mut self,
        tick: &Tick,
        inputs: &FinishInputs,
        reload: &ReloadConfig,
        engine: &mut impl ActionEngine,
    ) {
        let action = &reload.action;
        let worker = self.free.filter(|h| {
            self.status
                .insert_hands
                .is_none_or(|(holding, _)| holding != *h)
                && self.extraction_hand() != Some(*h)
        });
        let guns = Hand::BOTH.map(|hand| {
            tick.gun(hand)
                .filter(|(w, _)| workable(&w.plan, reload))
                .map(|(w, g)| (*w, *g, *self.records.get(w.key)))
        });
        let with_gun = || Hand::BOTH.into_iter().filter(|h| guns[h.index()].is_some());
        let worked = with_gun()
            .find(|h| {
                guns[h.index()]
                    .is_some_and(|(_, g, record)| record.travels[g.entry()].grip.is_some())
            })
            .or_else(|| {
                with_gun().find(|h| {
                    guns[h.index()].is_some_and(|(_, g, record)| {
                        let e = g.entry();
                        record.chambers[e] == Chamber::Empty
                            || !record.travels[e].in_battery(action)
                    })
                })
            })
            .or_else(|| with_gun().next());
        let mut pump_slide = None;
        for gun_hand in Hand::BOTH {
            let Some((weapon, gun, record)) = guns[gun_hand.index()] else {
                continue;
            };
            let e = gun.entry();
            let kind = weapon.plan.action;
            let travel_m = weapon.plan.travel_m / tick.world_scale;
            let foregrip_hand = engine.pump_hand(weapon.address);
            let Some((rounds, magazine_in)) = engine.ammo(weapon.address) else {
                self.records.get(weapon.key).travels[e].release();
                continue;
            };
            let worker = worker.filter(|_| {
                worked == Some(gun_hand)
                    && self.pending_ejects[gun_hand.index()].is_none()
                    && !self.extraction_blocks_belt[gun_hand.index()]
            });
            let hand = worker.and_then(|w| Some(tick.hands[w.index()]?.grip.position));
            let mut travel = record.travels[e];
            let Some(geometry) = travel_geometry(&weapon.plan, &gun) else {
                travel.release();
                self.records.get(weapon.key).travels[e] = travel;
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
                needed: record.chambers[e] == Chamber::Empty,
                magazine_in,
                feeds: magazine_in && rounds > 0,
                dt: inputs.dt,
            };
            let events = travel.step(&travel_inputs, action);
            self.records.get(weapon.key).travels[e] = travel;
            self.on_travel(
                &weapon,
                &gun,
                gun_hand,
                worker.filter(|_| travel.grip.is_some() || events.let_go),
                events,
                engine,
            );
            if kind == ActionKind::Pump && travel.position > 0.0 && pump_slide.is_none() {
                pump_slide = Some((weapon.address, travel.position * travel_m));
            }
        }
        set_pump_slide(pump_slide);
    }

    /// Act on what a gun's action did this tick: the haptic stops, the slide's catch, and the
    /// chamber loaded by a cycle. `worker` is the hand on the handle, when one is.
    fn on_travel(
        &mut self,
        weapon: &WeaponTick,
        gun: &GunTick,
        gun_hand: Hand,
        worker: Option<Hand>,
        events: TravelEvents,
        engine: &mut impl ActionEngine,
    ) {
        let kind = weapon.plan.action;
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
            let record = self.records.get(weapon.key);
            if record.chambers[gun.entry()] == Chamber::Chambered
                && engine.eject_round(weapon.address).is_none()
            {
                record.travels[gun.entry()].pulled = false;
                return;
            }
            record.chambers[gun.entry()] = Chamber::Empty;
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
            let chamber = engine
                .ammo(weapon.address)
                .map_or(Chamber::Empty, |(rounds, magazine_in)| {
                    chamber::after_work(rounds, magazine_in)
                });
            self.records.get(weapon.key).chambers[gun.entry()] = chamber;
            self.status.counts.racks += 1;
            self.event(format!("{} worked", kind.label()));
            // The hand moved the part already, so the gun's own clip is not played over it.
            engine.cycled(weapon.address, kind, false);
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

    /// The rounds in `weapon`'s magazine, and whether the magazine is in.
    fn magazine(&self, weapon: &WeaponTick) -> (i32, bool) {
        (weapon.reading.magazine, !weapon.reading.out)
    }

    /// Let go of every remembered weapon's actions, leaving the parts where they are.
    pub(super) fn release_actions(&mut self) {
        for record in self.records.iter_mut() {
            record.travels.iter_mut().for_each(ActionTravel::release);
        }
    }

    /// Publish each hand's action part with its own weapon, and the held magazine part with the
    /// weapon its round is for, for the render thread.
    fn publish_render(&mut self, tick: &Tick, reload: &ReloadConfig) {
        let physical = reload.action.physical;
        let working = matches!(self.status.work.stage, chamber::WorkStage::Held { .. });
        let records = Hand::BOTH.map(|hand| {
            tick.gun(hand)
                .map(|(weapon, gun)| (*self.records.get(weapon.key), gun.entry()))
        });
        let worked_gun = Hand::BOTH.into_iter().find(|&hand| {
            tick.gun(hand)
                .is_some_and(|(w, _)| workable(&w.plan, reload))
                && records[hand.index()].is_some_and(|(r, e)| r.chambers[e] == Chamber::Empty)
        });
        let actions = Hand::BOTH.map(|hand| {
            let i = hand.index();
            let (weapon, gun) = tick.gun(hand)?;
            let (record, e) = records[i]?;
            if !chambering(&weapon.plan, reload) {
                return None;
            }
            let kind = weapon.plan.action;
            let (share, lift) = if physical {
                (record.travels[e].position, record.travels[e].lift)
            } else if kind != ActionKind::Slide {
                return None;
            } else if working && worked_gun == Some(hand) {
                (self.status.work.progress, 0.0)
            } else if record.chambers[e] == Chamber::Empty {
                (1.0, 0.0)
            } else {
                return None;
            };
            Some(render::ActionPart {
                weapon: weapon.address,
                gun: gun.index as u32,
                // SAFETY: main thread, outside the world update; the weapon was read this tick.
                gun_address: unsafe { engine::gun_address(weapon.address, gun.index) }.unwrap_or(0),
                kind,
                share,
                lift,
                travel_m: weapon.plan.travel_m,
                lift_rad: side_sign(&weapon.plan) * reload.action.bolt_lift_deg.to_radians(),
                magazine_empty: weapon.reading.magazine <= 0,
            })
        });
        let held = self.status.insert_hands.and_then(|(hand, gun_hand)| {
            let (weapon, gun) = tick.gun(gun_hand)?;
            let captured = match self.status.insertion.stage {
                InsertStage::Captured { depth_m, .. } => gun.well.map(|well| {
                    let inverse = gun.wrist.orientation.inverse();
                    let axis = inverse * well.axis;
                    (
                        gun_hand,
                        Pose {
                            position: inverse
                                * (well.mouth + well.axis * depth_m - gun.wrist.position)
                                * tick.world_scale,
                            orientation: Quat::from_rotation_arc(Vec3::Y, axis),
                        },
                    )
                }),
                _ => None,
            };
            Some(render::HeldPart {
                weapon: weapon.address,
                hand,
                top_world_m: reload.held_top_m * tick.world_scale,
                part_top_m: weapon.plan.part_top_m?,
                captured,
            })
        });
        render::publish(render::Published {
            weapons: tick.weapons.map(|w| w.map_or(0, |w| w.address)),
            actions,
            held,
        });
    }
}

/// Whether the plan's slide catches at the rear on an empty magazine.
pub(super) fn locks_back(plan: &ReloadPlan, reload: &ReloadConfig) -> bool {
    plan.action == ActionKind::Slide && reload.action.physical && reload.action.lock_back
}

/// Whether the plan's action is the mod's to work.
fn workable(plan: &ReloadPlan, reload: &ReloadConfig) -> bool {
    chambering(plan, reload) && plan.action != ActionKind::None
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

/// The action geometry of `gun`, whose weapon's plan is `plan`, or `None` while the gun's frame or
/// handle is unknown.
fn travel_geometry(plan: &ReloadPlan, gun: &GunTick) -> Option<TravelGeometry> {
    let frame = gun.frame?;
    let (handle, rearward) = match (plan.action, gun.handle) {
        (_, Some(handle)) => handle,
        (ActionKind::Pump, None) => (frame.pose.position, frame.rearward()),
        _ => return None,
    };
    Some(TravelGeometry {
        handle,
        rearward: rearward.try_normalize()?,
        side: frame.direction(Vec3::X * side_sign(plan)).try_normalize()?,
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
