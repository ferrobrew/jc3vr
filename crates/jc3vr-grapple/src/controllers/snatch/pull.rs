//! What the snatch does with the target it fired at: frees a gun from a character, fires the hook,
//! opens a door the hook took hold of, pulls a weapon or an object in along an arc and hands it to
//! the hand on arrival, or swings it on the held line; and the toss a tap of the grapple gives a
//! gun.

use std::num::NonZeroUsize;

use glam::{Mat4, Vec3};
use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponBase};
use jc3vr_body::controllers::{
    aim::{grapple_origin_m, ray::Ray, raycast, raycast::CastOptions},
    grab::latch::GripLatch,
    hands::ik,
    haptics, physics,
    physics::{GRAVITY_MPS2, grapple_hook::BodyHit, refs::Weak, rigid::Velocities},
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::{frame::Hand, haptics::cues::Sustain};
use jc3vr_physical::controllers::physical_hands::{
    doors::engine::vehicle_frame,
    grasp::GripTarget as HoldPoint,
    hold,
    hold::{HandHold, HoldTuning},
    remote,
};
use jc3vr_view::hooks::camera;
use jc3vr_weapons::controllers::weapons::{
    grenade,
    hold::{SNAP_S, source as hold_source},
    inventory::engine::weapon_of,
};

pub(super) use crate::controllers::snatch::acquisition::{
    Fired, Freeing, Grabbed, Pulled, collect, fire_at,
};
use crate::{
    controllers::snatch::{
        FinishInput, Refusal, SNATCH, arrival,
        arrival::{Landing, LandingInput},
        brace,
        brace::{Brace, BraceInput, BraceReach, LineInput, Regrip},
        catch,
        catch::{CatchHand, CatchTuning, CatchWatch},
        engine, juice, landing, rules,
        rules::{FlightLimits, FlightSample, PropHandling, StallWatch, Verdict},
        sling::{LineTuning, Sling, SlingStep},
        sling_hooks,
        target::weak_of,
    },
    grapple::{line_grab, rope, rope::RopeFault},
};

/// Fire the hook at what `grabbed` holds.
///
/// # Safety
///
/// The game thread, before the world update.
pub(super) unsafe fn fire_hook(grabbed: &Grabbed) -> Result<NonZeroUsize, Refusal> {
    if !grabbed.live() {
        return Err(Refusal::Gone);
    }
    if matches!(
        grabbed.kind,
        Pulled::Weapon { .. } | Pulled::WorldGrenade { .. }
    ) {
        unsafe { engine::dropped(grabbed.object) }?;
    }
    if matches!(grabbed.kind, Pulled::Prop { .. }) {
        let (body, owner) = unsafe { target_body(grabbed) }?;
        unsafe { remote::loosen(owner, body) }.map_err(Refusal::Body)?;
    }
    if let Some(hit) = grabbed.hit.get() {
        let (wire, hit) = unsafe { engine::fire(grabbed.object, hit) }?;
        grabbed.hit.set(Some(hit));
        return Ok(wire);
    }
    let (body, owner) = match grabbed.kind {
        Pulled::Door { door, handle } => {
            let at = unsafe { vehicle_frame(door.vehicle) }.transform_point3(handle);
            return unsafe { engine::fire_object(grabbed.object, at) };
        }
        _ => unsafe { target_body(grabbed) }?,
    };
    let local = grabbed.object_local.ok_or(Refusal::Gone)?;
    let at = unsafe { engine::object_frame(grabbed.object) }
        .ok_or(Refusal::Gone)?
        .transform_point3(local);
    let hit = unsafe { BodyHit::at(body, owner, at) }.ok_or(Refusal::Gone)?;
    grabbed.hit.set(Some(hit));
    let (wire, hit) = unsafe { engine::fire(grabbed.object, hit) }?;
    grabbed.hit.set(Some(hit));
    Ok(wire)
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
    if !grabbed.live() {
        return Err(Refusal::Gone);
    }
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
    /// The physical catch queued the placed-charge swap at the next weapon update.
    Charge,
}

/// The target's flight on the line, from the hook's bite to the catch.
pub(super) struct Flight {
    /// Where the hook bit the target, in the target body's frame: the line pulls there, so a
    /// target hooked off its centre spins as it comes.
    hit: BodyHit,
    /// Where the hook bit, world space, kept for the fallback to the object's own body when the
    /// selected body no longer belongs to it.
    bite_world: Vec3,
    /// The target's centre at the last tick, for the swept catch: the gun's grip for a gun the
    /// landing steers.
    last_centre: Option<Vec3>,
    /// A gun's aligned landing, when the target is a gun whose pose could be read at the bite.
    landing: Option<Landing>,
    /// How long the line has been slack, seconds, or `None` while it pulls.
    slack_s: Option<f32>,
    stall: StallWatch,
    /// The hand the line steers toward, kept across ticks for the hysteresis.
    receiver: Option<Hand>,
    catch: CatchWatch,
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
        self.catch = CatchWatch::default();
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

/// The hook bit `grabbed`: turn a placed object at rest loose, note where the hook bit, take the
/// native rope off the line so that the line's pull is the one force on the target, and play the
/// hook's impact.
///
/// # Safety
///
/// The game thread, before the world update and the physics step.
pub(super) unsafe fn bite(
    hand: Hand,
    grabbed: &Grabbed,
    wire: NonZeroUsize,
) -> Result<Flight, Refusal> {
    let mut hit = grabbed.hit.get().ok_or(Refusal::Gone)?;
    let mut bite_world = unsafe { hit.point() }.ok_or(Refusal::Gone)?;
    let anchor = unsafe { verified_hit(wire, &mut hit, &mut bite_world) }?;
    grabbed.hit.set(Some(hit));
    let (body, owner) = (hit.body, hit.owner);
    if matches!(grabbed.kind, Pulled::Prop { .. }) {
        unsafe { remote::loosen(owner, body) }.map_err(Refusal::Body)?;
    }
    let state = unsafe { physics::body_state(body, owner) }.map_err(Refusal::Body)?;
    unsafe { engine::exclude_rope(wire) };
    if let Some(palm) = ik::tick_records().current[hand.index()].map(|t| t.wrist.position) {
        unsafe { juice::impact(wire, grabbed.object, palm) };
    }
    let (proxy, offset_m) = anchor.map_or((0, 0.0), |anchor| {
        (anchor.proxy, anchor.position.distance(bite_world))
    });
    tracing::info!(
        target: "controllers",
        body,
        proxy,
        offset_m,
        "snatch: the line holds the selected body"
    );
    let landing = match grabbed.kind {
        Pulled::Weapon { .. } => unsafe { engine::dropped(grabbed.object) }
            .ok()
            .and_then(|(weapon, _)| unsafe { Landing::measure(weapon, state.pose) }),
        _ => None,
    };
    Ok(Flight {
        hit,
        bite_world,
        last_centre: None,
        landing,
        slack_s: None,
        stall: StallWatch::default(),
        receiver: None,
        catch: CatchWatch::default(),
    })
}

/// One tick of the flight: the line yanks the target toward its receiver, the free, empty hand of
/// `hands` nearest the target's incoming path ([`catch::receiver`]), slowing over the final
/// approach, then goes slack a couple of ticks out, and a hand catches the target when its grip
/// closes on it within its catch zone or the zone's grace ([`CatchWatch`]). `hands` gives, by
/// [`Hand::index`], each free, empty hand's grip, closed or not. The grapple hand `hand` holds its
/// grip throughout, so it catches only as the receiver.
///
/// The line's pull goes through the hold solve, the physics every hand's hold on a body goes
/// through, with the line's strength and no twist of its own: the target keeps its momentum and its
/// spin, collides on the way, and is never placed. Beyond the final approach the line pulls at the
/// hook's point, so a target hooked off its centre spins; within it the line pulls at the centre,
/// so the centre's own velocity, which the target keeps once the line is slack, is the line's.
pub(super) fn pull(
    game: &GameTick<'_>,
    hand: Hand,
    grabbed: &Grabbed,
    wire: NonZeroUsize,
    flight: &mut Flight,
    hands: [Option<bool>; 2],
    input: &FinishInput,
) -> Result<Step, Refusal> {
    if !grabbed.live() {
        return Err(Refusal::Gone);
    }
    let (reading, line_m) = unsafe { engine::read_wire(wire) };
    if reading == engine::WireReading::Gone && !flight.slack() {
        tracing::info!(
            target: "controllers",
            reading = ?reading,
            slack = flight.slack(),
            body = flight.hit.body,
            owner = flight.hit.owner,
            "snatch: the line came away before the anchor check"
        );
        return Err(Refusal::LineLost);
    }
    let mut hit = flight.hit;
    unsafe { verified_hit(wire, &mut hit, &mut flight.bite_world)? };
    if hit != flight.hit {
        flight.hit = hit;
    }
    let BodyHit { body, owner, .. } = hit;
    let state = unsafe { physics::body_state(body, owner) }.map_err(Refusal::Body)?;
    let heavy = rules::HeavyCatch {
        mass_kg: state.mass_kg.unwrap_or(f32::INFINITY),
        heavy_mass_kg: SNATCH.get().heavy_mass_kg,
        zone_m: SNATCH.get().catch_zone_m,
        heavy_zone_m: SNATCH.get().heavy_catch_zone_m,
        speed_share: SNATCH.get().heavy_speed_share,
    };
    let zone_m = heavy.zone_m();
    let palms = ik::tick_records()
        .current
        .map(|t| t.map(|t| t.wrist.position));
    if palms[hand.index()].is_none() {
        // An untracked hand keeps the line where it is.
        return Ok(Step::Flying);
    }
    let centre = state.pose.centre;
    // Once the line is slack the target flies on by itself, so its receiver stays the hand it was
    // steered to.
    if !flight.slack() {
        let offered = Hand::BOTH.map(|h| palms[h.index()].filter(|_| hands[h.index()].is_some()));
        flight.receiver = catch::receiver(
            flight.receiver,
            offered,
            centre,
            camera::eye_point(),
            SNATCH.get().landing.hysteresis_m,
        );
    }
    let receiver = flight
        .receiver
        .filter(|r| palms[r.index()].is_some())
        .unwrap_or(hand);
    let Some(palm) = palms[receiver.index()] else {
        return Ok(Step::Flying);
    };
    // A placed charge comes off its surface slowly, as the body the drop restores settles into
    // the simulation: it gets longer before it counts as snagged.
    let stall_window_s = match grabbed.kind {
        Pulled::Charge { .. } => SNATCH.get().stall_window_s * SNATCH.get().charge_stall_scale,
        _ => SNATCH.get().stall_window_s,
    };
    let stalled = flight.stall.step(
        input.dt,
        centre,
        stall_window_s,
        SNATCH.get().stall_distance_m,
    );
    if let Some(slack_s) = &mut flight.slack_s {
        *slack_s += input.dt;
    }
    // A gun the landing steers is caught at its grip, where the catching hand will hold it.
    let landed = flight.landing.as_mut().map(|landing| {
        landing.tick(
            game,
            &LandingInput {
                body: state.pose,
                velocity: state.velocities.linear,
                angular: state.velocities.angular,
                receiver,
                palm,
                dt: input.dt,
            },
        )
    });
    let at = landed.map_or(centre, |l| l.grip);
    let from = flight.last_centre.unwrap_or(at);
    flight.last_centre = Some(at);
    let catchers = Hand::BOTH.map(|h| {
        let gripping = hands[h.index()].filter(|_| h != hand || h == receiver)?;
        let target = landed
            .and_then(|l| l.targets[h.index()])
            .map(|t| t.position)
            .or(palms[h.index()])?;
        Some(CatchHand {
            palm: target,
            gripping,
        })
    });
    let caught = flight.catch.step(
        input.dt,
        from,
        at,
        catchers,
        &CatchTuning {
            zone_m,
            grace_s: SNATCH.get().catch_grace_s,
        },
    );
    let sample = FlightSample {
        to: at,
        velocity: state.velocities.linear,
        hand: palm,
        catchers: catchers.map(|c| c.map(|c| c.palm)),
        catch: caught,
        slack_s: flight.slack_s,
    };
    let limits = FlightLimits {
        slack_radius_m: rules::slack_radius(
            state.velocities.linear.length(),
            SNATCH.get().coast_s,
            SNATCH.get().slack_min_m,
            SNATCH.get().slack_max_m,
        ),
        line_m: line_m.unwrap_or(SNATCH.get().range_m),
        max_slack_s: SNATCH.get().max_slack_s,
    };
    // The turn keeps running once the line is slack: it cannot change whether the catch happens.
    let drive = landed.and_then(|l| l.angular);
    match rules::verdict(&sample, stalled, &limits) {
        Verdict::Pulling => {}
        Verdict::Slack => {
            if let Some(angular) = drive {
                unsafe {
                    physics::set_body_velocities(body, owner, state.velocities.linear, angular)
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
            tracing::info!(
                target: "controllers",
                hand = catcher.label(),
                body,
                mass_kg = ?state.mass_kg,
                catch_m = at.distance(catcher_palm),
                zone_m,
                "snatch: the hand caught the target"
            );
            let slide_s = match (&flight.landing, landed) {
                (Some(landing), Some(l)) => l.targets[catcher.index()].map_or(SNAP_S, |target| {
                    let rotation = arrival::gun_rotation(landing, state.pose);
                    let (angle, distance) = arrival::residual(rotation, l.grip, target);
                    let speed = state.velocities.linear.length();
                    let slide_s =
                        landing::arrival_slide(angle, distance, speed, SNAP_S, &SNATCH.get().landing);
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
            let arrived = arrive(game, catcher, grabbed, body, centre, catcher_palm, slide_s);
            if arrived.is_err() {
                // A target the hand caught and could not take drops at the hand, as one let go of
                // does, rather than flying on past the player at the pull's speed.
                let _ =
                    unsafe { physics::set_body_velocities(body, owner, Vec3::ZERO, Vec3::ZERO) };
            }
            return arrived.map(|arrival| Step::Caught(catcher, arrival));
        }
        Verdict::Missed => {
            tracing::info!(
                target: "controllers",
                hand = receiver.label(),
                body,
                mass_kg = ?state.mass_kg,
                miss_m = at.distance(palm),
                zone_m,
                "snatch: the hand missed the catch"
            );
            return Err(Refusal::CatchMissed);
        }
        Verdict::Stalled => return Err(Refusal::Stalled),
        Verdict::OutOfLine => return Err(Refusal::OutOfLine),
    }
    let mass = state.mass_kg.unwrap_or(1.0);
    let tuning = HoldTuning {
        stiffness_per_s: 0.0,
        angular_stiffness_per_s: 0.0,
        max_speed_mps: SNATCH.get().line_speed_mps.max(0.0) * heavy.speed_scale(),
        max_angular_speed_rps: SNATCH.get().max_spin_rps.max(0.0),
        hand_force_n: mass * SNATCH.get().line_accel_mps2.max(0.0),
        hand_torque_nm: 0.0,
        two_hand_angular_share: 1.0,
        gravity: Vec3::new(0.0, -GRAVITY_MPS2, 0.0),
    };
    let rigid = state.rigid();
    let distance = centre.distance(palm);
    let anchor = state.pose.point(flight.hit.local_point);
    let speed = rules::line_speed(
        distance,
        SNATCH.get().line_speed_mps * heavy.speed_scale(),
        SNATCH.get().arrival_speed_mps * heavy.speed_scale(),
        SNATCH.get().approach_m,
    );
    let shift = landed.map_or(Vec3::ZERO, |l| l.shift);
    let aim = rules::pull_point(
        centre,
        palm + shift,
        SNATCH.get().arc,
        SNATCH.get().approach_m,
    ) + (anchor - centre);
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
    let solution = hold::solve_in_frame(&rigid, holds, input.dt, &tuning, Velocities::default());
    unsafe {
        physics::set_body_velocities(
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
    if !grabbed.live() {
        return Err(Refusal::Gone);
    }
    if let Some(hit) = grabbed.hit.get() {
        return Ok((hit.body, hit.owner));
    }
    match grabbed.kind {
        Pulled::Weapon { .. } => Ok((
            unsafe { engine::dropped(grabbed.object) }?.1,
            grabbed.object,
        )),
        Pulled::Prop { body, .. }
        | Pulled::Grenade { body }
        | Pulled::WorldGrenade { body }
        | Pulled::Charge { body, .. } => Ok((body, grabbed.object)),
        Pulled::Ragdoll { owner, body, .. } => Ok((body, owner)),
        Pulled::Door { .. } => Err(Refusal::Gone),
    }
}

/// The body hit and world bite point the line pulls, verified against the wire's live proxy and
/// the object: the selected body while it still belongs to the object, or the object's own body
/// when the proxy holds a different one of its bodies. Returns the anchor it read, for the bite's
/// diagnostic.
///
/// # Safety
///
/// The game thread, before the world update.
unsafe fn verified_hit(
    wire: NonZeroUsize,
    hit: &mut BodyHit,
    bite_world: &mut Vec3,
) -> Result<Option<engine::Anchor>, Refusal> {
    let anchor = unsafe { engine::anchor(wire) };
    let owner = hit.owner;
    let selected_owned = unsafe { physics::body_state(hit.body, owner) }.is_ok();
    let proxy = anchor.map(|anchor| {
        (
            anchor.body,
            unsafe { physics::body_state(anchor.body, owner) }.is_ok(),
        )
    });
    match engine::check_anchor(selected_owned, hit.body, proxy) {
        engine::AnchorCheck::Selected => Ok(anchor),
        engine::AnchorCheck::ObjectBody => {
            let anchor = anchor.ok_or(Refusal::LineLost)?;
            let rebound = unsafe { BodyHit::at(anchor.body, owner, anchor.position) }
                .ok_or(Refusal::LineLost)?;
            tracing::info!(
                target: "controllers",
                selected_body = hit.body,
                proxy_body = anchor.body,
                proxy = anchor.proxy,
                offset_m = anchor.position.distance(*bite_world),
                "snatch: the line follows the object's own body"
            );
            *hit = rebound;
            *bite_world = anchor.position;
            Ok(Some(anchor))
        }
        engine::AnchorCheck::Lost => {
            tracing::info!(
                target: "controllers",
                selected_body = hit.body,
                selected_owned,
                proxy_body = anchor.map(|anchor| anchor.body),
                proxy_owned = proxy.map(|(_, owned)| owned),
                "snatch: the line lost its anchor"
            );
            Err(Refusal::LineLost)
        }
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
    let anchor = wrist.position + wrist.orientation * grapple_origin_m();
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
    /// Whether the other hand holds the line ahead of the grapple hand.
    pub line_held: bool,
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

pub(super) struct SlingInput<'a> {
    pub flight: &'a mut Flight,
    pub rope: &'a mut Option<Sling>,
    pub brace: &'a mut Brace,
}

/// One tick of the held line: hold the target at the line's held length from the grapple hand,
/// reeling in toward the swing's length, with a tension up to the line's strength for the target's
/// mass, raised while the other hand braces the grapple hand's wrist or holds the line ahead of it
/// ([`brace`]), which then anchors the line.
///
/// # Safety
///
/// The game thread, before the world update and physics step.
pub(super) unsafe fn sling(
    hand: Hand,
    grabbed: &Grabbed,
    wire: NonZeroUsize,
    sling: SlingInput<'_>,
    input: &FinishInput,
) -> Result<Swinging, Refusal> {
    if !grabbed.live() {
        return Err(Refusal::Gone);
    }
    let SlingInput {
        flight,
        rope,
        brace,
    } = sling;
    if unsafe { engine::read_wire(wire) }.0 != engine::WireReading::Attached {
        return Err(Refusal::LineLost);
    }
    let mut hit = flight.hit;
    unsafe { verified_hit(wire, &mut hit, &mut flight.bite_world)? };
    if hit != flight.hit {
        flight.hit = hit;
    }
    let BodyHit { body, owner, .. } = hit;
    let state = unsafe { physics::body_state(body, owner) }.map_err(Refusal::Body)?;
    let (line_anchor, wrist) = unsafe { prepare_sling(hand, wire) }?;
    let other = hand.other();
    let other_record = ik::tick_records().current[other.index()];
    let other_wrist = other_record.map(|t| t.wrist.position);
    let other_palm = other_record.map(|t| line_grab::palm(t.wrist));
    let other_free = line_grab::free_and_empty(other);
    let was_braced = brace.braced();
    let was_holding = brace.holds_line();
    let braced = brace.step(
        BraceInput {
            grip_held: input.grips[other.index()].held()
                && matches!(input.latches[other.index()], GripLatch::Fist { .. }),
            free: other_free,
            gap_m: other_wrist.map(|other| other.distance(wrist)),
        },
        BraceReach {
            take_m: SNATCH.get().brace_take_m,
            break_m: SNATCH.get().brace_break_m,
        },
    );
    let braced_anchor = brace::anchor(line_anchor, wrist, other_wrist);
    let mut anchor = if braced { braced_anchor } else { line_anchor };
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
        slot @ None => {
            slot.insert(Sling::new(state.pose.point(hit.local_point), anchor).ok_or(Refusal::Gone)?)
        }
    };
    let regrip = brace.step_line(LineInput {
        holds: !braced
            && other_free
            && input.grips[other.index()].held()
            && line_grab::holds_line(input.latches[other.index()]),
        ahead_m: other_palm.and_then(|palm| {
            line_grab::ahead_m(line_anchor, state.pose.point(hit.local_point), palm)
        }),
        length_m: rope.length_m(),
    });
    let line_held = brace.holds_line();
    if let (true, Some(palm)) = (line_held, other_palm) {
        anchor = palm;
    }
    if let Some(regrip) = regrip {
        let (Regrip::Took { length_m } | Regrip::LetGo { length_m }) = regrip;
        rope.regrip(anchor, length_m);
        tracing::info!(
            target: "controllers",
            length_m,
            "snatch: the free hand {} the line",
            if line_held { "took hold of" } else { "let go of" }
        );
    }
    // A ragdoll's body drags the whole ragdoll, so the line holds and pulls the whole ragdoll's
    // weight.
    let mass_kg = match grabbed.kind {
        Pulled::Ragdoll { mass_kg, .. } => mass_kg,
        _ => state.mass_kg.unwrap_or(1.0),
    };
    let mut line = LineTuning::of(&SNATCH.get().swing, mass_kg, braced || line_held);
    if let Pulled::Ragdoll { .. } = grabbed.kind {
        // A body is yanked in to arm's reach, where the free hand can take it, and then swung on
        // the line held there; at the swing's own length and reel, a body bitten a few metres off
        // barely came.
        // The reel slows as the line nears that length, so the body arrives at a walk rather than
        // flying on through the view at the yank's speed.
        let ragdoll = &SNATCH.get().ragdoll;
        line.swing_length_m = line.swing_length_m.min(ragdoll.length_m);
        let left_m = (rope.length_m() - line.swing_length_m).max(0.0);
        line.reel_mps = (left_m * ragdoll.ease_per_s).clamp(line.reel_mps, ragdoll.mps);
    }
    let rigid = state.rigid();
    let hook_point = state.pose.point(hit.local_point);
    let out = rope
        .step(SlingStep::on_body(
            &rigid,
            hit.local_point,
            mass_kg,
            anchor,
            input.dt,
            line,
        ))
        .map_err(|fault| match fault {
            RopeFault::Invalid => Refusal::Gone,
            RopeFault::Stretched => Refusal::OutOfLine,
        })?;
    let impulse = out.impulse_on_b(input.dt);
    match grabbed.kind {
        Pulled::Ragdoll { .. } => unsafe {
            remote::pull_ragdoll(grabbed.object, body, owner, impulse, hook_point)
        },
        _ => unsafe { remote::push(body, owner, impulse, hook_point) },
    }
    .map_err(Refusal::Body)?;
    let swinging = Swinging {
        length_m: rope.length_m(),
        tension_n: out.tension_n,
        max_tension_n: line.max_tension_n,
        mass_kg,
        braced,
        line_held,
        anchor,
        centre: hook_point,
    };
    haptics::sustain(hand, Sustain::GrappleTension, swinging.share());
    if braced || line_held {
        haptics::sustain(other, Sustain::GrappleTension, swinging.share());
    } else if was_braced || was_holding {
        haptics::sustain(other, Sustain::GrappleTension, 0.0);
    }
    Ok(swinging)
}

/// Hand what the hand caught over: a weapon into the hand through the table, sliding the rest of
/// the way into its grip over `slide_s`, an object into the hand's hold at its surface nearest the
/// palm, and a grenade into the hand through the loadout.
fn arrive(
    game: &GameTick<'_>,
    hand: Hand,
    grabbed: &Grabbed,
    body: u32,
    centre: Vec3,
    palm: Vec3,
    slide_s: f32,
) -> Result<Arrival, Refusal> {
    match grabbed.kind {
        Pulled::Weapon { source } => {
            let (weapon, _) = unsafe { engine::dropped(grabbed.object) }?;
            let address = weapon as *mut WeaponBase as usize;
            unsafe { hold_source::deliver(game, hand, weapon, source, slide_s) }
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
        Pulled::Charge { .. } => {
            if grabbed.catch_charge(hand) {
                Ok(Arrival::Charge)
            } else {
                Err(Refusal::Gone)
            }
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

/// The centre of mass of `object`'s body `body`, world space.
///
/// # Safety
///
/// The game thread, before the world update.
unsafe fn body_centre(body: u32, object: NonZeroUsize) -> Result<Vec3, Refusal> {
    unsafe { physics::body_state(body, object) }
        .map(|state| state.pose.centre)
        .map_err(Refusal::Body)
}

/// Toss the weapon a tap of the grapple reeled in to land in front of the player.
pub(super) fn toss(game: &GameTick<'_>, weak: &Weak) -> Result<(), Refusal> {
    let alive = weak.lock().ok_or(Refusal::Gone)?;
    let weapon = NonZeroUsize::new(weapon_of(&alive) as usize).ok_or(Refusal::Gone)?;
    let (_, body) = unsafe { engine::dropped(weapon) }?;
    let from = unsafe { body_centre(body, weapon) }?;
    let player = engine::player_position(game).ok_or(Refusal::NoPlayer)?;
    let toward = Vec3::new(from.x - player.x, 0.0, from.z - player.z)
        .try_normalize()
        .unwrap_or(Vec3::ZERO);
    let toss = &SNATCH.get().toss;
    let land = player + toward * toss.land_m;
    let flight = rules::toss_flight(
        from.distance(land),
        toss.speed_mps,
        toss.min_flight_s,
        toss.max_flight_s,
    );
    let velocity = rules::toss_velocity(from, land, flight, Vec3::new(0.0, -GRAVITY_MPS2, 0.0));
    unsafe { physics::set_body_velocities(body, weapon, velocity, Vec3::ZERO) }
        .map_err(Refusal::Body)
}
