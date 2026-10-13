//! The reload's input-tick steps over the tick's weapon: the shots folded in, the ejects, the
//! two-handed load, the belt pushes, the action's work, and the fire hold.

use glam::Vec3;
use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponBase};

use crate::controllers::{
    config::ControllersConfig,
    frame::{Hand, Pose},
    grab::{Candidate, GrabVolume},
    hands::{ActiveHands, ik},
    haptics::{self, Event},
    weapons::{
        self,
        dual_wield::single,
        engine::HeldWeapon,
        foregrip::set_pump_slide,
        regions::{action_back, action_volume},
        reload::{
            FinishInputs, GunTick, MAX_EVENTS, MAX_RECORDS, Record, State, Tick, WeaponReadout,
            action::locks_back,
            belt::{self, BeltPush, PushInputs},
            chamber::{self, ActionKind, Chamber},
            config::{ReloadButton, ReloadConfig},
            dual::{self, SplitMagazines},
            eject::FlickDetector,
            engine,
            frames::{self, GunFrame},
            gate,
            insert::{self, InsertEvent, InsertInputs, InsertStage, Well},
            loose,
            plan::{self, LoadKind, ReloadPlan, WeaponFacts},
            travel::ActionTravel,
            wells,
        },
    },
};

impl State {
    /// Read the weapon, its plan, and its guns' frames.
    ///
    /// # Safety
    ///
    /// Main thread, outside the world update, with `weapon` read this tick.
    pub(super) unsafe fn read_tick(
        &mut self,
        weapon: &HeldWeapon,
        active: &ActiveHands,
        config: &ControllersConfig,
        body_points: [Option<Vec3>; 2],
    ) -> Option<Tick> {
        let reload = &config.weapons.reload;
        let base = unsafe { (weapon.address as *const WeaponBase).as_ref() }?;
        let holder = weapon.holder?;
        let reading = unsafe { engine::read(base) };
        let entry = wells::lookup(reading.name_hash);
        let plan = plan::plan(
            WeaponFacts {
                class: reading.class,
                capacity: reading.capacity,
                exempt: reading.exempt,
            },
            entry,
            reload.well_offset_m,
            reload.action.default_travel_m,
        )?;
        let reserve = unsafe { engine::reserve(base) };
        let split = weapon.wielded.slot == WeaponSlot::E_SLOT_DUAL_WIELD
            && !single::is(weapon.address)
            && reload.dual_split
            && config.weapons.dual_wield.split;
        // The gun as it is shown: turned onto the line between the hands by the two-handed pose
        // and by its weight, so the well and the handle are where the player sees them.
        let posed = weapons::weighted_hands(active);
        let targets = ik::tick_records().current;
        let mut guns = [None; 2];
        for hand in Hand::BOTH {
            let i = hand.index();
            if !weapon.wielded.hands[i] {
                self.measured[i] = None;
                continue;
            }
            let Some(cockpit) = posed[i] else {
                continue;
            };
            let Some(index) = (unsafe { engine::gun_index(base, hand) }) else {
                continue;
            };
            let fresh = targets[i].and_then(|target| unsafe {
                engine::measure_gun(base, index, target, active.world_scale)
            });
            if let Some(fresh) = fresh {
                self.measured[i] = Some((weapon.address, fresh));
            }
            let in_wrist = self.measured[i]
                .filter(|(address, _)| *address == weapon.address)
                .map(|(_, pose)| pose);
            let frame = in_wrist.map(|p| GunFrame::of(cockpit.wrist, p, active.world_scale));
            let well = frames::well(plan.well, frame.as_ref(), cockpit.wrist, hand)
                .map(|(mouth, axis)| Well { mouth, axis });
            let handle = frame
                .zip(plan.handle)
                .map(|(f, h)| (f.point(h), f.rearward()));
            guns[i] = Some(GunTick {
                index,
                wrist: cockpit.wrist,
                frame,
                well,
                handle,
                drawn_grip: active.hands[i].map_or(cockpit.grip.position, |h| h.grip.position),
                body_grip: body_points[i],
            });
        }
        Some(Tick {
            weapon: weapon.address,
            holder,
            dominant: config.acting_hand(),
            split,
            plan,
            from_table: entry.is_some(),
            reading,
            reserve,
            guns,
            hands: active.hands,
            world_scale: active.world_scale,
        })
    }

    /// The action handle's grab volume, while a gun's chamber waits on it and a hand is free.
    pub(super) fn handle_candidate(
        &mut self,
        tick: &Tick,
        free: Option<Hand>,
        reload: &ReloadConfig,
    ) -> Option<Candidate> {
        let free = free?;
        if !chambering(&tick.plan, reload) || tick.plan.action == ActionKind::Pump {
            return None;
        }
        let action = &reload.action;
        let record = *self.record(tick);
        let gun_hand = Hand::BOTH.into_iter().find(|h| {
            let i = h.index();
            tick.guns[i].is_some()
                && (record.chambers[i] == Chamber::Empty
                    || (action.physical && !record.travels[i].in_battery(action)))
        })?;
        let gun = tick.guns[gun_hand.index()]?;
        let (handle, rearward) = gun.handle?;
        // The handle rides the part, so a slide held back is taken where it is.
        let back = if action.physical {
            let travel = &record.travels[gun_hand.index()];
            action_back(travel.position, tick.plan.travel_m, tick.world_scale)
        } else {
            0.0
        };
        Some(action_volume(
            handle,
            rearward,
            back,
            action.grab_radius_m,
            free,
        ))
    }

    pub(super) fn record(&mut self, tick: &Tick) -> &mut Record {
        let index = match self.records.iter().position(|(w, _)| *w == tick.weapon) {
            Some(index) => index,
            None => {
                if self.records.len() >= MAX_RECORDS {
                    self.records.remove(0);
                }
                self.records.push((tick.weapon, Record::default()));
                self.records.len() - 1
            }
        };
        &mut self.records[index].1
    }

    /// The tick's steps; returns the hands whose guns may not fire.
    pub(super) fn step(
        &mut self,
        tick: &Tick,
        inputs: &FinishInputs,
        config: &ControllersConfig,
    ) -> [bool; 2] {
        let reload = &config.weapons.reload;
        let dominant = config.acting_hand();
        let gun_hands = tick.guns.map(|g| g.is_some());
        let dt = if inputs.dt.is_finite() {
            inputs.dt.max(0.0)
        } else {
            0.0
        };
        if self.drawn != Some((tick.weapon, gun_hands)) {
            self.drawn = Some((tick.weapon, gun_hands));
            self.since_draw_s = 0.0;
            self.status.insertion.reset();
            self.status.insert_hands = None;
            self.status.work.reset();
            self.release_actions();
        } else {
            self.since_draw_s += dt;
        }
        let chambering = chambering(&tick.plan, reload);
        self.fold_shots(tick, reload, chambering);
        self.sync(tick, dominant, reload);

        self.ejects(tick, inputs, config);
        self.insert(tick, inputs, config);
        self.belt(tick, config, dt);
        self.work(tick, inputs, config, chambering);

        self.last_grips = tick.hands.map(|h| h.map(|h| h.grip.position));
        let record = *self.record(tick);
        self.status.weapon = Some(WeaponReadout {
            name_hash: tick.reading.name_hash,
            plan: tick.plan,
            from_table: tick.from_table,
            magazine: tick.reading.magazine,
            capacity: tick.reading.capacity,
            reserve: tick.reserve,
            unlimited: tick.reading.unlimited,
            out: tick.reading.out,
            chambers: Hand::BOTH
                .map(|h| gun_hands[h.index()].then_some(record.chambers[h.index()])),
            actions: Hand::BOTH.map(|h| {
                (gun_hands[h.index()] && chambering && reload.action.physical)
                    .then_some(record.travels[h.index()])
            }),
            split: record.split.filter(|_| tick.split),
        });
        self.status.belt = self.belts.map(|b| b.stage);
        let held = Hand::BOTH.map(|hand| {
            let i = hand.index();
            gun_hands[i]
                && (self.belts[i].holds_fire()
                    || (chambering && record.chambers[i] == Chamber::Empty)
                    || (chambering
                        && reload.action.physical
                        && !record.travels[i].in_battery(&reload.action))
                    || match record.split.filter(|_| tick.split) {
                        Some(split) => !split.can_fire(hand),
                        None => tick.reading.out,
                    })
        });
        for hand in Hand::BOTH {
            if held[hand.index()] && inputs.triggers[hand.index()].pressed {
                self.status.counts.dry_fires += 1;
                haptics::play(hand, Event::DryFire);
            }
        }
        held
    }

    /// Fold the weapon's shots into the chambers and the split.
    fn fold_shots(&mut self, tick: &Tick, reload: &ReloadConfig, chambering: bool) {
        let shots = gate::take_shots();
        let capacity = tick.reading.capacity;
        let dominant_split = tick.split;
        let dominant = tick.dominant;
        let mut locked = false;
        let record = self.record(tick);
        for shot in shots.into_iter().filter(|s| s.weapon == tick.weapon) {
            if dominant_split {
                let split = record.split.get_or_insert_with(|| {
                    SplitMagazines::dealt(shot.magazine + 1, capacity, dominant)
                });
                split.on_shot(shot.hand, shot.magazine, capacity, dominant);
            }
            if chambering {
                let left = match record.split.filter(|_| dominant_split) {
                    Some(split) => split.hands[shot.hand.index()].rounds,
                    None => shot.magazine,
                };
                record.chambers[shot.hand.index()] =
                    chamber::after_shot(left, tick.plan.action, reload.manual_action);
                if locks_back(&tick.plan, reload) && left <= 0 {
                    record.travels[shot.hand.index()].lock_back();
                    locked = true;
                }
            }
        }
        if locked {
            self.status.counts.lock_backs += 1;
            self.event("the last shot locked the slide back".to_owned());
            tracing::info!(target: "controllers", "reload: the last shot locked the slide back");
        }
    }

    /// Bring the record in line with the game: deal the split, and empty the chamber of a weapon
    /// first seen with an empty magazine.
    fn sync(&mut self, tick: &Tick, dominant: Hand, reload: &ReloadConfig) {
        let magazine = tick.reading.magazine;
        let capacity = tick.reading.capacity;
        let record = self.record(tick);
        let previous = record.last_magazine.replace(magazine);
        if tick.split {
            match &mut record.split {
                Some(split) => {
                    split.sync(magazine, capacity, dominant);
                }
                None => record.split = Some(SplitMagazines::dealt(magazine, capacity, dominant)),
            }
        } else {
            record.split = None;
        }
        if previous.is_none() && magazine <= 0 {
            record.chambers = [Chamber::Empty; 2];
            if locks_back(&tick.plan, reload) {
                record.travels.iter_mut().for_each(ActionTravel::lock_back);
            }
        }
    }

    /// The eject triggers.
    fn ejects(&mut self, tick: &Tick, inputs: &FinishInputs, config: &ControllersConfig) {
        let reload = &config.weapons.reload;
        let tapped = self.tap.step(inputs.reload_button, inputs.dt, reload.tap_s);
        let mut targets = [false; 2];
        if tapped && reload.button == ReloadButton::Eject {
            if tick.split {
                targets = tick.guns.map(|g| g.is_some());
            } else {
                targets[tick.holder.index()] = true;
            }
        }
        for hand in Hand::BOTH {
            let i = hand.index();
            let orientation = tick.guns[i].map(|g| g.wrist.orientation);
            let flicked = self.flicks[i].step(
                orientation,
                inputs.dt,
                reload.flick_speed_dps,
                reload.flick_cooldown_s,
            );
            self.status.flick_dps[i] = self.flicks[i].speed_dps;
            if flicked && reload.flick_eject {
                targets[i] = true;
            }
        }
        if !tick.plan.load.ejects() {
            return;
        }
        for hand in Hand::BOTH.into_iter().filter(|h| targets[h.index()]) {
            self.eject(tick, hand, config);
        }
    }

    /// Eject `hand`'s gun's magazine, when it is in.
    fn eject(&mut self, tick: &Tick, hand: Hand, config: &ControllersConfig) -> bool {
        let Some(gun) = tick.guns[hand.index()] else {
            return false;
        };
        let unlimited = tick.reading.unlimited;
        let record = self.record(tick);
        let ejected = match record.split.as_mut().filter(|_| tick.split) {
            Some(split) => match split.eject(hand, unlimited) {
                Some(transfer) => {
                    // SAFETY: main thread, outside the world update; the weapon was read this tick.
                    unsafe {
                        engine::drop_magazine(
                            tick.weapon,
                            gun.index,
                            false,
                            config.weapons.reload.loose.enabled,
                        );
                        engine::transfer(tick.weapon, transfer);
                    }
                    true
                }
                None => false,
            },
            None if !tick.reading.out => {
                // SAFETY: as above.
                unsafe {
                    engine::drop_magazine(
                        tick.weapon,
                        gun.index,
                        true,
                        config.weapons.reload.loose.enabled,
                    )
                };
                true
            }
            None => false,
        };
        if ejected {
            self.status.counts.ejects += 1;
            self.event(format!("{hand:?} ejected"));
            haptics::play(hand, Event::Eject);
        }
        ejected
    }

    /// The two-handed load: take from the pouch, carry to the well, seat.
    fn insert(&mut self, tick: &Tick, inputs: &FinishInputs, config: &ControllersConfig) {
        let reload = &config.weapons.reload;
        let has_ammo = tick.reading.unlimited || tick.reserve > 0;
        let taker = Hand::BOTH
            .into_iter()
            .find(|h| inputs.grabs[h.index()] == Some(GrabVolume::MagazinePouch));
        let from_world = Hand::BOTH
            .into_iter()
            .find(|h| inputs.grabs[h.index()] == Some(GrabVolume::WorldMagazine))
            .filter(|_| !self.status.insertion.holding())
            .and_then(|hand| {
                let grip = tick.hands[hand.index()]?.grip.position;
                loose::nearest(&self.loose_seen, grip, reload.loose.radius_m).map(|s| (hand, s))
            });
        let mut taken = false;
        if let Some((hand, sighting)) = from_world {
            let gun_hand = if tick.guns[hand.other().index()].is_some() {
                hand.other()
            } else {
                tick.holder
            };
            loose::take(hand, sighting);
            self.status.insert_hands = Some((hand, gun_hand));
            taken = true;
            self.event(format!(
                "{hand:?} took a magazine of {} from the world",
                sighting.magazine.rounds
            ));
            haptics::play(hand, Event::PouchTake);
        } else if let Some(hand) = taker.filter(|_| !self.status.insertion.holding()) {
            if has_ammo {
                let gun_hand = if tick.guns[hand.other().index()].is_some() {
                    hand.other()
                } else {
                    tick.holder
                };
                self.status.insert_hands = Some((hand, gun_hand));
                taken = true;
                haptics::play(hand, Event::PouchTake);
            } else {
                self.status.counts.empty_pouch += 1;
                haptics::play(hand, Event::PouchEmpty);
            }
        }
        let Some((hand, gun_hand)) = self.status.insert_hands else {
            return;
        };
        let can_take = self.can_take(tick, gun_hand);
        let grip = self.grip_motion(tick, hand, inputs.dt);
        let well = tick.guns[gun_hand.index()]
            .and_then(|g| g.well)
            .filter(|_| can_take);
        let event = self.status.insertion.step(
            InsertInputs {
                taken,
                grip_held: inputs.grips[hand.index()].held(),
                held: grip.and_then(|(pose, velocity)| {
                    insert::held_round(
                        pose.position,
                        frames::pointing(pose),
                        reload.held_top_m,
                        velocity,
                    )
                }),
                well,
                dt: inputs.dt,
            },
            reload,
        );
        if self.status.insertion.stage == InsertStage::Idle {
            self.status.insert_hands = None;
        }
        let world_magazine = loose::held().is_some();
        if world_magazine
            && let Some((pose, velocity)) = grip
            && let Some(round) = insert::held_round(
                pose.position,
                frames::pointing(pose),
                reload.held_top_m,
                velocity,
            )
            && let Some(world) = self.frames
        {
            let top = world.cockpit_to_world(round.top);
            let axis = world.view * round.axis;
            // SAFETY: main thread, outside the world update.
            if !unsafe { loose::carry(top, axis, inputs.dt, &reload.loose) } {
                self.event("the magazine from the world is gone".to_owned());
            }
        }
        // A magazine part rides the hand while it is held, and hides again when it is dropped
        // with the gun's magazine still out. A magazine from the world is its own object in the
        // hand, so the part stays hidden until it seats.
        if tick.plan.part_top_m.is_some() && !world_magazine {
            match event {
                // SAFETY (both): main thread, outside the world update; the weapon was read this
                // tick.
                Some(InsertEvent::Taken) => unsafe {
                    engine::show_magazine_part(tick.weapon, true)
                },
                Some(InsertEvent::Dropped) => unsafe {
                    engine::show_magazine_part(tick.weapon, false)
                },
                _ => {}
            }
        }
        match event {
            Some(InsertEvent::Captured) => {
                pulse_both(Event::WellCatch);
            }
            Some(InsertEvent::Seated) => {
                // SAFETY (both): main thread, outside the world update; the weapon was read this
                // tick.
                let loaded = match unsafe { loose::seat() } {
                    Some(rounds) => {
                        unsafe { engine::load_rounds(tick.weapon, rounds) };
                        if tick.plan.action == ActionKind::None {
                            self.record(tick).chambers[gun_hand.index()] = Chamber::Chambered;
                        }
                        true
                    }
                    None => self.load(tick, gun_hand, config),
                };
                if loaded {
                    self.status.counts.seats += 1;
                    self.event(format!("{gun_hand:?} seated"));
                    pulse_both(Event::MagazineSeat);
                }
            }
            Some(InsertEvent::Dropped) => {
                loose::let_go();
                self.event("round dropped".to_owned());
            }
            _ => {}
        }
    }

    /// The belt pushes of each gun.
    fn belt(&mut self, tick: &Tick, config: &ControllersConfig, dt: f32) {
        let reload = &config.weapons.reload;
        let has_ammo = tick.reading.unlimited || tick.reserve > 0;
        for hand in Hand::BOTH {
            let i = hand.index();
            // The pouch is placed where the player's own hand reaches it, and the gun rides the
            // drawn hand, so the mouth is carried back by the remap's shift before they meet.
            let well = tick.guns[i].and_then(|g| {
                g.well.map(|w| Well {
                    mouth: belt::into_body_frame(w.mouth, g.drawn_grip, g.body_grip),
                    ..w
                })
            });
            let velocity = match (well, self.last_mouths[i]) {
                (Some(w), Some(last)) if dt > 1e-4 => (w.mouth - last) / dt,
                _ => Vec3::ZERO,
            };
            self.last_mouths[i] = well.map(|w| w.mouth);
            let holding_for_it = self.status.insert_hands.is_some_and(|(_, g)| g == hand);
            let eligible = reload.belt.enabled
                && has_ammo
                && !holding_for_it
                && self.since_draw_s >= reload.belt.after_draw_s
                && self.can_belt(tick, hand);
            let inputs = well
                .zip(self.pouch)
                .map(|(w, (centre, radius))| PushInputs {
                    mouth: w.mouth,
                    axis: w.axis,
                    velocity,
                    pouch_centre: centre,
                    pouch_radius: radius,
                    eligible,
                });
            if !self.belts[i].step(inputs, &reload.belt) {
                continue;
            }
            // A magazine still in drops as the fresh one goes in; the split returns its rounds
            // itself, and the game's reload returns the rest.
            if tick.plan.load.ejects() {
                let record = *self.record(tick);
                let magazine_in = match record.split.filter(|_| tick.split) {
                    Some(split) => !split.hands[i].out,
                    None => !tick.reading.out,
                };
                if let Some(gun) = tick.guns[i].filter(|_| magazine_in) {
                    // SAFETY: main thread, outside the world update; the weapon was read this
                    // tick.
                    unsafe {
                        engine::drop_magazine(
                            tick.weapon,
                            gun.index,
                            !tick.split,
                            reload.loose.enabled,
                        )
                    };
                }
            }
            if self.load(tick, hand, config) {
                self.status.counts.belt_reloads += 1;
                self.event(format!("{hand:?} belt reload"));
                let record = self.record(tick);
                record.chambers[i] = Chamber::Chambered;
                record.travels[i].close();
                haptics::play(hand, Event::BeltSeat);
            }
        }
    }

    /// Whether `gun_hand`'s gun can take a round from the hand now.
    fn can_take(&mut self, tick: &Tick, gun_hand: Hand) -> bool {
        let record = *self.record(tick);
        match tick.plan.load {
            LoadKind::Magazine | LoadKind::Cylinder => match record.split.filter(|_| tick.split) {
                Some(split) => split.hands[gun_hand.index()].out,
                None => tick.reading.out,
            },
            LoadKind::Shell | LoadKind::Rocket => tick.reading.magazine < tick.reading.capacity,
        }
    }

    /// Whether `hand`'s gun can be reloaded from the belt now: anything short of a full, seated
    /// magazine.
    fn can_belt(&mut self, tick: &Tick, hand: Hand) -> bool {
        let record = *self.record(tick);
        match record.split.filter(|_| tick.split) {
            Some(split) => {
                let h = split.hands[hand.index()];
                h.out || h.rounds < dual::hand_capacity(tick.reading.capacity, hand, tick.dominant)
            }
            None => tick.reading.out || tick.reading.magazine < tick.reading.capacity,
        }
    }

    /// Load `gun_hand`'s gun: a whole magazine or one shell. Returns whether anything loaded.
    fn load(&mut self, tick: &Tick, gun_hand: Hand, config: &ControllersConfig) -> bool {
        let unlimited = tick.reading.unlimited;
        let reserve = tick.reserve;
        let capacity = tick.reading.capacity;
        let dominant = config.acting_hand();
        let record = self.record(tick);
        // SAFETY (each engine call): main thread, outside the world update; the weapon was read
        // this tick.
        let loaded = match (tick.plan.load, record.split.as_mut().filter(|_| tick.split)) {
            (LoadKind::Shell, _) => unsafe { engine::load_one(tick.weapon) },
            (_, Some(split)) => {
                match split.load(gun_hand, capacity, dominant, reserve, unlimited) {
                    Some(transfer) => {
                        unsafe { engine::transfer(tick.weapon, transfer) };
                        true
                    }
                    None => false,
                }
            }
            (_, None) => unsafe { engine::load_full(tick.weapon) },
        };
        if loaded && tick.plan.action == ActionKind::None {
            record.chambers[gun_hand.index()] = Chamber::Chambered;
        }
        loaded
    }

    /// The grip pose of `hand` this tick and its velocity since the last.
    fn grip_motion(&self, tick: &Tick, hand: Hand, dt: f32) -> Option<(Pose, Vec3)> {
        let pose = tick.hands[hand.index()]?.grip;
        let velocity = match self.last_grips[hand.index()] {
            Some(last) if dt > 1e-4 => (pose.position - last) / dt,
            _ => Vec3::ZERO,
        };
        Some((pose, velocity))
    }

    pub(super) fn event(&mut self, event: String) {
        tracing::debug!(target: "controllers", event, "reload");
        self.status.events.push(event);
        let excess = self.status.events.len().saturating_sub(MAX_EVENTS);
        self.status.events.drain(..excess);
    }

    pub(super) fn clear_tick(&mut self) {
        self.tick = None;
        self.pouch = None;
        self.free = None;
    }

    pub(super) fn stand_down(&mut self) {
        loose::let_go();
        self.clear_tick();
        self.status.active = false;
        self.status.weapon = None;
        self.status.insertion.reset();
        self.status.insert_hands = None;
        self.status.work.reset();
        self.release_actions();
        set_pump_slide(None);
        self.status.fire_held = [false; 2];
        self.belts = [BeltPush::new(); 2];
        self.flicks.iter_mut().for_each(FlickDetector::reset);
        self.last_grips = [None; 2];
        self.last_mouths = [None; 2];
        self.drawn = None;
    }
}

/// Whether the plan's chamber is the mod's to gate: the chamber is worked after running dry, or a
/// bolt or a pump after every shot, and the action can be worked, by a handle the table places or
/// on the foregrip.
pub(super) fn chambering(plan: &ReloadPlan, reload: &ReloadConfig) -> bool {
    let workable = match plan.action {
        ActionKind::None => false,
        ActionKind::Pump => true,
        _ => plan.handle.is_some(),
    };
    workable && (reload.chamber || (reload.manual_action && plan.action.manual()))
}

fn pulse_both(event: Event) {
    for hand in Hand::BOTH {
        haptics::play(hand, event);
    }
}
