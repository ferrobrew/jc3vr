//! What the snatch does with the target it fired at: frees a gun from a character, fires the hook,
//! opens a door the hook took hold of, pulls a weapon or an object in along an arc and hands it to
//! the hand on arrival, or swings it on the held line; and the toss a tap of the grapple gives a
//! gun.

use std::num::NonZeroUsize;

use glam::{Mat4, Vec3};
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::WeaponBase,
};

use crate::{
    controllers::{
        aim::{
            GRAPPLE_ORIGIN_M,
            ray::Ray,
            raycast::{self, CastOptions},
        },
        frame::Hand,
        grab::latch::GripLatch,
        hands::ik,
        haptics::{self, Sustain},
        physical_hands::{
            self,
            doors::{DoorTarget, engine::vehicle_frame},
            grasp::GripTarget as HoldPoint,
            hold::{self, HandHold, HoldTuning},
            remote,
        },
        snatch::{
            FinishInput, Refusal, SNATCH,
            arrival::{self, Landing, LandingInput},
            brace::{self, Brace, BraceInput, BraceReach},
            engine, juice, landing,
            rules::{self, FlightLimits, FlightSample, PropHandling, StallWatch, Verdict},
            sling::{LineTuning, Sling, SlingStep},
            sling_hooks,
            target::{Offer, OfferKind, weak_of},
        },
        weapons::{
            grenade,
            hold::{
                Cause, SNAP_S,
                pickup::{self, TakeOptions},
                rack,
                refs::{Strong, Weak},
                steal,
            },
            inventory::engine::weapon_of,
        },
    },
    grapple::rope::{self, GRAVITY_MPS2, RopeFault},
};

/// What the snatch fired at, held alive from the fire to the end.
pub(super) struct Grabbed {
    /// The physics game object the hook goes to.
    pub object: NonZeroUsize,
    pub kind: Pulled,
    _alive: Strong,
}

/// How a grabbed thing comes.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) enum Pulled {
    /// A weapon lying in the world, into the hand through the table.
    Weapon,
    /// A loose object, body `body` of `mass_kg`: one a hand can carry into the hand's ordinary
    /// hold, or one too heavy, swung on the held line.
    Prop {
        body: u32,
        mass_kg: f32,
        handling: PropHandling,
    },
    /// A ragdolling character's ragdoll, pulled by its body `body` of `owner`, which drags the
    /// whole ragdoll of `mass_kg`: always swung on the held line.
    Ragdoll {
        owner: NonZeroUsize,
        body: u32,
        mass_kg: f32,
    },
    /// An inert grenade, body `body`, into the hand through the loadout.
    Grenade { body: u32 },
    /// A finite native hand grenade lying in the world, body `body`: the catch takes it into the
    /// hand as the grenade it is, as a hand closing on it does.
    WorldGrenade { body: u32 },
    /// A vehicle's closed door, pulled open at its handle, `handle` in the vehicle's frame.
    Door { door: DoorTarget, handle: Vec3 },
}

impl Pulled {
    /// Whether the target is too heavy to come to the hand, so the line swings it from the bite.
    pub(super) const fn tethered(&self) -> bool {
        matches!(
            self,
            Pulled::Prop {
                handling: PropHandling::Swing,
                ..
            } | Pulled::Ragdoll { .. }
        )
    }
}

/// What a fire at an armed target started.
pub(super) enum Fired {
    /// A character let go of its gun; the hook fires once the gun lies in the world.
    Freeing(Grabbed),
    /// The hook flies.
    HookOut(Grabbed, NonZeroUsize),
}

/// Fire at the armed target: free a gun from a character's hands or holster first, or fire the
/// hook at a weapon, an object, or a door.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn fire_at(hand: Hand, offer: Offer) -> Result<Fired, Refusal> {
    let freed = match offer.kind {
        OfferKind::Ground { weapon, weak, .. } => {
            let grabbed = Grabbed {
                object: weapon,
                kind: Pulled::Weapon,
                _alive: weak.lock().ok_or(Refusal::Gone)?,
            };
            let (dropped, _) = unsafe { engine::dropped(weapon) }?;
            // What the hand could not take is refused before the hook flies.
            let player = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
                .ok_or(Refusal::NoPlayer)?;
            unsafe { pickup::takeable(player, dropped) }.map_err(Refusal::Take)?;
            unsafe { pickup::route(dropped, Some(hand)) }.map_err(Refusal::Take)?;
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::Prop {
            object,
            body,
            mass_kg,
            handling,
            weak,
        } => {
            let grabbed = Grabbed {
                object,
                kind: Pulled::Prop {
                    body,
                    mass_kg,
                    handling,
                },
                _alive: weak.lock().ok_or(Refusal::Gone)?,
            };
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::Ragdoll {
            character,
            owner,
            body,
            mass_kg,
            weak,
        } => {
            let grabbed = Grabbed {
                object: character,
                kind: Pulled::Ragdoll {
                    owner,
                    body,
                    mass_kg,
                },
                _alive: weak.lock().ok_or(Refusal::Gone)?,
            };
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::WorldGrenade { weapon, body, weak } => {
            let grabbed = Grabbed {
                object: weapon,
                kind: Pulled::WorldGrenade { body },
                _alive: weak.lock().ok_or(Refusal::Gone)?,
            };
            unsafe { engine::dropped(weapon) }?;
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::Grenade { object, body, weak } => {
            let grabbed = Grabbed {
                object,
                kind: Pulled::Grenade { body },
                _alive: weak.lock().ok_or(Refusal::Gone)?,
            };
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::Door { door, handle, weak } => {
            let alive = weak.lock().ok_or(Refusal::Gone)?;
            let local = unsafe { vehicle_frame(door.vehicle) }
                .inverse()
                .transform_point3(handle);
            let grabbed = Grabbed {
                object: door.vehicle,
                kind: Pulled::Door {
                    door,
                    handle: local,
                },
                _alive: alive,
            };
            let wire = unsafe { fire_hook(&grabbed) }?;
            return Ok(Fired::HookOut(grabbed, wire));
        }
        OfferKind::Held(target) => {
            unsafe { steal::release(hand, &target) }.map_err(Refusal::Free)?
        }
        OfferKind::Holstered(target) => {
            unsafe { steal::release_holstered(hand, &target) }.map_err(Refusal::Free)?
        }
        OfferKind::Rack { weapon, weak } => {
            unsafe { rack::remote::release(hand, weapon, &weak) }.map_err(Refusal::Rack)?
        }
    };
    let object = NonZeroUsize::new(freed.weapon as usize).ok_or(Refusal::Gone)?;
    tracing::info!(target: "controllers", hand = hand.label(), "snatch: the gun is free of its holder");
    Ok(Fired::Freeing(Grabbed {
        object,
        kind: Pulled::Weapon,
        _alive: freed.alive,
    }))
}

/// Fire the hook at what `grabbed` holds.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn fire_hook(grabbed: &Grabbed) -> Result<NonZeroUsize, Refusal> {
    match grabbed.kind {
        Pulled::Weapon => {
            let (_, body) = unsafe { engine::dropped(grabbed.object) }?;
            let at = unsafe { body_centre(body, grabbed.object) }?;
            unsafe { engine::fire(grabbed.object, Vec3::ZERO, at) }
        }
        Pulled::Prop { body, .. } | Pulled::Grenade { body } | Pulled::WorldGrenade { body } => {
            let at = unsafe { body_centre(body, grabbed.object) }?;
            unsafe { engine::fire(grabbed.object, Vec3::ZERO, at) }
        }
        Pulled::Ragdoll { owner, body, .. } => {
            // The hook latches the character's bone nearest the point, given in the character's
            // model space.
            let at = unsafe { body_centre(body, owner) }?;
            let local = unsafe { character_frame(grabbed.object) }
                .inverse()
                .transform_point3(at);
            unsafe { engine::fire(grabbed.object, local, at) }
        }
        Pulled::Door { door, handle } => {
            let at = unsafe { vehicle_frame(door.vehicle) }.transform_point3(handle);
            unsafe { engine::fire(grabbed.object, handle, at) }
        }
    }
}

/// What the hook taking hold of a grabbed thing does.
pub(super) enum Attached {
    /// The pull starts.
    Pull,
    /// The door came open, and the line's work is done.
    Opened,
}

/// The hook took hold of `grabbed`: a door comes open and swings outward; anything else is pulled.
///
/// # Safety
///
/// The game thread, before the world update and the physics step.
pub(super) unsafe fn attached(grabbed: &Grabbed, swing_mps: f32) -> Result<Attached, Refusal> {
    let Pulled::Door { door, handle } = grabbed.kind else {
        return Ok(Attached::Pull);
    };
    let at = unsafe { vehicle_frame(door.vehicle) }.transform_point3(handle);
    unsafe { remote::swing_open(&door, at, swing_mps) }.map_err(Refusal::Door)?;
    Ok(Attached::Opened)
}

/// How a pulled thing arrived.
pub(super) enum Arrival {
    /// The weapon (a `WeaponBase` address) went into the hand, to this slot.
    Weapon { slot: WeaponSlot, weapon: usize },
    /// The object went into the hand's hold.
    Prop,
    /// The grenade came to the hand, which takes it at the next input tick.
    Grenade,
}

/// The target's flight on the line, from the hook's bite to the catch.
pub(super) struct Flight {
    /// Where the hook bit the target, in the target body's frame: the line pulls there, so a
    /// target hooked off its centre spins as it comes.
    hook_local: Vec3,
    /// The target's centre at the last tick, for the swept catch: the gun's grip for a gun the
    /// landing steers.
    last_centre: Option<Vec3>,
    /// A gun's aligned landing, when the target is a gun whose pose could be read at the bite.
    landing: Option<Landing>,
    /// How long the line has been slack, seconds, or `None` while it pulls.
    slack_s: Option<f32>,
    stall: StallWatch,
}

impl Flight {
    /// Whether the line has gone slack near the hand.
    pub(super) const fn slack(&self) -> bool {
        self.slack_s.is_some()
    }

    /// Start the flight's watch over, for a pull that resumes after the sling held the target.
    pub(super) fn resume(&mut self) {
        self.last_centre = None;
        self.slack_s = None;
        self.stall = StallWatch::default();
    }
}

/// What a tick of the flight did.
pub(super) enum Step {
    /// The target is on its way.
    Flying,
    /// The line went slack this tick; the target flies on by its own momentum.
    WentSlack,
    /// This hand caught the target.
    Caught(Hand, Arrival),
}

/// The hook bit `grabbed`: note where, take the native rope off the line so that the line's pull
/// is the one force on the target, and play the hook's impact.
///
/// # Safety
///
/// The game thread, before the world update and the physics step.
pub(super) unsafe fn bite(
    hand: Hand,
    grabbed: &Grabbed,
    wire: NonZeroUsize,
) -> Result<Flight, Refusal> {
    let (body, owner) = unsafe { target_body(grabbed) }?;
    let state = unsafe { physical_hands::body_state(body, owner) }.map_err(Refusal::Body)?;
    let hook_local = unsafe { engine::hook_end(wire) }
        .map(|end| Vec3::from_array(end.m_WorldPositionT1.data))
        .filter(|at| at.is_finite())
        .map_or(Vec3::ZERO, |at| state.pose.local(at));
    unsafe { engine::exclude_rope(wire) };
    if let Some(palm) = ik::tick_records().current[hand.index()].map(|t| t.wrist.position) {
        unsafe { juice::impact(wire, grabbed.object, palm) };
    }
    let landing = match grabbed.kind {
        Pulled::Weapon => unsafe { engine::dropped(grabbed.object) }
            .ok()
            .and_then(|(weapon, _)| unsafe { Landing::measure(weapon, state.pose) }),
        _ => None,
    };
    Ok(Flight {
        hook_local,
        last_centre: None,
        landing,
        slack_s: None,
        stall: StallWatch::default(),
    })
}

/// One tick of the flight: the line yanks the target toward the line's hand `hand`, slowing over
/// the final approach, then goes slack a couple of ticks out, and a hand of `catchers` (by
/// [`Hand::index`], the hands free with their grips held) catches the target if the target passes
/// through it.
///
/// The line's pull goes through the hold solve, the physics every hand's hold on a body goes
/// through, with the line's strength and no twist of its own: the target keeps its momentum and its
/// spin, collides on the way, and is never placed. Beyond the final approach the line pulls at the
/// hook's point, so a target hooked off its centre spins; within it the line pulls at the centre,
/// so the centre's own velocity, which the target keeps once the line is slack, is the line's.
///
/// # Safety
///
/// The game thread, before the world update and the physics step.
pub(super) unsafe fn pull(
    hand: Hand,
    grabbed: &Grabbed,
    wire: NonZeroUsize,
    flight: &mut Flight,
    catchers: [bool; 2],
    input: &FinishInput,
) -> Result<Step, Refusal> {
    let (reading, line_m) = unsafe { engine::read_wire(wire) };
    if reading == engine::WireReading::Gone && !flight.slack() {
        return Err(Refusal::LineLost);
    }
    let (body, owner) = unsafe { target_body(grabbed) }?;
    let state = unsafe { physical_hands::body_state(body, owner) }.map_err(Refusal::Body)?;
    let palms = ik::tick_records()
        .current
        .map(|t| t.map(|t| t.wrist.position));
    let Some(palm) = palms[hand.index()] else {
        // An untracked hand keeps the line where it is.
        return Ok(Step::Flying);
    };
    let centre = state.pose.centre;
    let stalled = flight.stall.step(
        input.dt,
        centre,
        SNATCH.stall_window_s,
        SNATCH.stall_distance_m,
    );
    if let Some(slack_s) = &mut flight.slack_s {
        *slack_s += input.dt;
    }
    // A gun the landing steers is caught at its grip, where the catching hand will hold it.
    let landed = flight.landing.as_mut().map(|landing| unsafe {
        landing.tick(&LandingInput {
            body: state.pose,
            velocity: state.velocities.linear,
            angular: state.velocities.angular,
            hand,
            palm,
            catchers,
            dt: input.dt,
        })
    });
    let at = landed.map_or(centre, |l| l.grip);
    let sample = FlightSample {
        from: flight.last_centre.unwrap_or(at),
        to: at,
        velocity: state.velocities.linear,
        hand: palm,
        catchers: Hand::BOTH.map(|h| {
            let target = landed
                .and_then(|l| l.targets[h.index()])
                .map(|t| t.position);
            target.or(palms[h.index()]).filter(|_| catchers[h.index()])
        }),
        slack_s: flight.slack_s,
    };
    flight.last_centre = Some(at);
    let limits = FlightLimits {
        slack_radius_m: rules::slack_radius(
            state.velocities.linear.length(),
            SNATCH.coast_s,
            SNATCH.slack_min_m,
            SNATCH.slack_max_m,
        ),
        catch_radius_m: SNATCH.catch_radius_m,
        line_m: line_m.unwrap_or(SNATCH.range_m),
        max_slack_s: SNATCH.max_slack_s,
    };
    // The turn keeps running once the line is slack: it cannot change whether the catch happens.
    let drive = landed.and_then(|l| l.angular);
    match rules::verdict(&sample, stalled, &limits) {
        Verdict::Pulling => {}
        Verdict::Slack => {
            if let Some(angular) = drive {
                unsafe {
                    physical_hands::set_body_velocities(
                        body,
                        owner,
                        state.velocities.linear,
                        angular,
                    )
                }
                .map_err(Refusal::Body)?;
            }
            if flight.slack() {
                return Ok(Step::Flying);
            }
            flight.slack_s = Some(0.0);
            return Ok(Step::WentSlack);
        }
        Verdict::Caught(catcher) => {
            unsafe { end_line(wire) };
            let catcher_palm = palms[catcher.index()].unwrap_or(palm);
            let slide_s = match (&flight.landing, landed) {
                (Some(landing), Some(l)) => l.targets[catcher.index()].map_or(SNAP_S, |target| {
                    let rotation = arrival::gun_rotation(landing, state.pose);
                    let (angle, distance) = arrival::residual(rotation, l.grip, target);
                    let speed = state.velocities.linear.length();
                    let slide_s =
                        landing::arrival_slide(angle, distance, speed, SNAP_S, &SNATCH.landing);
                    tracing::info!(
                        target: "controllers",
                        hand = catcher.label(),
                        angle_deg = angle.to_degrees(),
                        distance_m = distance,
                        speed_mps = speed,
                        slide_s,
                        "snatch: the gun arrived {:.0} degrees and {:.2} m from where the {} hand holds it",
                        angle.to_degrees(),
                        distance,
                        catcher.label()
                    );
                    slide_s
                }),
                _ => SNAP_S,
            };
            return unsafe { arrive(catcher, grabbed, body, centre, catcher_palm, slide_s) }
                .map(|arrival| Step::Caught(catcher, arrival));
        }
        Verdict::Missed => return Err(Refusal::CatchMissed),
        Verdict::Stalled => return Err(Refusal::Stalled),
        Verdict::OutOfLine => return Err(Refusal::OutOfLine),
    }
    let mass = state.mass_kg.unwrap_or(1.0);
    let tuning = HoldTuning {
        stiffness_per_s: 0.0,
        angular_stiffness_per_s: 0.0,
        max_speed_mps: SNATCH.line_speed_mps.max(0.0),
        max_angular_speed_rps: SNATCH.max_spin_rps.max(0.0),
        hand_force_n: mass * SNATCH.line_accel_mps2.max(0.0),
        hand_torque_nm: 0.0,
        two_hand_angular_share: 1.0,
        gravity: Vec3::new(0.0, -GRAVITY_MPS2, 0.0),
    };
    let rigid = state.rigid();
    let distance = centre.distance(palm);
    let anchor = if distance > SNATCH.approach_m {
        state.pose.point(flight.hook_local)
    } else {
        centre
    };
    let speed = rules::line_speed(
        distance,
        SNATCH.line_speed_mps,
        SNATCH.arrival_speed_mps,
        SNATCH.approach_m,
    );
    let shift = landed.map_or(Vec3::ZERO, |l| l.shift);
    let aim =
        rules::pull_point(centre, palm + shift, SNATCH.arc, SNATCH.approach_m) + (anchor - centre);
    let mut holds = [None; 2];
    holds[hand.index()] = Some(HandHold {
        anchor,
        target: HoldPoint {
            point: anchor,
            orientation: state.pose.orientation,
        },
        velocity: rules::line_velocity(anchor, aim, speed),
        angular_velocity: rigid.velocities.angular,
    });
    let solution = hold::solve(&rigid, holds, input.dt, &tuning);
    unsafe {
        physical_hands::set_body_velocities(
            body,
            owner,
            solution.velocities.linear,
            drive.unwrap_or(solution.velocities.angular),
        )
    }
    .map_err(Refusal::Body)?;
    haptics::sustain(hand, Sustain::GrappleTension, solution.loads[hand.index()]);
    Ok(Step::Flying)
}

/// End the snatch's line `wire` with the line's retract, while it is still the hook's active wire.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn end_line(wire: NonZeroUsize) {
    if unsafe { engine::read_wire(wire) }.0 != engine::WireReading::Gone {
        unsafe { juice::retract() };
    }
    unsafe { engine::cancel(wire) };
}

/// The body of the thing `grabbed` holds, for the line to pull, and the object that owns it.
///
/// # Safety
///
/// The game thread, before the world update.
unsafe fn target_body(grabbed: &Grabbed) -> Result<(u32, NonZeroUsize), Refusal> {
    match grabbed.kind {
        Pulled::Weapon => Ok((
            unsafe { engine::dropped(grabbed.object) }?.1,
            grabbed.object,
        )),
        Pulled::Prop { body, .. } | Pulled::Grenade { body } | Pulled::WorldGrenade { body } => {
            Ok((body, grabbed.object))
        }
        Pulled::Ragdoll { owner, body, .. } => Ok((body, owner)),
        Pulled::Door { .. } => Err(Refusal::Gone),
    }
}

/// The native device endpoint follows the tracked wrist frame, never the aim ray. Gives the line's
/// origin on the grapple hand and the hand's wrist.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn prepare_sling(
    hand: Hand,
    wire: NonZeroUsize,
) -> Result<(Vec3, Vec3), Refusal> {
    let wrist = ik::tick_records().current[hand.index()]
        .ok_or(Refusal::Gone)?
        .wrist;
    let anchor = wrist.position + wrist.orientation * GRAPPLE_ORIGIN_M;
    let transform = Mat4::from_rotation_translation(wrist.orientation, anchor);
    if !transform.is_finite() {
        return Err(Refusal::Gone);
    }
    let proxy = unsafe { engine::device_proxy(wire) }.ok_or(Refusal::LineLost)?;
    unsafe { engine::remove_constraint(wire) };
    sling_hooks::publish(wire.get(), proxy, transform);
    Ok((anchor, wrist.position))
}

/// A tick of the held line, for the readout, the haptics, and the overlay.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Swinging {
    /// The line's held length, metres.
    pub length_m: f32,
    /// The line's tension, and the most it holds on this target with this brace, newtons.
    pub tension_n: f32,
    pub max_tension_n: f32,
    pub mass_kg: f32,
    /// Whether the other hand braces the grapple hand.
    pub braced: bool,
    /// Where the line is anchored, and the target's centre, world space.
    pub anchor: Vec3,
    pub centre: Vec3,
}

impl Swinging {
    /// The line's tension as a share of its strength, `0..=1`.
    pub(super) fn share(&self) -> f32 {
        rope::tension_share(self.tension_n, self.max_tension_n)
    }
}

/// One tick of the held line: hold the target at the line's held length from the grapple hand,
/// reeling in toward the swing's length, with a tension up to the line's strength for the target's
/// mass, raised while the other hand braces the grapple hand's wrist ([`brace`]).
///
/// # Safety
///
/// The game thread, before the world update and physics step.
pub(super) unsafe fn sling(
    hand: Hand,
    grabbed: &Grabbed,
    wire: NonZeroUsize,
    rope: &mut Option<Sling>,
    brace: &mut Brace,
    input: &FinishInput,
) -> Result<Swinging, Refusal> {
    if unsafe { engine::read_wire(wire) }.0 != engine::WireReading::Attached {
        return Err(Refusal::LineLost);
    }
    let (body, owner) = unsafe { target_body(grabbed) }?;
    let state = unsafe { physical_hands::body_state(body, owner) }.map_err(Refusal::Body)?;
    let (line_anchor, wrist) = unsafe { prepare_sling(hand, wire) }?;
    let other = hand.other();
    let other_wrist = ik::tick_records().current[other.index()].map(|t| t.wrist.position);
    let was_braced = brace.braced();
    let braced = brace.step(
        BraceInput {
            grip_held: input.grips[other.index()].held()
                && matches!(input.latches[other.index()], GripLatch::Fist { .. }),
            free: physical_hands::free_hands()[other.index()]
                && physical_hands::held_props()[other.index()].is_none(),
            gap_m: other_wrist.map(|other| other.distance(wrist)),
        },
        BraceReach {
            take_m: SNATCH.brace_take_m,
            break_m: SNATCH.brace_break_m,
        },
    );
    let braced_anchor = brace::anchor(line_anchor, wrist, other_wrist);
    let anchor = if braced { braced_anchor } else { line_anchor };
    let rope = match rope {
        Some(rope) => {
            // The anchor moving between the hand and the pair is no motion of the hands.
            if braced != was_braced && other_wrist.is_some() {
                rope.shift_anchor(if braced {
                    braced_anchor - line_anchor
                } else {
                    line_anchor - braced_anchor
                });
            }
            rope
        }
        slot @ None => slot.insert(Sling::new(state.pose.centre, anchor).ok_or(Refusal::Gone)?),
    };
    // A ragdoll's body drags the whole ragdoll, so the line holds and pulls the whole ragdoll's
    // weight.
    let ragdoll = matches!(grabbed.kind, Pulled::Ragdoll { .. });
    let mass_kg = match grabbed.kind {
        Pulled::Ragdoll { mass_kg, .. } => mass_kg,
        _ => state.mass_kg.unwrap_or(1.0),
    };
    let line = LineTuning::of(&input.config.weapons.snatch.swing, mass_kg, braced);
    let rigid = state.rigid();
    let out = rope
        .step(SlingStep {
            centre: state.pose.centre,
            velocity: rigid.velocities.linear,
            mass_kg,
            anchor,
            dt: input.dt,
            line,
        })
        .map_err(|fault| match fault {
            RopeFault::Invalid => Refusal::Gone,
            RopeFault::Stretched => Refusal::OutOfLine,
        })?;
    if ragdoll {
        // The line's impulse on the one body, which the ragdoll's constraints share with the rest.
        let impulse = out.impulse_on_b(input.dt);
        unsafe { remote::push(body, owner, impulse, state.pose.centre) }
    } else {
        unsafe {
            physical_hands::set_body_velocities(
                body,
                owner,
                out.b_velocity,
                rigid.velocities.angular,
            )
        }
    }
    .map_err(Refusal::Body)?;
    let swinging = Swinging {
        length_m: rope.length_m(),
        tension_n: out.tension_n,
        max_tension_n: line.max_tension_n,
        mass_kg,
        braced,
        anchor,
        centre: state.pose.centre,
    };
    haptics::sustain(hand, Sustain::GrappleTension, swinging.share());
    if braced {
        haptics::sustain(other, Sustain::GrappleTension, swinging.share());
    } else if was_braced {
        haptics::sustain(other, Sustain::GrappleTension, 0.0);
    }
    Ok(swinging)
}

/// Where a tossed gun lands, metres in front of the player toward where it came from.
const TOSS_LAND_M: f32 = 1.2;
/// The toss's speed over the ground, metres per second, and its shortest and longest flight,
/// seconds.
const TOSS_SPEED_MPS: f32 = 9.0;
const TOSS_FLIGHT_S: (f32, f32) = (0.45, 1.2);

/// Hand what the hand caught over: a weapon into the hand through the table, sliding the rest of
/// the way into its grip over `slide_s`, an object into the hand's hold at its surface nearest the
/// palm, and a grenade into the hand through the loadout.
///
/// # Safety
///
/// The game thread, before the world update and the physics step.
unsafe fn arrive(
    hand: Hand,
    grabbed: &Grabbed,
    body: u32,
    centre: Vec3,
    palm: Vec3,
    slide_s: f32,
) -> Result<Arrival, Refusal> {
    match grabbed.kind {
        Pulled::Weapon => {
            let (weapon, _) = unsafe { engine::dropped(grabbed.object) }?;
            let address = weapon as *mut WeaponBase as usize;
            let options = TakeOptions {
                slide_s,
                cause: Cause::Snatch,
            };
            unsafe { pickup::take(hand, weapon, options) }
                .map(|slot| Arrival::Weapon {
                    slot,
                    weapon: address,
                })
                .map_err(Refusal::Take)
        }
        Pulled::Prop {
            handling: PropHandling::Swing,
            ..
        }
        | Pulled::Ragdoll { .. } => Err(Refusal::Gone),
        Pulled::Prop { mass_kg, .. } => {
            let point = unsafe { surface_toward(grabbed.object, centre, palm) };
            remote::hold_remote(hand, grabbed.object, body, point, mass_kg);
            Ok(Arrival::Prop)
        }
        Pulled::Grenade { body } => {
            if grenade::snatched(hand, grabbed.object, body) {
                Ok(Arrival::Grenade)
            } else {
                Err(Refusal::Gone)
            }
        }
        Pulled::WorldGrenade { .. } => {
            // SAFETY: the snatch's strong reference keeps the weapon alive.
            grenade::snatched_from_world(hand, unsafe { weak_of(grabbed.object) });
            Ok(Arrival::Grenade)
        }
        Pulled::Door { .. } => Err(Refusal::Gone),
    }
}

/// The point on `object`'s surface on the line from `palm` to its centre `centre`, or the centre
/// when the line does not find it.
///
/// # Safety
///
/// The game thread, before the world update.
unsafe fn surface_toward(object: NonZeroUsize, centre: Vec3, palm: Vec3) -> Vec3 {
    let options = unsafe { CastOptions::ignoring_player(0) };
    let distance = palm.distance(centre);
    Ray::new(palm, centre - palm)
        .and_then(|ray| {
            unsafe { raycast::cast(&ray, distance, &options) }
                .ok()
                .flatten()
        })
        .filter(|hit| hit.object == Some(object))
        .map_or(centre, |hit| hit.position)
}

/// The world transform of the character `character`.
///
/// # Safety
///
/// `character` must be a live character, held alive.
unsafe fn character_frame(character: NonZeroUsize) -> Mat4 {
    let character = unsafe { &*(character.get() as *const Character) };
    Mat4::from(character.m_WorldMatrixT1)
}

/// The centre of mass of `object`'s body `body`, world space.
///
/// # Safety
///
/// The game thread, before the world update.
unsafe fn body_centre(body: u32, object: NonZeroUsize) -> Result<Vec3, Refusal> {
    unsafe { physical_hands::body_state(body, object) }
        .map(|state| state.pose.centre)
        .map_err(Refusal::Body)
}

/// Toss the weapon a tap of the grapple reeled in to land in front of the player.
///
/// # Safety
///
/// The game thread, before the world update and the physics step.
pub(super) unsafe fn toss(weak: &Weak) -> Result<(), Refusal> {
    let alive = weak.lock().ok_or(Refusal::Gone)?;
    let weapon = NonZeroUsize::new(weapon_of(&alive) as usize).ok_or(Refusal::Gone)?;
    let (_, body) = unsafe { engine::dropped(weapon) }?;
    let from = unsafe { body_centre(body, weapon) }?;
    let player = unsafe { engine::player_position() }.ok_or(Refusal::NoPlayer)?;
    let toward = Vec3::new(from.x - player.x, 0.0, from.z - player.z)
        .try_normalize()
        .unwrap_or(Vec3::ZERO);
    let land = player + toward * TOSS_LAND_M;
    let flight = rules::toss_flight(
        from.distance(land),
        TOSS_SPEED_MPS,
        TOSS_FLIGHT_S.0,
        TOSS_FLIGHT_S.1,
    );
    let velocity = rules::toss_velocity(from, land, flight, Vec3::new(0.0, -GRAVITY_MPS2, 0.0));
    unsafe { physical_hands::set_body_velocities(body, weapon, velocity, Vec3::ZERO) }
        .map_err(Refusal::Body)
}
