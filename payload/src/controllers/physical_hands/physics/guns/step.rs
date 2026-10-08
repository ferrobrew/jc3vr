//! The gun bodies' lifecycle, owned by the pre-simulation step: each holding hand's gun body is
//! made where the gun was last placed, joined to the holding hand's body on its seat and to the
//! bracing hand's where that hand holds it, kept on its seat, kicked by the shots, and destroyed,
//! its joints first, when the gun leaves the hand or the hand's body stops.
//!
//! Every function here changes the physics world, so it runs only on the thread that steps it,
//! outside the step: from the pre-simulation callback, or from the shutdown cleanup on the game
//! thread.

use std::time::Instant;

use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    physical_hands::physics::{
        body::{BodyShape, BodySpec, HandBody},
        config::HeldGunsConfig,
        contact::{self, Source},
        drive::BodyMotion,
        guns::{
            GunBodyStatus, GunHolds, HeldGun, SHARED,
            engine::{GunError, check_own_body, read_source},
            math::{
                GunAxes, MassSource, class_value, deviation, gun_mass, seat, seated, shot_impulse,
                strayed, turn_rad,
            },
        },
        joint::Joint,
        lifecycle::{Hands, PLACED_MUTE_STEPS},
        types::BodyPhase,
    },
};

/// A holding hand's gun body.
pub(crate) struct GunSlot {
    body: HandBody,
    held: HeldGun,
    mass_kg: f32,
    mass_source: MassSource,
    axes: Option<GunAxes>,
    /// The joint to the holding hand's body, and the gun's seat on it.
    holder: Option<(Joint, Pose)>,
    /// The joint to the bracing hand's body.
    brace: Option<Joint>,
}

/// What one step's gun phase works from.
pub(crate) struct GunStep<'a> {
    /// The guns in the hands, as last published; empty when stale or switched off.
    pub holds: GunHolds,
    pub config: &'a HeldGunsConfig,
    /// Whether the gun bodies' contacts are listened for.
    pub contact_events: bool,
    /// The player's collision system group.
    pub group: Option<u32>,
}

/// Make, join, keep, and destroy the gun bodies for this step, after the hands acted.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step.
pub(crate) unsafe fn step(hands: &mut Hands, input: GunStep<'_>) {
    let now = Instant::now();
    let shots = std::mem::take(&mut SHARED.lock().shots);
    let mut statuses = [None; 2];
    let mut deviations = [None; 2];
    let mut tally = Tally::default();
    for hand in Hand::BOTH {
        let i = hand.index();
        let wanted = input.holds.guns[i].filter(|_| input.group.is_some());
        let acting = hands.phase(hand) == BodyPhase::Acting;
        if let Some(slot) = hands.guns[i].take() {
            let reason = if !acting {
                Some("the holding hand's body stopped")
            } else if wanted.is_some_and(|w| {
                HeldGun {
                    carry: slot.held.carry,
                    ..w
                } == slot.held
            }) && wanted != Some(slot.held)
            {
                Some("the hand holds the gun by another part")
            } else if wanted != Some(slot.held) {
                Some("the gun left the hand")
            } else if Some(slot.body.spec().system_group) != input.group {
                Some("the player's collision group changed")
            } else if slot.body.spec().contact_events != input.contact_events {
                Some("the hits were switched on or off")
            } else {
                None
            };
            match reason {
                Some(reason) => {
                    // SAFETY: forwarded from the caller.
                    tally.destroyed += u64::from(unsafe { destroy_slot(slot, hand, reason) });
                }
                None => hands.guns[i] = Some(slot),
            }
        }
        let (Some(held), Some(group)) = (wanted, input.group) else {
            continue;
        };
        if !acting {
            continue;
        }
        if hands.guns[i].is_none() {
            // SAFETY: forwarded from the caller.
            match unsafe { make(hands, hand, held, group, &input) } {
                Ok(slot) => {
                    tally.made += 1;
                    hands.guns[i] = Some(slot);
                    forget_refusal(hand);
                }
                Err(e) => {
                    note_refusal(hand, held, e);
                    // A gun on its way into the hand, or just let go of, is no fault.
                    if !matches!(e, GunError::NotInHand | GunError::OwnBodyInWorld) {
                        tally.error = Some(e.to_string());
                    }
                    continue;
                }
            }
        }
        // SAFETY: forwarded from the caller.
        let kept = unsafe { keep(hands, hand, &input, shots[i], &mut tally) };
        match kept {
            Ok(kept) => {
                statuses[i] = Some(kept.status);
                deviations[i] = Some((held.weapon.get(), kept.deviation));
            }
            Err(reason) => {
                if let Some(slot) = hands.guns[i].take() {
                    // SAFETY: forwarded from the caller.
                    tally.destroyed += u64::from(unsafe { destroy_slot(slot, hand, reason) });
                }
            }
        }
    }
    let mut shared = SHARED.lock();
    shared.deviations = deviations;
    shared.stepped = Some(now);
    let status = &mut shared.status;
    status.hands = statuses;
    status.made += tally.made;
    status.destroyed += tally.destroyed;
    status.reseats += tally.reseats;
    status.shots += tally.shots;
    if let Some(error) = tally.error {
        status.last_error = Some(error);
    }
}

/// Forget the last step's gun bodies and the shots queued, for a step that acts on no hand (the
/// hand bodies off, or the game out of gameplay), whose bodies, guns included, it destroys.
pub(crate) fn stand_down() {
    let mut shared = SHARED.lock();
    shared.shots = [None; 2];
    shared.deviations = [None; 2];
    shared.status.hands = [None; 2];
}

/// Destroy every gun body that hangs from `hand`'s body: the gun it holds, with its joints, and
/// the brace joint it holds the other hand's gun by. Called before `hand`'s body is parked, held,
/// forgotten, or destroyed, so that no joint outlives either of its bodies.
///
/// # Safety
///
/// As [`step`].
pub(crate) unsafe fn drop_hand(guns: &mut [Option<GunSlot>; 2], hand: Hand, reason: &str) {
    if let Some(slot) = guns[hand.index()].take() {
        // SAFETY: forwarded from the caller.
        let destroyed = unsafe { destroy_slot(slot, hand, reason) };
        SHARED.lock().status.destroyed += u64::from(destroyed);
    }
    if let Some(slot) = guns[hand.other().index()].as_mut()
        && let Some(brace) = slot.brace.take()
    {
        // SAFETY: forwarded from the caller.
        unsafe { end_joint(brace, hand, "brace", reason) };
    }
}

/// What one step did to the gun bodies, for the readout.
#[derive(Default)]
struct Tally {
    made: u64,
    destroyed: u64,
    reseats: u64,
    shots: u64,
    error: Option<String>,
}

/// Make `hand`'s gun body where the gun was last placed, moving with the hand, and join it to the
/// hand's body on its seat.
///
/// # Safety
///
/// As [`step`]; `hand`'s body acts.
unsafe fn make(
    hands: &mut Hands,
    hand: Hand,
    held: HeldGun,
    group: u32,
    input: &GunStep<'_>,
) -> Result<GunSlot, GunError> {
    let hand_body = hands.bodies[hand.index()]
        .as_ref()
        .ok_or(GunError::Body("the holding hand has no body"))?;
    // SAFETY: forwarded from the caller.
    let hand_motion =
        unsafe { hand_body.motion() }.map_err(|_| GunError::Body("the hand body is unreadable"))?;
    let hand_id = hand_body.body_id();
    // SAFETY: forwarded from the caller.
    let source = unsafe { read_source(&held) }?;
    if source.pose.position.distance(hand_motion.position) > input.config.break_distance_m.max(0.0)
    {
        return Err(GunError::NotInHand);
    }
    let (mass_kg, mass_source) = gun_mass(source.inverse_mass, held.class, input.config);
    let spec = BodySpec {
        shape: BodyShape::Shared(source.shape),
        mass_kg,
        system_group: group,
        contact_events: input.contact_events,
        gravity: true,
    };
    // SAFETY: forwarded from the caller; the shape is the live gun's own body's, read just now.
    let mut body = unsafe {
        HandBody::create(
            Source::Gun(hand),
            spec,
            source.pose.position,
            source.pose.orientation,
        )
    }
    .map_err(|_| GunError::Body("the gun body could not be made"))?;
    // Added, then placed, as the engine activates its own free bodies, moving with the hand.
    unsafe { body.add() };
    let placed = unsafe {
        body.place(source.pose.position, source.pose.orientation)
            .and_then(|()| body.set_velocities(hand_motion.linear, hand_motion.angular))
    };
    if placed.is_err() {
        unsafe { body.destroy() };
        return Err(GunError::Body("the gun body could not be placed"));
    }
    contact::mute_for(Source::Gun(hand), PLACED_MUTE_STEPS);
    // SAFETY: forwarded from the caller; both bodies are in the simulation.
    let joint = match unsafe { Joint::create_between(hand_id, body.body_id()) } {
        Ok(joint) => joint,
        Err(e) => {
            tracing::warn!(
                hand = hand.label(),
                error = %e,
                "held guns: could not join a gun body to its hand"
            );
            unsafe { body.destroy() };
            return Err(GunError::Body(
                "the gun body could not be joined to its hand",
            ));
        }
    };
    let gun_pose = unsafe { body.motion() }.map_or(source.pose, |m| pose_of(&m));
    let seat = seat(pose_of(&hand_motion), gun_pose);
    tracing::info!(
        hand = hand.label(),
        weapon = format_args!("{:#x}", held.weapon.get()),
        class = held.class.label(),
        body_id = body.body_id(),
        hand_body = hand_id,
        constraint = joint.id(),
        mass_kg,
        mass_from = mass_source.label(),
        seat_m = seat.position.length(),
        listens = input.contact_events,
        "held guns: made a gun body and joined it to its hand"
    );
    Ok(GunSlot {
        body,
        held,
        mass_kg,
        mass_source,
        axes: source.axes,
        holder: Some((joint, seat)),
        brace: None,
    })
}

/// One step of a kept gun body.
struct Kept {
    status: GunBodyStatus,
    deviation: Pose,
}

/// Keep `hand`'s gun body for this step: check its joint, put it back on its seat when its hand
/// was put somewhere or it strayed, give it the shots queued for it, and make or end its brace.
/// `Err` with the reason when the gun body must go.
///
/// # Safety
///
/// As [`step`]; `hand` has a gun slot and an acting body.
unsafe fn keep(
    hands: &mut Hands,
    hand: Hand,
    input: &GunStep<'_>,
    shots: Option<(usize, u32)>,
    tally: &mut Tally,
) -> Result<Kept, &'static str> {
    let i = hand.index();
    let off = hand.other();
    let placed = hands.placed;
    let off_body = hands.bodies[off.index()]
        .as_ref()
        .filter(|_| hands.phase(off) == BodyPhase::Acting && hands.guns[off.index()].is_none())
        .map(HandBody::body_id);
    let hand_motion = hands.bodies[i]
        .as_ref()
        // SAFETY: forwarded from the caller.
        .and_then(|b| unsafe { b.motion() }.ok())
        .ok_or("the holding hand's body is unreadable")?;
    let slot = hands.guns[i].as_mut().ok_or("the gun body is gone")?;
    // SAFETY: forwarded from the caller.
    match unsafe { check_own_body(&slot.held) } {
        Ok(()) => {}
        Err(GunError::OwnBodyInWorld) => return Err("the gun's own body is back in the world"),
        Err(_) => return Err("the gun is gone"),
    }
    let (joint, seat) = slot.holder.as_ref().ok_or("the gun body has no joint")?;
    // SAFETY: forwarded from the caller.
    if !unsafe { joint.intact() } {
        return Err("the joint to the holding hand went away");
    }
    let seat = *seat;
    // SAFETY: forwarded from the caller.
    let gun_motion = unsafe { slot.body.motion() }.map_err(|_| "the gun body was lost")?;
    let hand_pose = pose_of(&hand_motion);
    let mut gun_pose = pose_of(&gun_motion);
    let mut drift = deviation(hand_pose, seat, gun_pose);
    let reseat = if placed[i] {
        Some("its hand was put somewhere")
    } else if strayed(drift, input.config.break_distance_m) {
        Some("it strayed from its seat")
    } else {
        None
    };
    if let Some(reason) = reseat {
        let back = seated(hand_pose, seat);
        // SAFETY: forwarded from the caller.
        unsafe {
            slot.body
                .place(back.position, back.orientation)
                .and_then(|()| {
                    slot.body
                        .set_velocities(hand_motion.linear, hand_motion.angular)
                })
        }
        .map_err(|_| "the gun body could not be put back on its seat")?;
        contact::mute_for(Source::Gun(hand), PLACED_MUTE_STEPS);
        tracing::debug!(
            hand = hand.label(),
            reason,
            "held guns: put a gun body back on its seat"
        );
        gun_pose = back;
        drift = Pose::IDENTITY;
        tally.reseats += 1;
    }
    if let Some((_, count)) = shots.filter(|(w, _)| *w == slot.held.weapon.get())
        && let Some(axes) = slot.axes
    {
        let per_shot = class_value(&input.config.recoil_impulse_ns, slot.held.class);
        let (impulse, at) = shot_impulse(
            gun_pose,
            &axes,
            per_shot * count as f32,
            input.config.muzzle_rise,
        );
        // SAFETY: forwarded from the caller.
        if unsafe { slot.body.apply_impulse(impulse, at) }.is_ok() {
            tally.shots += u64::from(count);
        }
    }
    contact::record_motion(Source::Gun(hand), gun_pose.position, gun_motion.linear);

    // The bracing hand holds the gun where it took it, until it lets go or its body is put
    // somewhere.
    let wants_brace = input.holds.braces[off.index()] && off_body.is_some() && !placed[off.index()];
    if let Some(brace) = slot.brace.take() {
        // SAFETY: forwarded from the caller.
        let keep = wants_brace && unsafe { brace.intact() };
        if keep {
            slot.brace = Some(brace);
        } else {
            let reason = if wants_brace {
                "the joint went away"
            } else {
                "the bracing hand let go"
            };
            // SAFETY: forwarded from the caller.
            unsafe { end_joint(brace, off, "brace", reason) };
        }
    }
    if wants_brace
        && slot.brace.is_none()
        && let Some(off_id) = off_body
    {
        // SAFETY: forwarded from the caller; both bodies are in the simulation.
        match unsafe { Joint::create_between(off_id, slot.body.body_id()) } {
            Ok(brace) => {
                tracing::info!(
                    hand = off.label(),
                    constraint = brace.id(),
                    gun_body = slot.body.body_id(),
                    "held guns: joined the bracing hand to the gun"
                );
                slot.brace = Some(brace);
            }
            Err(e) => {
                tracing::warn!(
                    hand = off.label(),
                    error = %e,
                    "held guns: could not join the bracing hand to the gun"
                );
            }
        }
    }
    let holder_joint = slot.holder.as_ref().map(|(j, _)| j.id());
    Ok(Kept {
        status: GunBodyStatus {
            weapon: slot.held.weapon.get(),
            body_id: slot.body.body_id(),
            // SAFETY: forwarded from the caller.
            in_world: unsafe { slot.body.is_added() },
            mass_kg: slot.mass_kg,
            mass_source: slot.mass_source,
            holder_joint,
            brace_joint: slot.brace.as_ref().map(Joint::id),
            deviation_m: drift.position.length(),
            deviation_rad: turn_rad(drift.orientation),
            speed_mps: gun_motion.linear.length(),
        },
        deviation: drift,
    })
}

/// Destroy a gun body: its brace joint, its holder joint, then the body. Returns whether the body
/// was destroyed rather than forgotten.
///
/// # Safety
///
/// As [`step`].
unsafe fn destroy_slot(slot: GunSlot, hand: Hand, reason: &str) -> bool {
    let GunSlot {
        body,
        holder,
        brace,
        held,
        ..
    } = slot;
    if let Some(brace) = brace {
        // SAFETY: forwarded from the caller.
        unsafe { end_joint(brace, hand.other(), "brace", reason) };
    }
    if let Some((joint, _)) = holder {
        // SAFETY: forwarded from the caller.
        unsafe { end_joint(joint, hand, "holder", reason) };
    }
    let body_id = body.body_id();
    // SAFETY: forwarded from the caller; no joint names the body any more.
    let destroyed = unsafe { body.destroy() };
    tracing::info!(
        hand = hand.label(),
        weapon = format_args!("{:#x}", held.weapon.get()),
        body_id,
        destroyed,
        reason,
        "held guns: destroyed a gun body"
    );
    destroyed
}

/// End a gun's joint, `role` the holder's or the brace's.
///
/// # Safety
///
/// As [`step`].
unsafe fn end_joint(joint: Joint, hand: Hand, role: &str, reason: &str) {
    let constraint = joint.id();
    // SAFETY: forwarded from the caller.
    let destroyed = unsafe { joint.destroy() };
    tracing::info!(
        hand = hand.label(),
        role,
        constraint,
        destroyed,
        reason,
        "held guns: ended a gun's joint"
    );
}

fn pose_of(motion: &BodyMotion) -> Pose {
    Pose {
        position: motion.position,
        orientation: motion.orientation,
    }
}

/// The gun each hand last could not make a body for, so that a refusal is logged once per gun.
static REFUSED: Mutex<[Option<(usize, GunError)>; 2]> = Mutex::new([None; 2]);

fn note_refusal(hand: Hand, held: HeldGun, error: GunError) {
    let mut refused = REFUSED.lock();
    let entry = Some((held.gun.get(), error));
    if refused[hand.index()] == entry {
        return;
    }
    refused[hand.index()] = entry;
    // A gun still sliding into the hand, or just let go of, is not worth more than a debug line.
    if matches!(error, GunError::NotInHand | GunError::OwnBodyInWorld) {
        tracing::debug!(hand = hand.label(), error = %error, "held guns: no gun body yet");
    } else {
        tracing::info!(
            hand = hand.label(),
            weapon = format_args!("{:#x}", held.weapon.get()),
            error = %error,
            "held guns: could not make a gun body"
        );
    }
}

fn forget_refusal(hand: Hand) {
    REFUSED.lock()[hand.index()] = None;
}
