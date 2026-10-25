//! The reload's input-tick steps over the hands' guns, each with its own weapon: the shots folded
//! in, the ejects, the two-handed load, the belt pushes, the action's work, and the fire hold.

use glam::Vec3;
use jc3gi::equipment::weapon::WeaponBase;

use crate::controllers::{
    config::ControllersConfig,
    frame::{Hand, Pose},
    grab::{Candidate, GrabVolume},
    hands::{ActiveHands, ik},
    haptics::{self, Event},
    loadout::live::item_of,
    weapons::{
        self,
        engine::HeldWeapon,
        foregrip::set_pump_slide,
        regions::{action_back, action_volume},
        reload::{
            FinishInputs, GunTick, MAX_EVENTS, State, Tick, WeaponReadout, WeaponTick,
            action::locks_back,
            belt::{BeltPush, PushInputs},
            chamber::{self, ActionKind, Chamber},
            config::{LooseMagazineConfig, ReloadButton, ReloadConfig},
            eject::{self, FlickDetector},
            engine,
            extraction::MagazineId,
            frames::{self, GunFrame},
            gate,
            insert::{self, InsertEvent, InsertInputs, InsertStage, InsertionSource, Well},
            loose,
            plan::{self, LoadKind, ReloadPlan, WeaponFacts},
            records::RecordKey,
            serial,
            travel::ActionTravel,
            wells,
        },
    },
};

trait MagazineInsertion {
    fn held(&self) -> bool;
    fn take(&mut self, hand: Hand, sighting: loose::Sighting) -> bool;
    fn carry(
        &mut self,
        top: Vec3,
        axis: Vec3,
        dt: f32,
        config: &LooseMagazineConfig,
    ) -> loose::Operation<()>;
    fn seat(&mut self) -> loose::Operation<i32>;
    fn release(&mut self);
    fn load_rounds(&mut self, weapon: usize, rounds: i32);
}

struct NativeMagazine;

impl MagazineInsertion for NativeMagazine {
    fn held(&self) -> bool {
        loose::held().is_some()
    }
    fn take(&mut self, hand: Hand, sighting: loose::Sighting) -> bool {
        loose::take(hand, sighting)
    }
    fn carry(
        &mut self,
        top: Vec3,
        axis: Vec3,
        dt: f32,
        config: &LooseMagazineConfig,
    ) -> loose::Operation<()> {
        unsafe { loose::carry(top, axis, dt, config) }
    }
    fn seat(&mut self) -> loose::Operation<i32> {
        unsafe { loose::seat() }
    }
    fn release(&mut self) {
        loose::let_go();
    }
    fn load_rounds(&mut self, weapon: usize, rounds: i32) {
        unsafe { engine::load_rounds(weapon, rounds) };
    }
}

impl State {
    /// Read the hands' weapons, their plans, and their guns' frames.
    ///
    /// # Safety
    ///
    /// Main thread, outside the world update, with `weapons` read this tick.
    pub(super) unsafe fn read_tick(
        &mut self,
        weapons: [Option<&HeldWeapon>; 2],
        active: &ActiveHands,
        config: &ControllersConfig,
    ) -> Option<Tick> {
        // The guns as they are shown: turned onto the line between the hands by the two-handed pose
        // and by their weight, so the wells and the handles are where the player sees them.
        let posed = weapons::weighted_hands(active);
        let targets = ik::tick_records().current;
        let mut read = [None; 2];
        let mut guns = [None; 2];
        let mut marked = [false; 2];
        for (slot, weapon) in weapons.into_iter().enumerate() {
            let Some(weapon) = weapon else {
                continue;
            };
            let Some(base) = (unsafe { (weapon.address as *const WeaponBase).as_ref() }) else {
                continue;
            };
            let Some(weapon_tick) = (unsafe { weapon_tick(weapon, base, config) }) else {
                continue;
            };
            for hand in Hand::BOTH {
                let i = hand.index();
                // A hand holds one gun; the wielded weapon's comes first.
                if !weapon.wielded.hands[i] || marked[i] {
                    continue;
                }
                marked[i] = true;
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
                let plan = &weapon_tick.plan;
                let frame = in_wrist.map(|p| GunFrame::of(cockpit.wrist, p, active.world_scale));
                let well = frames::well(plan.well, frame.as_ref(), cockpit.wrist, hand)
                    .map(|(mouth, axis)| Well { mouth, axis });
                let handle = frame
                    .zip(plan.handle)
                    .map(|(f, h)| (f.point(h), f.rearward()));
                guns[i] = Some(GunTick {
                    weapon: slot,
                    index,
                    // SAFETY: tick construction reads the live inventory on the input thread.
                    magazine: unsafe {
                        serial::capture(MagazineId {
                            weapon: weapon.address,
                            gun: index,
                            holder: hand,
                        })
                    },
                    wrist: cockpit.wrist,
                    frame,
                    well,
                    handle,
                });
            }
            read[slot] = Some(weapon_tick);
        }
        for hand in Hand::BOTH {
            if !marked[hand.index()] {
                self.measured[hand.index()] = None;
            }
        }
        if read.iter().all(Option::is_none) {
            return None;
        }
        Some(Tick {
            weapons: read,
            dominant: config.acting_hand(),
            guns,
            hands: active.hands,
            world_scale: active.world_scale,
        })
    }

    /// The action handle's grab volume, while a gun's chamber or action waits on it and a hand is
    /// free: the first such gun's.
    pub(super) fn handle_candidate(
        &mut self,
        tick: &Tick,
        free: Option<Hand>,
        reload: &ReloadConfig,
    ) -> Option<Candidate> {
        let free = free?;
        let action = &reload.action;
        let (weapon, gun, record) = Hand::BOTH.into_iter().find_map(|hand| {
            let (weapon, gun) = tick.gun(hand)?;
            if !chambering(&weapon.plan, reload) || weapon.plan.action == ActionKind::Pump {
                return None;
            }
            let record = *self.records.get(weapon.key);
            let e = gun.entry();
            (record.chambers[e] == Chamber::Empty
                || (action.physical && !record.travels[e].in_battery(action)))
            .then_some((weapon, gun, record))
        })?;
        let (handle, rearward) = gun.handle?;
        // The handle rides the part, so a slide held back is taken where it is.
        let back = if action.physical {
            let travel = &record.travels[gun.entry()];
            action_back(travel.position, weapon.plan.travel_m, tick.world_scale)
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

    /// The tick's steps; returns the hands whose guns may not fire.
    pub(super) fn step(
        &mut self,
        tick: &Tick,
        inputs: &FinishInputs,
        config: &ControllersConfig,
    ) -> [bool; 2] {
        let reload = &config.weapons.reload;
        let dt = if inputs.dt.is_finite() {
            inputs.dt.max(0.0)
        } else {
            0.0
        };
        let drawn = Hand::BOTH.map(|hand| tick.gun(hand).map(|(weapon, _)| weapon.address));
        if self.drawn != Some(drawn) {
            self.drawn = Some(drawn);
            self.since_draw_s = 0.0;
            loose::let_go();
            self.cancel_extraction();
            self.status.insertion.reset();
            self.status.insert_hands = None;
            self.status.work.reset();
            self.release_actions();
        } else {
            self.since_draw_s += dt;
        }
        for weapon in tick.weapons.iter().flatten() {
            self.records.touch(weapon.key);
        }
        self.fold_shots(tick, reload);
        self.sync(tick, reload);

        self.ejects(tick, inputs, config);
        self.insert(tick, inputs, config);
        self.extract(tick, inputs, reload);
        self.belt(tick, config, dt);
        self.work(tick, inputs, config);

        self.last_grips = tick.hands.map(|h| h.map(|h| h.grip.position));
        self.status.weapons = Hand::BOTH.map(|hand| self.readout(tick, hand, reload));
        self.status.belt = self.belts.map(|b| b.stage);
        let held = Hand::BOTH.map(|hand| self.fire_hold(tick, hand, reload));
        for hand in Hand::BOTH {
            if held[hand.index()] && inputs.triggers[hand.index()].pressed {
                self.status.counts.dry_fires += 1;
                haptics::play(hand, Event::DryFire);
            }
        }
        held
    }

    /// `hand`'s gun and its weapon, for the readout.
    fn readout(&mut self, tick: &Tick, hand: Hand, reload: &ReloadConfig) -> Option<WeaponReadout> {
        let (weapon, gun) = tick.gun(hand)?;
        let record = *self.records.get(weapon.key);
        let e = gun.entry();
        Some(WeaponReadout {
            address: weapon.address,
            name_hash: weapon.reading.name_hash,
            plan: weapon.plan,
            from_table: weapon.from_table,
            magazine: weapon.reading.magazine,
            capacity: weapon.reading.capacity,
            reserve: weapon.reserve,
            unlimited: weapon.reading.unlimited,
            out: weapon.reading.out,
            chamber: record.chambers[e],
            action: (chambering(&weapon.plan, reload) && reload.action.physical)
                .then_some(record.travels[e]),
        })
    }

    /// Whether `hand`'s gun may not fire: it sits in the belt pouch, its chamber is empty, its
    /// action is out of battery, or its magazine is out.
    fn fire_hold(&mut self, tick: &Tick, hand: Hand, reload: &ReloadConfig) -> bool {
        let Some((weapon, gun)) = tick.gun(hand) else {
            return false;
        };
        let record = *self.records.get(weapon.key);
        let e = gun.entry();
        let chambering = chambering(&weapon.plan, reload);
        self.belts[hand.index()].holds_fire()
            || self.extraction_hand() == Some(hand.other())
            || (self
                .status
                .insert_hands
                .is_some_and(|(_, target)| target == hand)
                && loose::held().is_some())
            || (chambering && record.chambers[e] == Chamber::Empty)
            || (chambering
                && reload.action.physical
                && !record.travels[e].in_battery(&reload.action))
            || weapon.reading.out
    }

    /// Fold each weapon's shots into its chambers.
    fn fold_shots(&mut self, tick: &Tick, reload: &ReloadConfig) {
        let mut locked = false;
        for shot in gate::take_shots() {
            let Some(weapon) = tick.weapon(shot.weapon) else {
                continue;
            };
            let entry = tick
                .gun(shot.hand)
                .filter(|(w, _)| w.address == shot.weapon)
                .map(|(_, gun)| gun.entry());
            let record = self.records.get(weapon.key);
            if chambering(&weapon.plan, reload)
                && let Some(entry) = entry
            {
                let left = shot.magazine;
                record.chambers[entry] =
                    chamber::after_shot(left, weapon.plan.action, reload.manual_action);
                if locks_back(&weapon.plan, reload) && left <= 0 {
                    record.travels[entry].lock_back();
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

    /// Bring each weapon's record in line with the game: empty the chambers of a weapon first seen
    /// with an empty magazine.
    fn sync(&mut self, tick: &Tick, reload: &ReloadConfig) {
        for weapon in tick.weapons.iter().flatten() {
            let magazine = weapon.reading.magazine;
            let record = self.records.get(weapon.key);
            let previous = record.last_magazine.replace(magazine);
            if previous.is_none() && magazine <= 0 {
                record.chambers = [Chamber::Empty; 2];
                if locks_back(&weapon.plan, reload) {
                    record.travels.iter_mut().for_each(ActionTravel::lock_back);
                }
            }
        }
    }

    /// The eject triggers.
    fn ejects(&mut self, tick: &Tick, inputs: &FinishInputs, config: &ControllersConfig) {
        let reload = &config.weapons.reload;
        let tapped = self.tap.step(inputs.reload_button, inputs.dt, reload.tap_s);
        let mut targets = [false; 2];
        if tapped && reload.button == ReloadButton::Eject {
            let guns = Hand::BOTH.map(|hand| tick.guns[hand.index()].is_some());
            targets = eject::tap_targets(guns, tick.dominant);
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
        for hand in Hand::BOTH.into_iter().filter(|h| targets[h.index()]) {
            if let Some((weapon, gun)) = tick.gun(hand)
                && weapon.plan.load.ejects()
            {
                self.eject(weapon, gun, hand, config);
            }
        }
    }

    /// Eject the magazine of `gun` of `weapon`, in `hand`, when it is in.
    fn eject(
        &mut self,
        weapon: &WeaponTick,
        gun: &GunTick,
        hand: Hand,
        config: &ControllersConfig,
    ) {
        let loose = config.weapons.reload.loose.enabled;
        let ejected = !weapon.reading.out;
        let outcome = if ejected {
            // SAFETY: this legacy path retains its existing execution boundary; physics write
            // authority is not established by the input tick and remains a separate engine risk.
            unsafe { engine::drop_magazine(weapon.address, gun.index, loose) }
        } else {
            engine::DetachOutcome::NoThrow
        };
        if outcome != engine::DetachOutcome::NoThrow {
            self.status.counts.ejects += 1;
            self.event(format!("{hand:?} ejected"));
            haptics::play(hand, Event::Eject);
        }
    }

    /// The two-handed load: take from the pouch, carry to the well of the gun in the other hand,
    /// seat.
    fn insert(&mut self, tick: &Tick, inputs: &FinishInputs, config: &ControllersConfig) {
        self.insert_with(tick, inputs, config, &mut NativeMagazine);
    }

    fn insert_with(
        &mut self,
        tick: &Tick,
        inputs: &FinishInputs,
        config: &ControllersConfig,
        adapter: &mut impl MagazineInsertion,
    ) {
        let reload = &config.weapons.reload;
        if !reload.loose.enabled && adapter.held() {
            adapter.release();
            self.status.insertion.reset();
            self.status.insert_hands = None;
        }
        let gun_hands = tick.gun_hands();
        let loading = |taker: Hand| insert::loading_hand(gun_hands, Some(taker), tick.dominant);
        let taker = Hand::BOTH
            .into_iter()
            .find(|h| inputs.grabs[h.index()] == Some(GrabVolume::MagazinePouch));
        let from_world = Hand::BOTH
            .into_iter()
            .find(|h| inputs.grabs[h.index()] == Some(GrabVolume::WorldMagazine))
            .filter(|_| !self.status.insertion.holding())
            .and_then(|hand| {
                let gun_hand = loading(hand)?;
                let grip = tick.hands[hand.index()]?.grip.position;
                loose::nearest(&self.loose_seen, grip, reload.loose.radius_m)
                    .map(|s| (hand, gun_hand, s))
            });
        let mut taken = false;
        if let Some((hand, gun_hand, sighting)) = from_world {
            if !adapter.take(hand, sighting) {
                return;
            }
            self.status.insert_hands = Some((hand, gun_hand));
            self.insertion_source = InsertionSource::RetainedMagazine;
            taken = true;
            self.event(format!(
                "{hand:?} took a magazine of {} from the world",
                sighting.magazine.rounds
            ));
            haptics::play(hand, Event::PouchTake);
        } else if let Some(hand) = taker.filter(|_| !self.status.insertion.holding())
            && let Some(gun_hand) = loading(hand)
        {
            if tick.gun(gun_hand).is_some_and(|(w, _)| w.has_ammo()) {
                self.status.insert_hands = Some((hand, gun_hand));
                self.insertion_source = InsertionSource::Pouch;
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
        let target = tick.gun(gun_hand);
        let can_take = target.is_some_and(|(weapon, _)| self.can_take(weapon));
        let grip = self.grip_motion(tick, hand, inputs.dt);
        let well = target.and_then(|(_, gun)| gun.well).filter(|_| can_take);
        let prior = self.status.insertion;
        let before_hands = self.status.insert_hands;
        let event = self.status.insertion.step_with_source(
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
            self.insertion_source,
            reload,
        );
        // Taking commits ownership, so deferred work must retain its Holding state and count.
        let before = if event == Some(InsertEvent::Taken) {
            self.status.insertion
        } else {
            prior
        };
        if self.status.insertion.stage == InsertStage::Idle {
            self.status.insert_hands = None;
        }
        let world_magazine = adapter.held();
        if world_magazine && event == Some(InsertEvent::Dropped) {
            adapter.release();
            self.event("round dropped".to_owned());
            return;
        }
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
            match adapter.carry(top, axis, inputs.dt, &reload.loose) {
                loose::Operation::Ready(()) => {}
                loose::Operation::Deferred => {
                    if inputs.grips[hand.index()].held() {
                        self.status.insertion = before;
                        self.status.insert_hands = before_hands;
                    } else {
                        adapter.release();
                        self.status.insertion.reset();
                        self.status.insert_hands = None;
                    }
                    return;
                }
                loose::Operation::Gone => {
                    adapter.release();
                    self.status.insertion.reset();
                    self.status.insert_hands = None;
                    return;
                }
            }
        }
        // A magazine part rides the hand while it is held, and hides again when it is dropped
        // with the gun's magazine still out. A magazine from the world is its own object in the
        // hand, so the part stays hidden until it seats.
        if let Some((weapon, _)) = target
            && weapon.plan.part_top_m.is_some()
            && !world_magazine
        {
            match event {
                // SAFETY (both): main thread, outside the world update; the weapon was read this
                // tick.
                Some(InsertEvent::Taken) => unsafe {
                    engine::show_magazine_part(weapon.address, true)
                },
                Some(InsertEvent::Dropped) => unsafe {
                    engine::show_magazine_part(weapon.address, false)
                },
                _ => {}
            }
        }
        match event {
            Some(InsertEvent::Captured) => {
                pulse_both(Event::WellCatch);
            }
            Some(InsertEvent::Seated) => {
                let loaded = match target {
                    // SAFETY (both): main thread, outside the world update; the weapon was read
                    // this tick.
                    Some((weapon, gun)) if world_magazine => match adapter.seat() {
                        loose::Operation::Ready(rounds) => {
                            adapter.load_rounds(weapon.address, rounds);
                            if weapon.plan.action == ActionKind::None {
                                self.records.get(weapon.key).chambers[gun.entry()] =
                                    Chamber::Chambered;
                            }
                            true
                        }
                        loose::Operation::Deferred => {
                            if inputs.grips[hand.index()].held() {
                                self.status.insertion = before;
                                self.status.insert_hands = before_hands;
                            } else {
                                adapter.release();
                                self.status.insertion.reset();
                                self.status.insert_hands = None;
                            }
                            return;
                        }
                        loose::Operation::Gone => {
                            adapter.release();
                            self.status.insertion.reset();
                            self.status.insert_hands = None;
                            return;
                        }
                    },
                    Some((weapon, gun)) => self.load(weapon, gun),
                    // The gun left the hand, so a magazine from the world goes back to it.
                    None => {
                        adapter.release();
                        false
                    }
                };
                if loaded {
                    self.status.counts.seats += 1;
                    self.event(format!("{gun_hand:?} seated"));
                    pulse_both(Event::MagazineSeat);
                }
            }
            Some(InsertEvent::Dropped) => {
                adapter.release();
                self.event("round dropped".to_owned());
            }
            _ => {}
        }
    }

    /// The belt pushes of each hand's gun.
    pub(super) fn belt(&mut self, tick: &Tick, config: &ControllersConfig, dt: f32) {
        let reload = &config.weapons.reload;
        for hand in Hand::BOTH {
            let i = hand.index();
            let target = tick.gun(hand);
            let well = target.and_then(|(_, gun)| gun.well);
            let velocity = match (well, self.last_mouths[i]) {
                (Some(w), Some(last)) if dt > 1e-4 => (w.mouth - last) / dt,
                _ => Vec3::ZERO,
            };
            self.last_mouths[i] = well.map(|w| w.mouth);
            if self.extraction_hand() == Some(hand.other()) || self.extraction_blocks_belt[i] {
                self.belts[i] = BeltPush::new();
                continue;
            }
            let holding_for_it = self.status.insert_hands.is_some_and(|(_, g)| g == hand);
            let eligible = reload.belt.enabled
                && !holding_for_it
                && self.since_draw_s >= reload.belt.after_draw_s
                && target.is_some_and(|(weapon, _)| weapon.has_ammo() && self.can_belt(weapon));
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
            let Some((weapon, gun)) = target else {
                continue;
            };
            // A magazine still in drops as the fresh one goes in, and the game's reload returns
            // its rounds.
            if weapon.plan.load.ejects() && !weapon.reading.out {
                // SAFETY: main thread, outside the world update; the weapon was read this tick.
                if unsafe { engine::drop_magazine(weapon.address, gun.index, reload.loose.enabled) }
                    == engine::DetachOutcome::NoThrow
                {
                    continue;
                }
            }
            if self.load(weapon, gun) {
                self.status.counts.belt_reloads += 1;
                self.event(format!("{hand:?} belt reload"));
                let record = self.records.get(weapon.key);
                record.chambers[gun.entry()] = Chamber::Chambered;
                record.travels[gun.entry()].close();
                haptics::play(hand, Event::BeltSeat);
            }
        }
    }

    /// Whether `weapon`'s gun in `gun_hand` can take a round from the hand now.
    fn can_take(&self, weapon: &WeaponTick) -> bool {
        match weapon.plan.load {
            LoadKind::Magazine | LoadKind::Cylinder => weapon.reading.out,
            LoadKind::Shell | LoadKind::Rocket => weapon.reading.magazine < weapon.reading.capacity,
        }
    }

    /// Whether `weapon`'s gun can be reloaded from the belt now: anything short of a full, seated
    /// magazine.
    fn can_belt(&self, weapon: &WeaponTick) -> bool {
        let reading = &weapon.reading;
        reading.out || reading.magazine < reading.capacity
    }

    /// Load `gun` of `weapon`: a whole magazine or one shell. Returns whether anything loaded.
    fn load(&mut self, weapon: &WeaponTick, gun: &GunTick) -> bool {
        let record = self.records.get(weapon.key);
        // SAFETY (each engine call): main thread, outside the world update; the weapon was read
        // this tick.
        let loaded = match weapon.plan.load {
            LoadKind::Shell => unsafe { engine::load_one(weapon.address) },
            _ => unsafe { engine::load_full(weapon.address) },
        };
        if loaded && weapon.plan.action == ActionKind::None {
            record.chambers[gun.entry()] = Chamber::Chambered;
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
        self.stocked_pouch = None;
        self.free = None;
        self.cancel_extraction();
    }

    pub(super) fn stand_down(&mut self) {
        loose::let_go();
        self.clear_tick();
        self.status.active = false;
        self.status.weapons = [None; 2];
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

/// The tick's view of `weapon`, which `base` is, and its plan. `None` for a weapon the reload
/// leaves to the game, or one with no gun in a hand.
///
/// # Safety
///
/// As [`State::read_tick`].
unsafe fn weapon_tick(
    weapon: &HeldWeapon,
    base: &WeaponBase,
    config: &ControllersConfig,
) -> Option<WeaponTick> {
    let reload = &config.weapons.reload;
    weapon.holder?;
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
    Some(WeaponTick {
        address: weapon.address,
        key: RecordKey::new(item_of(weapon.address), weapon.address),
        plan,
        from_table: entry.is_some(),
        reading,
        reserve: unsafe { engine::reserve(base) },
    })
}

#[cfg(test)]
mod insertion_tests;

fn pulse_both(event: Event) {
    for hand in Hand::BOTH {
        haptics::play(hand, event);
    }
}
