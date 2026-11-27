//! The reload's input-tick steps over the hands' guns, each with its own weapon: the shots folded
//! in, the ejects, the two-handed load, the belt pushes, the action's work, and the fire hold.

use glam::Vec3;
use jc3gi::equipment::weapon::WeaponBase;
use jc3vr_body::controllers::{
    grab::{Candidate, GrabVolume, resolve::nearest_within},
    hands::{ActiveHands, ik},
    haptics, mode,
    mode::Mode,
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::{
    frame::{self, Hand, Pose},
    haptics::cues::Event,
};

use crate::controllers::{
    loadout::live::item_of,
    weapons,
    weapons::{
        config::WeaponSettings,
        engine::{HeldWeapon, reserve},
        foregrip::set_pump_slide,
        inventory::engine::strong_to,
        pouch_models,
        regions::{action_back, action_volume},
        reload::{
            BeltFinish, BeltGate, FinishInputs, GunTick, HandBelt, State, Tick, WeaponReadout,
            WeaponTick,
            action::locks_back,
            ammo,
            ammo::{NativeAmmo, Observation},
            belt,
            belt::{BeltPush, PushInputs},
            chamber,
            chamber::{ActionKind, Chamber},
            cylinders, eject, engine,
            extraction::{DetachKind, MagazineId},
            fire_hold::{FireFacts, FireHold, fire_hold},
            frames,
            frames::GunFrame,
            gate, guide, infinite, insert,
            insert::{InsertEvent, InsertInputs, InsertStage, InsertionSource, Well},
            knock, loose, plan,
            plan::{LoadKind, ReloadPlan, WeaponFacts},
            records::RecordKey,
            retained,
            retained::SeatOutcome,
            serial,
            travel::ActionTravel,
            tuning::{LooseTuning, Reload},
            wells,
        },
        sounds::GunSound,
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
        config: &LooseTuning,
    ) -> loose::Operation<()>;
    fn seat(&mut self, weapon: usize) -> SeatOutcome;
    fn release(&mut self);
    /// Play the magazine-in sound on gun `gun` of the weapon at `weapon`, where a round seated.
    fn seat_sound(&mut self, weapon: usize, gun: usize);
    /// Play the refusal sound on gun `gun` of the weapon at `weapon`, whose well turned the round
    /// away.
    fn refuse_sound(&mut self, weapon: usize, gun: usize);
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
        config: &LooseTuning,
    ) -> loose::Operation<()> {
        unsafe { loose::carry(top, axis, dt, config) }
    }
    fn seat(&mut self, address: usize) -> SeatOutcome {
        let Some(magazine) = loose::held() else {
            return SeatOutcome::Gone;
        };
        let Some(weapon) = (unsafe { (address as *const WeaponBase).as_ref() }) else {
            return SeatOutcome::Gone;
        };
        let capacity =
            unsafe { crate::controllers::weapons::dual_wield::single::magazine_size(weapon) };
        retained::seat(
            magazine.rounds,
            capacity,
            || unsafe { loose::seat() },
            |rounds| unsafe { engine::load_rounds(address, rounds) },
        )
    }
    fn release(&mut self) {
        loose::let_go();
    }
    fn seat_sound(&mut self, weapon: usize, gun: usize) {
        // SAFETY: the insertion's seat runs on the game thread before the world update, with the
        // weapon read this tick.
        unsafe { seat_sound(weapon, gun) };
    }
    fn refuse_sound(&mut self, weapon: usize, gun: usize) {
        // SAFETY: as `seat_sound`.
        unsafe { refuse_sound(weapon, gun) };
    }
}

impl State {
    /// Read the hands' weapons, their plans, and their guns' frames.
    ///
    /// # Safety
    ///
    /// Game thread, before the world update, with `weapons` read this tick.
    pub(super) unsafe fn read_tick(
        &mut self,
        weapons: [Option<&HeldWeapon>; 2],
        active: &ActiveHands,
        config: &WeaponSettings,
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
            let Some(mut weapon_tick) = (unsafe { weapon_tick(weapon, base, config) }) else {
                continue;
            };
            // SAFETY: tick construction runs on the input thread with this weapon read this tick.
            unsafe { engine::borrow_spare(weapon.address, weapon_tick.reading.name_hash) };
            // Before the partition adopts the native total, so a gun first seen short, such as one
            // taken from a character, is adopted full with its chamber loaded.
            // A revolver's swung-out cylinder stays as dumped until a speedloader fills it.
            if !self.cylinder_out(weapon.address) {
                // SAFETY: tick construction runs on the input thread with this weapon read this
                // tick.
                weapon_tick.reading =
                    unsafe { infinite::top_up(weapon.address, weapon_tick.reading) };
            }
            let mut cylinder_tick = None;
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
                // SAFETY: tick construction runs on the input thread with this weapon read this
                // tick.
                let cylinder = unsafe { self.cylinder_for(hand, plan, weapon.address, index) };
                cylinder_tick = cylinder.as_ref().map(cylinders::reading).or(cylinder_tick);
                let frame = in_wrist.map(|p| GunFrame::of(cockpit.wrist, p, active.world_scale));
                let well = frames::well(
                    cylinders::well(plan.well, cylinder.as_ref()),
                    frame.as_ref(),
                    cockpit.wrist,
                    hand,
                )
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
            weapon_tick.cylinder = cylinder_tick;
            if let Some(owner) = unsafe { strong_to(base) } {
                let record = self.records.get(weapon_tick.key);
                let topology = unsafe { engine::gun_topology(base) };
                let gun_map = Hand::BOTH.map(|hand| {
                    let gun = guns[hand.index()].filter(|gun| gun.weapon == slot)?;
                    Some((gun.entry(), weapon_tick.plan.action.manual()))
                });
                let partition = ammo::observe(
                    weapon.address,
                    owner,
                    base.m_User[1] as usize,
                    Observation {
                        native: NativeAmmo {
                            total: weapon_tick.reading.magazine,
                            out: weapon_tick.reading.out,
                        },
                        chambers: record.chambers,
                        topology,
                        guns: gun_map,
                        active: chambering(&weapon_tick.plan),
                    },
                );
                weapon_tick.ammo_owned = partition.is_some();
                if let Some(partition) = partition {
                    record.chambers = partition.chambers;
                }
                weapon_tick.reading.chambers = ammo::protected_rounds(weapon.address);
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
            sequence: frame::with_latest(|frame| frame.sequence),
            weapons: read,
            dominant: config.dominant,
            guns,
            hands: active.hands,
            world_scale: active.world_scale,
        })
    }

    /// The other hand's action handle, including a ready slide or bolt with physical actions on.
    pub(super) fn handle_candidate(
        &mut self,
        tick: &Tick,
        free: Option<Hand>,
        reload: &Reload,
    ) -> Option<Candidate> {
        let free = free?;
        let action = &reload.action;
        if self.status.insertion.holding()
            || self.extraction_hand().is_some()
            || self.pending_ejects[free.other().index()].is_some()
            || self.extraction_blocks_belt[free.other().index()]
        {
            return None;
        }
        let (weapon, gun) = tick.gun(free.other())?;
        if !chambering(&weapon.plan) || weapon.plan.action == ActionKind::Pump {
            return None;
        }
        let record = *self.records.get(weapon.key);
        let e = gun.entry();
        let wanted = record.chambers[e] == Chamber::Empty
            || weapon.plan.action.ready_rack()
            || !record.travels[e].in_battery(action);
        if !wanted {
            return None;
        }
        let (handle, rearward) = gun.handle?;
        // The handle rides the part, so a slide held back is taken where it is.
        let travel = &record.travels[gun.entry()];
        let back = action_back(travel.position, weapon.plan.travel_m, tick.world_scale);
        Some(action_volume(
            handle,
            rearward,
            back,
            action.grab_radius_m,
            free,
        ))
    }

    /// The tick's steps; returns why each hand's gun may not fire, by [`Hand::index`].
    pub(super) fn step(
        &mut self,
        game: &GameTick<'_>,
        tick: &Tick,
        inputs: &FinishInputs,
        config: &WeaponSettings,
    ) -> [Option<FireHold>; 2] {
        let reload = &Reload::of(&config.weapons.reload);
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
            self.cancel_ejects();
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
        self.fold_shots(tick);
        self.sync(tick);
        self.cylinders(tick, dt);

        self.extraction_blocks_belt = [false; 2];
        self.finish_ejects(tick, config);
        self.push_knocked(tick);
        self.ejects(tick, inputs, config);
        self.insert(tick, inputs, config);
        self.knock(tick, inputs, config);
        self.extract(tick, inputs, reload);
        self.belt(tick, config);
        self.work(game, tick, inputs, config);
        self.sync_owned(tick);

        self.insertion_guide(tick, reload);
        self.last_grips = tick.hands.map(|h| h.map(|h| h.grip.position));
        let readouts = Hand::BOTH.map(|hand| self.readout(tick, hand));
        for hand in Hand::BOTH {
            log_magazine_change(
                hand,
                self.status.weapons[hand.index()],
                readouts[hand.index()],
            );
        }
        self.status.weapons = readouts;
        self.status.belt = self.belts.map(|b| b.stage);
        Hand::BOTH.map(|hand| self.fire_hold(tick, hand, reload))
    }

    /// `hand`'s gun and its weapon, for the readout.
    fn readout(&mut self, tick: &Tick, hand: Hand) -> Option<WeaponReadout> {
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
            occupied: weapon.occupied(),
            chamber: record.chambers[e],
            action: chambering(&weapon.plan).then_some(record.travels[e]),
        })
    }

    /// Why `hand`'s gun may not fire ([`fire_hold`]), or `None` when it may.
    fn fire_hold(&mut self, tick: &Tick, hand: Hand, reload: &Reload) -> Option<FireHold> {
        let (weapon, gun) = tick.gun(hand)?;
        let record = *self.records.get(weapon.key);
        let e = gun.entry();
        fire_hold(&FireFacts {
            belt: self.belts[hand.index()].holds_fire(),
            ejecting: self.pending_ejects[hand.index()].is_some(),
            extracting: self.extraction_hand() == Some(hand.other()),
            inserting: self
                .status
                .insert_hands
                .is_some_and(|(_, target)| target == hand)
                && matches!(self.status.insertion.stage, InsertStage::Captured { .. }),
            chambering: chambering(&weapon.plan),
            chamber_empty: record.chambers[e] == Chamber::Empty,
            out_of_battery: !record.travels[e].in_battery(&reload.action),
            magazine_out: weapon.reading.out,
            magazine_empty: weapon.reading.magazine <= 0,
            cylinder_open: weapon.cylinder.is_some_and(|c| !c.shut),
        })
    }

    /// Apply queued shot effects, deriving readiness only for weapons without an active partition.
    fn fold_shots(&mut self, tick: &Tick) {
        self.apply_shots(tick, gate::take_shots());
    }

    fn apply_shots(&mut self, tick: &Tick, shots: Vec<gate::Shot>) {
        let mut locked = false;
        for shot in shots {
            let Some(weapon) = tick.weapon(shot.weapon) else {
                continue;
            };
            let entry = tick
                .gun(shot.hand)
                .filter(|(w, _)| w.address == shot.weapon)
                .map(|(_, gun)| gun.entry());
            let record = self.records.get(weapon.key);
            if chambering(&weapon.plan)
                && let Some(entry) = entry
            {
                let left = shot.magazine;
                if !weapon.ammo_owned {
                    record.chambers[entry] = chamber::after_shot(left, weapon.plan.action);
                }
                if locks_back(&weapon.plan) && left <= 0 {
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
    fn sync(&mut self, tick: &Tick) {
        for weapon in tick.weapons.iter().flatten() {
            let magazine = weapon.reading.magazine;
            let record = self.records.get(weapon.key);
            let previous = record.last_magazine.replace(magazine);
            if previous.is_none() && magazine <= 0 {
                if !weapon.ammo_owned {
                    record.chambers = [Chamber::Empty; 2];
                }
                if locks_back(&weapon.plan) {
                    record.travels.iter_mut().for_each(ActionTravel::lock_back);
                }
            }
        }
        self.sync_owned(tick);
    }

    fn sync_owned(&mut self, tick: &Tick) {
        for weapon in tick.weapons.iter().flatten() {
            if weapon.ammo_owned
                && let Some(ammo) = ammo::active_snapshot(weapon.address)
            {
                self.records.get(weapon.key).chambers = ammo.chambers;
            }
        }
    }

    /// The eject triggers.
    fn ejects(&mut self, tick: &Tick, inputs: &FinishInputs, config: &WeaponSettings) {
        let reload = &Reload::of(&config.weapons.reload);
        let tapped = self.tap.step(inputs.reload_button, inputs.dt, reload.tap_s);
        let mut targets = [false; 2];
        if tapped {
            let rounds = Hand::BOTH.map(|hand| self.ejectable_rounds(tick, hand));
            if let Some(hand) = eject::tap_target(rounds, tick.dominant) {
                targets[hand.index()] = true;
            } else {
                self.event("the reload button found no magazine to eject".to_owned());
            }
        }
        // In a vehicle seat the hands are on the controls and the reload button is not bound, so
        // a double tap of the gun's fire press drops a dry gun's magazine. A loaded gun's double
        // tap is two shots, which no detector can tell from a release in time.
        let seated = matches!(mode::current().mode, Mode::Vehicle(_));
        for hand in Hand::BOTH {
            let i = hand.index();
            let double = self.fire_taps[i].step(
                inputs.fire_presses[i],
                inputs.dt,
                reload.tap_s,
                reload.double_tap_gap_s,
            );
            if seated && double && self.ejectable_rounds(tick, hand) == Some(0) {
                self.event(format!(
                    "the {hand:?} gun's double tap drops its dry magazine"
                ));
                targets[i] = true;
            }
        }
        for hand in Hand::BOTH.into_iter().filter(|h| targets[h.index()]) {
            if let Some((weapon, gun)) = tick.gun(hand)
                && weapon.plan.load.ejects()
            {
                self.eject(weapon, gun, hand);
            }
        }
    }

    /// The rounds in the magazine of `hand`'s gun, while it holds one that a tap can eject: one that
    /// ejects, with its magazine in and no eject of it already queued.
    fn ejectable_rounds(&self, tick: &Tick, hand: Hand) -> Option<i32> {
        let (weapon, gun) = tick.gun(hand)?;
        (weapon.plan.load.ejects()
            && !weapon.reading.out
            && gun.magazine.is_some()
            && self.pending_ejects[hand.index()].is_none())
        .then(|| weapon.reading.magazine_rounds())
    }

    /// Eject the magazine of `gun` of `weapon`, in `hand`, when it is in.
    fn eject(&mut self, weapon: &WeaponTick, gun: &GunTick, hand: Hand) {
        if !self.queue_eject(weapon, gun, hand, DetachKind::Eject) {
            self.event(format!(
                "the {hand:?} gun's magazine release could not drop it"
            ));
            haptics::play(hand, Event::PouchRefused);
            // SAFETY: the game stage, on the game thread; the weapon was read this tick.
            unsafe { refuse_sound(weapon.address, gun.index) };
        }
    }

    /// The two-handed load: take from the pouch, carry to the well of the gun in the other hand,
    /// seat.
    fn insert(&mut self, tick: &Tick, inputs: &FinishInputs, config: &WeaponSettings) {
        self.insert_with(tick, inputs, config, &mut NativeMagazine);
    }

    fn insert_with(
        &mut self,
        tick: &Tick,
        inputs: &FinishInputs,
        config: &WeaponSettings,
        adapter: &mut impl MagazineInsertion,
    ) {
        let reload = &Reload::of(&config.weapons.reload);
        if let Some(hand) = self.ready_cue.step(inputs.dt) {
            haptics::play(hand, Event::ReadyToFire);
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
                nearest_within(&self.loose_seen, |s| s.cockpit, grip, reload.loose.radius_m)
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
            if tick
                .gun(gun_hand)
                .is_some_and(|(weapon, _)| self.can_take_from_pouch(weapon))
            {
                self.status.insert_hands = Some((hand, gun_hand));
                self.insertion_source = InsertionSource::Pouch;
                taken = true;
                if let Some((weapon, _)) = tick.gun(gun_hand) {
                    self.event(format!(
                        "{hand:?} took a {} from the pouch for the {gun_hand:?} gun ({} in the gun, {} in reserve)",
                        weapon.plan.load.label(),
                        weapon.reading.magazine,
                        weapon.reserve
                    ));
                }
                haptics::play(hand, Event::PouchTake);
            } else {
                self.status.counts.empty_pouch += 1;
                self.event(format!("{hand:?} found the pouch empty"));
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
        let occupied = target.is_some_and(|(weapon, _)| weapon.occupied());
        let prior = self.status.insertion;
        let before_hands = self.status.insert_hands;
        let capture = target.map_or(*reload, |(weapon, _)| reload.for_plan(&weapon.plan));
        let event = self.status.insertion.step_with_source(
            InsertInputs {
                taken,
                grip_held: inputs.grips[hand.index()].held(),
                held: grip.and_then(|(pose, velocity)| {
                    insert::held_round(
                        frames::held_line(pose, hand, capture.held_palm_gap_m),
                        frames::pointing(pose),
                        reload.held_top_m,
                        velocity,
                    )
                }),
                well,
                occupied,
                dt: inputs.dt,
            },
            self.insertion_source,
            &capture,
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
                frames::held_line(pose, hand, capture.held_palm_gap_m),
                frames::pointing(pose),
                reload.held_top_m,
                velocity,
            )
            && let Some(world) = self.frames
        {
            let round = self.status.insertion.carried(round, well);
            let top = world.cockpit.point_to_world(round.top);
            let axis = world.cockpit.direction_to_world(round.axis);
            match adapter.carry(top, axis, inputs.game_dt, &reload.loose) {
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
        // hand, so the part stays hidden until it seats, and so does the part of a gun whose spare
        // magazine model draws the magazine in the hand.
        if let Some((weapon, _)) = target
            && weapon.plan.part_top_m.is_some()
            && !world_magazine
            // SAFETY: game thread before the world update; the weapon was read this tick.
            && !(weapon.plan.spare_top_m.is_some()
                && unsafe { pouch_models::hand_draws(weapon.address) })
        {
            match event {
                // SAFETY (both): game thread before the world update; the weapon was read this
                // tick.
                Some(InsertEvent::Taken) => unsafe {
                    engine::show_magazine_part(weapon.address, true)
                },
                Some(InsertEvent::Dropped) => unsafe {
                    engine::show_magazine_part(weapon.address, weapon.part_shown())
                },
                _ => {}
            }
        }
        match event {
            Some(InsertEvent::Captured) => {
                self.event(format!("the {gun_hand:?} gun's well caught the round"));
                haptics::play_hands(Hand::BOTH, Event::WellCatch);
            }
            Some(InsertEvent::Seated) => {
                let loaded = match target {
                    // SAFETY (both): game thread before the world update; the weapon was read
                    // this tick.
                    Some((weapon, gun)) if world_magazine => match adapter.seat(weapon.address) {
                        SeatOutcome::Loaded => {
                            if weapon.plan.action == ActionKind::None {
                                self.records.get(weapon.key).chambers[gun.entry()] =
                                    Chamber::Chambered;
                            }
                            true
                        }
                        SeatOutcome::Rejected { rounds, capacity } => {
                            tracing::info!(target: "controllers", rounds, capacity, "reload: retained magazine does not fit the weapon");
                            if inputs.grips[hand.index()].held() {
                                self.status.insertion = before;
                                self.status.insert_hands = before_hands;
                            } else {
                                adapter.release();
                                self.status.insertion = before;
                                self.status.insertion.reset();
                                self.status.insert_hands = None;
                            }
                            return;
                        }
                        SeatOutcome::Deferred => {
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
                        SeatOutcome::Gone => {
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
                    if let Some((weapon, gun)) = target {
                        adapter.seat_sound(weapon.address, gun.index);
                        self.seated(weapon, gun, gun_hand, Event::MagazineSeat, reload);
                    }
                } else if !world_magazine && let Some((weapon, _)) = target {
                    self.event(format!(
                        "the {gun_hand:?} gun's round went home but loaded nothing ({} in the gun, out {})",
                        weapon.reading.magazine, weapon.reading.out
                    ));
                    // SAFETY: the weapon is this tick's verified target; failed seating transfers no round.
                    unsafe { engine::show_magazine_part(weapon.address, weapon.part_shown()) };
                }
            }
            Some(InsertEvent::Pushed { speed_mps })
                if self.push_knock(tick, gun_hand, speed_mps) => {}
            Some(InsertEvent::Refused | InsertEvent::Pushed { .. }) => {
                self.event(format!(
                    "the {gun_hand:?} gun's well refused the round: it still holds one ({} m from the mouth)",
                    self.status.insertion.distance_m.map_or(f32::NAN, |d| (d * 1000.0).round() / 1000.0)
                ));
                haptics::play(hand, Event::PouchRefused);
                if let Some((weapon, gun)) = target {
                    adapter.refuse_sound(weapon.address, gun.index);
                }
            }
            Some(InsertEvent::Dropped) => {
                adapter.release();
                self.event("round dropped".to_owned());
            }
            _ => {}
        }
    }

    /// The belt pushes of each hand's gun, and the pouches shown as ready for one.
    pub(super) fn belt(&mut self, tick: &Tick, config: &WeaponSettings) {
        let reload = &Reload::of(&config.weapons.reload);
        let belt = &reload.belt;
        self.belt_rings = [None; 2];
        for hand in Hand::BOTH {
            let i = hand.index();
            let target = tick.gun(hand);
            let pushed = target.and_then(|(weapon, gun)| {
                let well = gun.well?;
                Some((belt_point(weapon, well, tick.world_scale), well.axis))
            });
            if self.extraction_hand() == Some(hand.other())
                || self.pending_ejects[i].is_some()
                || self.extraction_blocks_belt[i]
            {
                self.belts[i] = BeltPush::new();
                continue;
            }
            let eligible = self.belt_gate(tick, hand).allows(reload);
            let pouch = self.pouches[i];
            if eligible
                && let (Some((point, axis)), Some(centre)) = (pushed, pouch)
                && belt::approaching(point, axis, centre, belt.radius_m, belt.approach_m, belt)
            {
                self.belt_rings[i] = Some(centre);
            }
            let inputs = pushed.zip(pouch).map(|((point, axis), centre)| PushInputs {
                point,
                axis,
                pouch_centre: centre,
                pouch_radius: belt.radius_m,
                eligible,
            });
            if !self.belts[i].step(inputs, belt) {
                continue;
            }
            let Some((weapon, gun)) = target else {
                continue;
            };
            // A magazine still in drops as the fresh one goes in, and the game's reload returns
            // its rounds.
            if weapon.occupied() && weapon.plan.load.ejects() {
                if !self.queue_eject(weapon, gun, hand, DetachKind::Belt) {
                    // A refused drop must not leave the gun held as reloaded: on the 14 November
                    // take it read as a dead trigger until the gun left the pouch.
                    self.belts[i].refuse();
                    self.event(format!(
                        "the {hand:?} gun's belt push could not drop its magazine"
                    ));
                    haptics::play(hand, Event::PouchRefused);
                    // SAFETY: the game stage, on the game thread; the weapon was read this tick.
                    unsafe { refuse_sound(weapon.address, gun.index) };
                }
                continue;
            }
            if !self.load(weapon, gun) {
                self.event(format!("the {hand:?} gun's belt push loaded nothing"));
            } else {
                self.status.counts.belt_reloads += 1;
                self.event(format!("{hand:?} belt reload"));
                let record = self.records.get(weapon.key);
                record.chambers[gun.entry()] =
                    engine::feed_chamber(weapon.address, gun.entry()).unwrap_or(Chamber::Chambered);
                record.travels[gun.entry()].close();
                // SAFETY: game thread before the world update; the weapon was read this tick.
                unsafe { seat_sound(weapon.address, gun.index) };
                self.seated(weapon, gun, hand, Event::BeltSeat, reload);
            }
        }
    }

    pub(super) fn belt_finish(&self, tick: &Tick) -> BeltFinish {
        BeltFinish {
            sequence: tick.sequence,
            dominant: tick.dominant,
            dual: tick.gun_hands() == [true; 2],
            hands: Hand::BOTH.map(|hand| HandBelt {
                weapon: tick.gun(hand).map(|(weapon, _)| weapon.address),
                gate: self.belt_gate(tick, hand),
                well: tick.gun(hand).is_some_and(|(_, gun)| gun.well.is_some()),
                pouch: self.pouches[hand.index()].is_some(),
            }),
        }
    }

    fn belt_gate(&self, tick: &Tick, hand: Hand) -> BeltGate {
        BeltGate {
            holding: self.status.insert_hands.is_some_and(|(_, gun)| gun == hand),
            extracting: self.extraction_hand() == Some(hand.other())
                || self.pending_ejects[hand.index()].is_some(),
            blocked: self.extraction_blocks_belt[hand.index()],
            since_draw_s: self.since_draw_s,
            has_ammo: tick.gun(hand).is_some_and(|(weapon, _)| weapon.has_ammo()),
            can_belt: tick
                .gun(hand)
                .is_some_and(|(weapon, _)| self.can_belt(weapon)),
        }
    }

    /// Whether `weapon`'s gun in `gun_hand` can take a round from the hand now: a magazine at any
    /// time, held until the one still in leaves ([`State::knock`]), and a shell, a rocket, a
    /// revolver's cylinder of rounds, or a one-round launcher's round while it has space or its
    /// round is out.
    pub(super) fn can_take(&self, weapon: &WeaponTick) -> bool {
        match weapon.plan.load {
            LoadKind::Magazine if !weapon.plan.spent_round_replaced => true,
            _ => weapon.reading.out || weapon.reading.has_space(),
        }
    }

    pub(super) fn can_take_from_pouch(&self, weapon: &WeaponTick) -> bool {
        weapon.has_ammo()
            && self.can_take(weapon)
            && (!weapon.reading.unlimited || weapon.reading.out || weapon.reading.has_space())
    }

    /// Whether `weapon`'s gun can be reloaded from the belt now: anything short of a full, seated
    /// magazine.
    fn can_belt(&self, weapon: &WeaponTick) -> bool {
        let reading = &weapon.reading;
        (reading.out || reading.has_space()) && weapon.cylinder.is_none_or(|c| c.shut)
    }

    /// A round seated in `gun` of `weapon`, in `gun_hand`: the seat's thud (`cue`) on the gun hand,
    /// and the ready cue to follow once the gun can fire.
    pub(super) fn seated(
        &mut self,
        weapon: &WeaponTick,
        gun: &GunTick,
        gun_hand: Hand,
        cue: Event,
        reload: &Reload,
    ) {
        haptics::play(gun_hand, cue);
        self.cylinder_loaded(gun_hand);
        if ready_after_seat(
            &weapon.plan,
            self.records.get(weapon.key).chambers[gun.entry()],
        ) {
            self.ready_cue
                .arm(gun_hand, reload.ready_cue_delay_s, reload.ready_cue_pulses);
        }
    }

    /// Load `gun` of `weapon`: a whole magazine or one shell. Returns whether anything loaded.
    pub(super) fn load(&mut self, weapon: &WeaponTick, gun: &GunTick) -> bool {
        let record = self.records.get(weapon.key);
        // SAFETY (each engine call): game thread before the world update; the weapon was read
        // this tick.
        let loaded = match weapon.plan.load {
            LoadKind::Shell => unsafe { engine::load_one(weapon.address) },
            _ => unsafe { engine::load_full(weapon.address, weapon.replaces_spent()) },
        };
        if loaded && weapon.plan.action == ActionKind::None {
            record.chambers[gun.entry()] = Chamber::Chambered;
        }
        loaded
    }

    /// The grip pose of `hand` this tick and its velocity since the last.
    pub(super) fn grip_motion(&self, tick: &Tick, hand: Hand, dt: f32) -> Option<(Pose, Vec3)> {
        let pose = tick.hands[hand.index()]?.grip;
        let velocity = match self.last_grips[hand.index()] {
            Some(last) if dt > 1e-4 => (pose.position - last) / dt,
            _ => Vec3::ZERO,
        };
        Some((pose, velocity))
    }

    pub(super) fn event(&mut self, event: String) {
        tracing::info!(target: "controllers", "reload: {event}");
        self.status.events.push(event);
    }

    pub(super) fn clear_tick(&mut self) {
        guide::clear();
        self.tick = None;
        self.pouches = [None; 2];
        self.speedloaders = [None; 2];
        self.belt_rings = [None; 2];
        self.stocked_pouch = None;
        self.pouch_refusal = None;
        self.free = None;
        loose::let_go();
        self.status.insertion.reset();
        self.status.insert_hands = None;
        self.ready_cue.cancel();
        self.cancel_extraction();
        self.cancel_ejects();
    }

    pub(super) fn stand_down(&mut self) {
        ammo::suspend();
        loose::let_go();
        self.clear_tick();
        self.status.active = false;
        self.status.weapons = [None; 2];
        self.status.insertion.reset();
        self.ready_cue.cancel();
        self.status.insert_hands = None;
        self.status.work.reset();
        self.release_actions();
        set_pump_slide(None);
        self.status.fire_held = [None; 2];
        self.cylinders = [None; 2];
        self.belts = [BeltPush::new(); 2];
        self.last_grips = [None; 2];
        self.drawn = None;
        // A knocked magazine's part kept drawn in the well is hidden now, since no tick follows to
        // hide it, and its magazine is still out.
        if let Some(linger) = self.knocks.linger.take() {
            // SAFETY: the weapon handling stands down in the game stage, on the game thread before
            // the world update, and the part's lookup resolves the address only through the
            // inventory's slots and the carry set.
            unsafe { engine::show_magazine_part(linger.weapon, false) };
        }
    }
}

/// Whether the plan's chamber is the mod's to gate: the action is worked by hand after the gun runs
/// dry (a tactical reload skips it, and the belt reload works the action itself), and a bolt or a
/// pump after every shot, so the gate needs an action that can be worked, by a handle the table
/// places or on the foregrip.
pub(super) fn chambering(plan: &ReloadPlan) -> bool {
    match plan.action {
        ActionKind::None => false,
        ActionKind::Pump => true,
        _ => plan.handle.is_some(),
    }
}

/// Whether a gun whose plan is `plan` fires once its round seats, its chamber then being
/// `chamber`: one with no action worked by hand (a launcher, a revolver, or a gun whose handle the
/// table does not place), or one whose chamber kept its round through a tactical reload. A dry gun
/// with an action waits on it.
pub(super) fn ready_after_seat(plan: &ReloadPlan, chamber: Chamber) -> bool {
    !chambering(plan) || chamber != Chamber::Empty
}

/// Log a hand's gun's magazine going out or in, or its count or reserve changing outside a shot,
/// between the last tick's readout and this one's: a change no reload step logged is the game's.
fn log_magazine_change(hand: Hand, before: Option<WeaponReadout>, after: Option<WeaponReadout>) {
    let (Some(before), Some(after)) = (before, after) else {
        return;
    };
    if before.address != after.address
        || (before.out == after.out
            && before.reserve == after.reserve
            && after.magazine <= before.magazine)
    {
        return;
    }
    tracing::info!(
        target: "controllers",
        hand = ?hand,
        weapon = format_args!("{:#010x}", after.name_hash),
        magazine = after.magazine,
        was = before.magazine,
        out = after.out,
        was_out = before.out,
        reserve = after.reserve,
        was_reserve = before.reserve,
        "reload: the gun's magazine changed"
    );
}

/// Play the magazine-in sound where a round seated in gun `gun` of the weapon at `weapon`.
///
/// # Safety
///
/// As [`engine::play_seat_sound`].
pub(super) unsafe fn seat_sound(weapon: usize, gun: usize) {
    let sound = unsafe { engine::play_seat_sound(weapon, gun) };
    tracing::debug!(target: "controllers", ?sound, "reload: played the seat's sound");
}

/// Play the dry-fire click where a well refused the round pushed at it, on gun `gun` of the weapon
/// at `weapon`.
///
/// # Safety
///
/// As [`engine::play_refuse_sound`].
pub(super) unsafe fn refuse_sound(weapon: usize, gun: usize) {
    let sound = unsafe { engine::play_refuse_sound(weapon, gun) };
    tracing::debug!(target: "controllers", ?sound, "reload: played the refusal's sound");
}

/// Play `sound` at gun `gun` of the weapon at `weapon`, for a move of the hands the game's clips
/// would have voiced.
///
/// # Safety
///
/// As [`engine::play_sound`].
pub(super) unsafe fn gun_sound(weapon: usize, gun: usize, sound: GunSound) {
    let usage = unsafe { engine::play_sound(weapon, gun, sound) };
    tracing::debug!(
        target: "controllers",
        ?sound,
        played = usage.is_some(),
        "reload: played the gun's sound"
    );
}

/// The point of `weapon`'s gun pushed onto the belt pouch, its well at `well`: the base of a
/// magazine still in, its visible length below the mouth from the wells table, or the mouth.
fn belt_point(weapon: &WeaponTick, well: Well, world_scale: f32) -> Vec3 {
    let below = if weapon.occupied() && world_scale.is_finite() && world_scale > 0.0 {
        knock::below_m(wells::lookup(weapon.reading.name_hash)) / world_scale
    } else {
        0.0
    };
    belt::push_point(well.mouth, well.axis, below)
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
    config: &WeaponSettings,
) -> Option<WeaponTick> {
    let reload = &Reload::of(&config.weapons.reload);
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
    )?
    .cocked();
    Some(WeaponTick {
        address: weapon.address,
        ammo_owned: false,
        key: RecordKey::new(item_of(weapon.address), weapon.address),
        plan,
        cylinder: None,
        from_table: entry.is_some(),
        reading,
        reserve: unsafe { reserve(base) },
    })
}

#[cfg(any(test, feature = "test-support"))]
pub(super) use test_support::eligible_belt_for_test;

#[cfg(test)]
mod action_tests;
#[cfg(test)]
mod authority_tests;
#[cfg(test)]
mod belt_tests;
#[cfg(test)]
mod insertion_tests;
#[cfg(any(test, feature = "test-support"))]
mod test_support;
