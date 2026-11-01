//! The encoding of [`Event`]s, in both directions.
//!
//! An event's payload is its `time_ns` (`u64`), a kind byte, and the kind's fields in declaration
//! order. A label is a string; an optional value is a presence byte and, when present, the value;
//! a list is a `u16` count and its entries.

use crate::{
    codec::{hand_code, read_hand},
    event::{
        AimState, ControllerState, Event, EventKind, GrappleAim, GripAcquisition, GripCandidate,
        GripDrive, GripScan, GripScanEntry, GripStage, GunRelease, HandAim, HandLate, HandTier,
        IkState, Label, LateState, ReleaseController, ReleaseDrop, RoomscaleState, RootTick,
        TakeMarker, TierState, WeaponAim, WorldMotion,
    },
    model::Hand,
    wire::{DecodeError, DecodeErrorKind, Decoder, Encoder},
};

/// The most entries a list in an event holds.
pub(crate) const MAX_LIST: usize = 256;

pub(crate) fn encode(out: &mut Encoder, event: &Event) {
    out.u64(event.time_ns);
    match &event.kind {
        EventKind::ControllerState(state) => {
            out.u8(KIND_CONTROLLER_STATE);
            controller_state(out, state);
        }
        EventKind::GripAcquisition(grip) => {
            out.u8(KIND_GRIP_ACQUISITION);
            grip_acquisition(out, grip);
        }
        EventKind::GunRelease(release) => {
            out.u8(KIND_GUN_RELEASE);
            gun_release(out, release);
        }
        EventKind::TakeMarker(marker) => {
            out.u8(KIND_TAKE_MARKER);
            take_marker(out, marker);
        }
    }
}

/// Decode an event from a container of `version`.
pub(crate) fn decode(input: &mut Decoder, version: u16) -> Result<Event, DecodeError> {
    let time_ns = input.u64()?;
    let offset = input.offset();
    let kind = match input.u8()? {
        KIND_CONTROLLER_STATE => {
            EventKind::ControllerState(Box::new(read_controller_state(input, version)?))
        }
        KIND_GRIP_ACQUISITION => {
            EventKind::GripAcquisition(Box::new(read_grip_acquisition(input)?))
        }
        KIND_GUN_RELEASE => EventKind::GunRelease(Box::new(read_gun_release(input)?)),
        KIND_TAKE_MARKER => EventKind::TakeMarker(Box::new(read_take_marker(input)?)),
        kind => return Err(invalid(offset, "the event kind", kind.into())),
    };
    Ok(Event { time_ns, kind })
}

/// Whether an event's lists fit [`MAX_LIST`], so that it encodes without truncation.
pub(crate) fn is_bounded(event: &Event) -> bool {
    match &event.kind {
        EventKind::GripAcquisition(grip) => {
            grip.candidates.len() <= MAX_LIST
                && grip
                    .scan
                    .as_ref()
                    .is_none_or(|scan| scan.entries.len() <= MAX_LIST)
        }
        EventKind::ControllerState(_) | EventKind::GunRelease(_) | EventKind::TakeMarker(_) => true,
    }
}

const KIND_CONTROLLER_STATE: u8 = 1;
const KIND_GRIP_ACQUISITION: u8 = 2;
const KIND_GUN_RELEASE: u8 = 3;
const KIND_TAKE_MARKER: u8 = 4;

fn invalid(offset: usize, field: &'static str, value: u64) -> DecodeError {
    DecodeError {
        offset,
        kind: DecodeErrorKind::InvalidValue { field, value },
    }
}

fn opt<T>(out: &mut Encoder, value: Option<&T>, write: impl FnOnce(&mut Encoder, &T)) {
    match value {
        Some(value) => {
            out.bool(true);
            write(out, value);
        }
        None => out.bool(false),
    }
}

fn read_opt<T>(
    input: &mut Decoder,
    field: &'static str,
    read: impl FnOnce(&mut Decoder) -> Result<T, DecodeError>,
) -> Result<Option<T>, DecodeError> {
    if input.bool(field)? {
        read(input).map(Some)
    } else {
        Ok(None)
    }
}

fn label(out: &mut Encoder, value: &Label) {
    out.str(value);
}

fn read_label(input: &mut Decoder, field: &'static str) -> Result<Label, DecodeError> {
    input.str(field).map(Label::Owned)
}

fn opt_label(out: &mut Encoder, value: &Option<Label>) {
    opt(out, value.as_ref(), label);
}

fn read_opt_label(input: &mut Decoder, field: &'static str) -> Result<Option<Label>, DecodeError> {
    read_opt(input, field, |input| read_label(input, field))
}

fn opt_f32(out: &mut Encoder, value: Option<f32>) {
    opt(out, value.as_ref(), |out, value| out.f32(*value));
}

fn read_opt_f32(input: &mut Decoder, field: &'static str) -> Result<Option<f32>, DecodeError> {
    read_opt(input, field, |input| input.f32())
}

fn opt_u32(out: &mut Encoder, value: Option<u32>) {
    opt(out, value.as_ref(), |out, value| out.u32(*value));
}

fn read_opt_u32(input: &mut Decoder, field: &'static str) -> Result<Option<u32>, DecodeError> {
    read_opt(input, field, |input| input.u32())
}

fn opt_u64(out: &mut Encoder, value: Option<u64>) {
    opt(out, value.as_ref(), |out, value| out.u64(*value));
}

fn read_opt_u64(input: &mut Decoder, field: &'static str) -> Result<Option<u64>, DecodeError> {
    read_opt(input, field, |input| input.u64())
}

fn opt_vec3(out: &mut Encoder, value: Option<glam::Vec3>) {
    opt(out, value.as_ref(), |out, value| out.vec3(*value));
}

fn read_opt_vec3(
    input: &mut Decoder,
    field: &'static str,
) -> Result<Option<glam::Vec3>, DecodeError> {
    read_opt(input, field, |input| input.vec3())
}

fn list<T>(out: &mut Encoder, values: &[T], write: impl Fn(&mut Encoder, &T)) {
    let values = &values[..values.len().min(MAX_LIST)];
    out.u16(values.len() as u16);
    for value in values {
        write(out, value);
    }
}

fn read_list<T>(
    input: &mut Decoder,
    field: &'static str,
    read: impl Fn(&mut Decoder) -> Result<T, DecodeError>,
) -> Result<Vec<T>, DecodeError> {
    let offset = input.offset();
    let count = usize::from(input.u16()?);
    if count > MAX_LIST {
        return Err(invalid(offset, field, count as u64));
    }
    (0..count).map(|_| read(input)).collect()
}

fn bools<const N: usize>(out: &mut Encoder, values: [bool; N]) {
    let mut flags = 0u8;
    for (index, value) in values.into_iter().enumerate() {
        flags |= u8::from(value) << index;
    }
    out.u8(flags);
}

fn read_bools<const N: usize>(
    input: &mut Decoder,
    field: &'static str,
) -> Result<[bool; N], DecodeError> {
    let offset = input.offset();
    let flags = input.u8()?;
    if u32::from(flags) >> N != 0 {
        return Err(invalid(offset, field, flags.into()));
    }
    Ok(std::array::from_fn(|index| flags & (1 << index) != 0))
}

fn opt_hand(out: &mut Encoder, hand: Option<Hand>) {
    out.u8(hand.map_or(NO_HAND, hand_code));
}

fn read_opt_hand(input: &mut Decoder, field: &'static str) -> Result<Option<Hand>, DecodeError> {
    let offset = input.offset();
    match input.u8()? {
        NO_HAND => Ok(None),
        0 => Ok(Some(Hand::Left)),
        1 => Ok(Some(Hand::Right)),
        value => Err(invalid(offset, field, value.into())),
    }
}

const NO_HAND: u8 = 0xff;

fn controller_state(out: &mut Encoder, state: &ControllerState) {
    opt_u64(out, state.frame_sequence);
    opt_u64(out, state.publication_age_us);
    label(out, &state.scheme);
    label(out, &state.mode);
    opt_label(out, &state.traversal);
    opt_label(out, &state.held);
    opt_label(out, &state.input_error);
    for ik in &state.ik {
        ik_state(out, ik);
    }
    opt_label(out, &state.tiers.state);
    for tier in &state.tiers.hands {
        hand_tier(out, tier);
    }
    for late in &state.late.hands {
        hand_late(out, late);
    }
    out.u64(state.late.weapons_updated);
    out.u64(state.late.palettes_edited);
    aim_state(out, &state.aim);
    roomscale_state(out, &state.roomscale);
    opt(out, state.root.as_ref(), root_tick);
}

fn read_controller_state(
    input: &mut Decoder,
    version: u16,
) -> Result<ControllerState, DecodeError> {
    let frame_sequence = read_opt_u64(input, "the controller state's frame sequence flag")?;
    let publication_age_us = read_opt_u64(input, "the controller state's age flag")?;
    let scheme = read_label(input, "the scheme")?;
    let mode = read_label(input, "the mode")?;
    let traversal = read_opt_label(input, "the traversal")?;
    let held = read_opt_label(input, "the mode hold")?;
    let input_error = read_opt_label(input, "the input error")?;
    let ik = [read_ik_state(input)?, read_ik_state(input)?];
    let tier_state = read_opt_label(input, "the tier state")?;
    let tier_hands = [read_hand_tier(input)?, read_hand_tier(input)?];
    let late_hands = [read_hand_late(input)?, read_hand_late(input)?];
    let late = LateState {
        hands: late_hands,
        weapons_updated: input.u64()?,
        palettes_edited: input.u64()?,
    };
    let aim = read_aim_state(input)?;
    let roomscale = read_roomscale_state(input)?;
    let root = if version >= ROOT_TICK_VERSION {
        read_opt(input, "the root tick flag", read_root_tick)?
    } else {
        None
    };
    Ok(ControllerState {
        frame_sequence,
        publication_age_us,
        scheme,
        mode,
        traversal,
        held,
        input_error,
        ik,
        tiers: TierState {
            state: tier_state,
            hands: tier_hands,
        },
        late,
        aim,
        roomscale,
        root,
    })
}

/// The first version whose controller state carries the root's tick.
const ROOT_TICK_VERSION: u16 = 6;

fn root_tick(out: &mut Encoder, root: &RootTick) {
    out.u64(root.tick);
    out.f32(root.dt_s);
    out.vec3(root.position);
    out.vec3(root.moved_m);
    out.f32(root.unexplained_m);
    out.vec2(root.task_mps);
    out.vec2(root.stick_mps);
    out.vec2(root.palms_mps);
    out.vec2(root.chase_mps);
    out.vec3(root.wanted_mps);
    out.vec3(root.proxy_mps);
    out.vec3(root.surface_mps);
    label(out, &root.support);
    bools(
        out,
        [
            root.surface_dynamic,
            root.authored,
            root.capsule_owned,
            root.capsule_blocked,
        ],
    );
    opt_u32(out, root.ground_body);
    out.i32(root.proxy_state);
    out.i32(root.pending_proxy_state);
    out.i32(root.default_proxy_state);
    out.u64(root.jumps);
}

fn read_root_tick(input: &mut Decoder) -> Result<RootTick, DecodeError> {
    let tick = input.u64()?;
    let dt_s = input.f32()?;
    let position = input.vec3()?;
    let moved_m = input.vec3()?;
    let unexplained_m = input.f32()?;
    let task_mps = input.vec2()?;
    let stick_mps = input.vec2()?;
    let palms_mps = input.vec2()?;
    let chase_mps = input.vec2()?;
    let wanted_mps = input.vec3()?;
    let proxy_mps = input.vec3()?;
    let surface_mps = input.vec3()?;
    let support = read_label(input, "the root's support")?;
    let [surface_dynamic, authored, capsule_owned, capsule_blocked] =
        read_bools(input, "the root tick's flags")?;
    Ok(RootTick {
        tick,
        dt_s,
        position,
        moved_m,
        unexplained_m,
        task_mps,
        stick_mps,
        palms_mps,
        chase_mps,
        wanted_mps,
        proxy_mps,
        surface_mps,
        support,
        surface_dynamic,
        ground_body: read_opt_u32(input, "the root's ground body flag")?,
        proxy_state: input.i32()?,
        pending_proxy_state: input.i32()?,
        default_proxy_state: input.i32()?,
        authored,
        capsule_owned,
        capsule_blocked,
        jumps: input.u64()?,
    })
}

fn hand_tier(out: &mut Encoder, tier: &HandTier) {
    label(out, &tier.assigned);
    label(out, &tier.effective);
    out.f32(tier.controller_weight);
    out.f32(tier.anchor_weight);
}

fn read_hand_tier(input: &mut Decoder) -> Result<HandTier, DecodeError> {
    Ok(HandTier {
        assigned: read_label(input, "the assigned tier")?,
        effective: read_label(input, "the effective tier")?,
        controller_weight: input.f32()?,
        anchor_weight: input.f32()?,
    })
}

fn ik_state(out: &mut Encoder, ik: &IkState) {
    bools(
        out,
        [
            ik.queued,
            ik.secondary_wrist_off,
            ik.secondary_grip_off,
            ik.overrode_live_target,
        ],
    );
    opt_label(out, &ik.skip);
    out.f32(ik.weight);
    out.f32(ik.reach_distance_m);
    out.f32(ik.rotation_deg);
}

fn read_ik_state(input: &mut Decoder) -> Result<IkState, DecodeError> {
    let [
        queued,
        secondary_wrist_off,
        secondary_grip_off,
        overrode_live_target,
    ] = read_bools(input, "the IK flags")?;
    Ok(IkState {
        queued,
        skip: read_opt_label(input, "the IK skip")?,
        weight: input.f32()?,
        reach_distance_m: input.f32()?,
        rotation_deg: input.f32()?,
        secondary_wrist_off,
        secondary_grip_off,
        overrode_live_target,
    })
}

fn hand_late(out: &mut Encoder, late: &HandLate) {
    label(out, &late.reason);
    opt_f32(out, late.physical_displacement_m);
    opt_f32(out, late.relative_rotation_deg);
    opt_vec3(out, late.requested_wrist);
    opt_vec3(out, late.reference_wrist);
    opt_vec3(out, late.latest_wrist);
    opt_vec3(out, late.achieved_wrist_model);
}

fn read_hand_late(input: &mut Decoder) -> Result<HandLate, DecodeError> {
    Ok(HandLate {
        reason: read_label(input, "the late reason")?,
        physical_displacement_m: read_opt_f32(input, "the late displacement flag")?,
        relative_rotation_deg: read_opt_f32(input, "the late rotation flag")?,
        requested_wrist: read_opt_vec3(input, "the requested wrist flag")?,
        reference_wrist: read_opt_vec3(input, "the reference wrist flag")?,
        latest_wrist: read_opt_vec3(input, "the latest wrist flag")?,
        achieved_wrist_model: read_opt_vec3(input, "the achieved wrist flag")?,
    })
}

fn aim_state(out: &mut Encoder, aim: &AimState) {
    opt_label(out, &aim.idle);
    for hand in &aim.hands {
        opt(out, hand.as_ref(), |out, hand| {
            label(out, &hand.source);
            opt_u32(out, hand.gun_index);
            opt_f32(out, hand.hit_distance_m);
            out.bool(hand.hit_object);
            out.vec3(hand.target);
        });
    }
    opt(out, aim.grapple.as_ref(), |out, grapple| {
        bools(out, [grapple.hit, grapple.valid, grapple.tetherable]);
        out.vec3(grapple.point);
    });
    opt(out, aim.weapon.as_ref(), |out, weapon| {
        out.u32(weapon.name_hash);
        for hand in weapon.gun_hands {
            opt_hand(out, hand);
        }
        label(out, &weapon.sight_source);
        opt_vec3(out, weapon.sight_offset_m);
        out.vec2(weapon.sight_angles_deg);
    });
    opt_label(out, &aim.raycast_error);
}

fn read_aim_state(input: &mut Decoder) -> Result<AimState, DecodeError> {
    let idle = read_opt_label(input, "the aim idle reason")?;
    let mut hands = [None, None];
    for hand in &mut hands {
        *hand = read_opt(input, "the hand aim flag", |input| {
            Ok(HandAim {
                source: read_label(input, "the aim source")?,
                gun_index: read_opt_u32(input, "the aim gun flag")?,
                hit_distance_m: read_opt_f32(input, "the aim hit flag")?,
                hit_object: input.bool("the aim object flag")?,
                target: input.vec3()?,
            })
        })?;
    }
    let grapple = read_opt(input, "the grapple aim flag", |input| {
        let [hit, valid, tetherable] = read_bools(input, "the grapple aim flags")?;
        Ok(GrappleAim {
            hit,
            valid,
            tetherable,
            point: input.vec3()?,
        })
    })?;
    let weapon = read_opt(input, "the weapon aim flag", |input| {
        let name_hash = input.u32()?;
        let mut gun_hands = [None; 4];
        for hand in &mut gun_hands {
            *hand = read_opt_hand(input, "the gun hand")?;
        }
        Ok(WeaponAim {
            name_hash,
            gun_hands,
            sight_source: read_label(input, "the sight source")?,
            sight_offset_m: read_opt_vec3(input, "the sight offset flag")?,
            sight_angles_deg: input.vec2()?,
        })
    })?;
    Ok(AimState {
        idle,
        hands,
        grapple,
        weapon,
        raycast_error: read_opt_label(input, "the raycast error")?,
    })
}

fn roomscale_state(out: &mut Encoder, room: &RoomscaleState) {
    bools(out, [room.active, room.crouched, room.seated_crouch]);
    label(out, &room.posture);
    out.f32(room.eye_drop_m);
    opt_label(out, &room.root_owner);
    out.f32(room.gap_m);
    out.f32(room.residual_m);
    out.f32(room.pushback_m);
    out.vec3(room.cockpit_shift_m);
    out.vec2(room.chase_velocity_mps);
    out.vec2(room.task_velocity_mps);
    label(out, &room.seat);
    opt_label(out, &room.probe);
    out.f32(room.fade_level);
}

fn read_roomscale_state(input: &mut Decoder) -> Result<RoomscaleState, DecodeError> {
    let [active, crouched, seated_crouch] = read_bools(input, "the roomscale flags")?;
    Ok(RoomscaleState {
        active,
        posture: read_label(input, "the posture")?,
        eye_drop_m: input.f32()?,
        root_owner: read_opt_label(input, "the root owner")?,
        gap_m: input.f32()?,
        residual_m: input.f32()?,
        pushback_m: input.f32()?,
        cockpit_shift_m: input.vec3()?,
        chase_velocity_mps: input.vec2()?,
        task_velocity_mps: input.vec2()?,
        crouched,
        seated_crouch,
        seat: read_label(input, "the seat")?,
        probe: read_opt_label(input, "the probe")?,
        fade_level: input.f32()?,
    })
}

fn grip_acquisition(out: &mut Encoder, grip: &GripAcquisition) {
    out.u8(hand_code(grip.hand));
    out.u8(grip_stage_code(grip.stage));
    out.u64(grip.sequence);
    out.u64(grip.observation);
    out.u64(grip.episode);
    opt_u64(out, grip.request);
    bools(out, [grip.grip_down, grip.grip_pressed, grip.grip_released]);
    label(out, &grip.latch);
    opt_label(out, &grip.grab);
    opt(out, grip.chosen.as_ref(), grip_candidate);
    list(out, &grip.candidates, grip_candidate);
    out.u32(grip.candidates_omitted);
    opt(out, grip.scan.as_ref(), grip_scan);
    label(out, &grip.consumer);
    opt_label(out, &grip.target);
    opt_label(out, &grip.result);
    opt(out, grip.drive.as_ref(), |out, drive| {
        out.u64(drive.object);
        out.u32(drive.body);
        label(out, &drive.before);
        opt_label(out, &drive.commanded);
        opt_label(out, &drive.result);
        opt_label(out, &drive.report);
    });
}

fn read_grip_acquisition(input: &mut Decoder) -> Result<GripAcquisition, DecodeError> {
    let hand = read_hand(input, "the grip hand")?;
    let stage = read_grip_stage(input)?;
    let sequence = input.u64()?;
    let observation = input.u64()?;
    let episode = input.u64()?;
    let request = read_opt_u64(input, "the grip request flag")?;
    let [grip_down, grip_pressed, grip_released] = read_bools(input, "the grip edges")?;
    Ok(GripAcquisition {
        hand,
        stage,
        sequence,
        observation,
        episode,
        request,
        grip_down,
        grip_pressed,
        grip_released,
        latch: read_label(input, "the grip latch")?,
        grab: read_opt_label(input, "the grab volume")?,
        chosen: read_opt(input, "the chosen candidate flag", read_grip_candidate)?,
        candidates: read_list(input, "the candidate count", read_grip_candidate)?,
        candidates_omitted: input.u32()?,
        scan: read_opt(input, "the scan flag", read_grip_scan)?,
        consumer: read_label(input, "the input consumer")?,
        target: read_opt_label(input, "the request target")?,
        result: read_opt_label(input, "the request result")?,
        drive: read_opt(input, "the drive flag", |input| {
            Ok(GripDrive {
                object: input.u64()?,
                body: input.u32()?,
                before: read_label(input, "the body state")?,
                commanded: read_opt_label(input, "the commanded velocities")?,
                result: read_opt_label(input, "the drive result")?,
                report: read_opt_label(input, "the held report")?,
            })
        })?,
    })
}

fn grip_stage_code(stage: GripStage) -> u8 {
    match stage {
        GripStage::Input => 0,
        GripStage::Request => 1,
        GripStage::Drive => 2,
    }
}

fn read_grip_stage(input: &mut Decoder) -> Result<GripStage, DecodeError> {
    let offset = input.offset();
    let code = input.u8()?;
    GripStage::ALL
        .into_iter()
        .find(|stage| grip_stage_code(*stage) == code)
        .ok_or_else(|| invalid(offset, "the grip stage", code.into()))
}

fn grip_candidate(out: &mut Encoder, candidate: &GripCandidate) {
    out.u32(candidate.resolver_index);
    out.f32(candidate.distance_m);
    label(out, &candidate.tier);
    label(out, &candidate.volume);
    opt_label(out, &candidate.observed);
    opt_label(out, &candidate.lost);
}

fn read_grip_candidate(input: &mut Decoder) -> Result<GripCandidate, DecodeError> {
    Ok(GripCandidate {
        resolver_index: input.u32()?,
        distance_m: input.f32()?,
        tier: read_label(input, "the candidate tier")?,
        volume: read_label(input, "the candidate volume")?,
        observed: read_opt_label(input, "the observed reach")?,
        lost: read_opt_label(input, "the loss reason")?,
    })
}

fn grip_scan(out: &mut Encoder, scan: &GripScan) {
    list(out, &scan.entries, |out, entry| {
        label(out, &entry.branch);
        opt_u64(out, entry.object);
        opt_u32(out, entry.body);
        opt_label(out, &entry.door);
        opt_vec3(out, entry.point);
        opt_f32(out, entry.distance_m);
        opt_f32(out, entry.mass_kg);
        label(out, &entry.result);
    });
    out.u32(scan.omitted);
    bools(out, [scan.ray_ran, scan.handles_ran]);
    opt_label(out, &scan.ray_selected);
    opt_label(out, &scan.handle_selected);
    opt_label(out, &scan.selected);
}

fn read_grip_scan(input: &mut Decoder) -> Result<GripScan, DecodeError> {
    let entries = read_list(input, "the scan entry count", |input| {
        Ok(GripScanEntry {
            branch: read_label(input, "the scan branch")?,
            object: read_opt_u64(input, "the scan object flag")?,
            body: read_opt_u32(input, "the scan body flag")?,
            door: read_opt_label(input, "the scan door")?,
            point: read_opt_vec3(input, "the scan point flag")?,
            distance_m: read_opt_f32(input, "the scan distance flag")?,
            mass_kg: read_opt_f32(input, "the scan mass flag")?,
            result: read_label(input, "the scan result")?,
        })
    })?;
    let omitted = input.u32()?;
    let [ray_ran, handles_ran] = read_bools(input, "the scan flags")?;
    Ok(GripScan {
        entries,
        omitted,
        ray_ran,
        handles_ran,
        ray_selected: read_opt_label(input, "the ray selection")?,
        handle_selected: read_opt_label(input, "the handle selection")?,
        selected: read_opt_label(input, "the scan selection")?,
    })
}

fn gun_release(out: &mut Encoder, release: &GunRelease) {
    out.u8(hand_code(release.hand));
    out.u64(release.attempt);
    opt_u64(out, release.weapon);
    label(out, &release.route);
    out.f32(release.max_speed_mps);
    opt_label(out, &release.refusal);
    bools(
        out,
        [
            release.before_disarm,
            release.disarm_returned,
            release.slot_clear_returned,
            release.reconcile_returned,
        ],
    );
    opt(out, release.controller.as_ref(), |out, controller| {
        out.u64(controller.frame_sequence);
        out.i64(controller.display_time_ns);
        out.u64(controller.publication_age_ns);
    });
    opt(out, release.drop.as_ref(), |out, drop| {
        out.u64(drop.weapon);
        opt_label(out, &drop.slot);
        label(out, &drop.sample_stage);
        label(out, &drop.sample_source);
        opt(out, drop.cached_motion.as_ref(), world_motion);
        opt_u32(out, drop.native_body);
        opt(out, drop.requested_motion.as_ref(), world_motion);
        label(out, &drop.release_source);
        opt_f32(out, drop.history_age_s);
        label(out, &drop.write);
        bools(out, [drop.on_drop_returned, drop.slot_clear_returned]);
    });
}

fn read_gun_release(input: &mut Decoder) -> Result<GunRelease, DecodeError> {
    let hand = read_hand(input, "the release hand")?;
    let attempt = input.u64()?;
    let weapon = read_opt_u64(input, "the released weapon flag")?;
    let route = read_label(input, "the release route")?;
    let max_speed_mps = input.f32()?;
    let refusal = read_opt_label(input, "the release refusal")?;
    let [
        before_disarm,
        disarm_returned,
        slot_clear_returned,
        reconcile_returned,
    ] = read_bools(input, "the release stages")?;
    let controller = read_opt(input, "the release controller flag", |input| {
        Ok(ReleaseController {
            frame_sequence: input.u64()?,
            display_time_ns: input.i64()?,
            publication_age_ns: input.u64()?,
        })
    })?;
    let drop = read_opt(input, "the release drop flag", |input| {
        let weapon = input.u64()?;
        let slot = read_opt_label(input, "the weapon slot")?;
        let sample_stage = read_label(input, "the sample stage")?;
        let sample_source = read_label(input, "the sample source")?;
        let cached_motion = read_opt(input, "the cached motion flag", read_world_motion)?;
        let native_body = read_opt_u32(input, "the native body flag")?;
        let requested_motion = read_opt(input, "the requested motion flag", read_world_motion)?;
        let release_source = read_label(input, "the release source")?;
        let history_age_s = read_opt_f32(input, "the history age flag")?;
        let write = read_label(input, "the native write")?;
        let [on_drop_returned, slot_clear_returned] = read_bools(input, "the drop returns")?;
        Ok(ReleaseDrop {
            weapon,
            slot,
            sample_stage,
            sample_source,
            cached_motion,
            native_body,
            requested_motion,
            release_source,
            history_age_s,
            write,
            on_drop_returned,
            slot_clear_returned,
        })
    })?;
    Ok(GunRelease {
        hand,
        attempt,
        weapon,
        route,
        max_speed_mps,
        refusal,
        before_disarm,
        disarm_returned,
        slot_clear_returned,
        reconcile_returned,
        controller,
        drop,
    })
}

fn world_motion(out: &mut Encoder, motion: &WorldMotion) {
    out.vec3(motion.linear);
    out.vec3(motion.angular);
}

fn read_world_motion(input: &mut Decoder) -> Result<WorldMotion, DecodeError> {
    Ok(WorldMotion {
        linear: input.vec3()?,
        angular: input.vec3()?,
    })
}

fn take_marker(out: &mut Encoder, marker: &TakeMarker) {
    out.u32(marker.index);
    label(out, &marker.stamp);
    out.i64(marker.unix_ms);
    opt_u64(out, marker.game_run_time_us);
    label(out, &marker.label);
}

fn read_take_marker(input: &mut Decoder) -> Result<TakeMarker, DecodeError> {
    Ok(TakeMarker {
        index: input.u32()?,
        stamp: read_label(input, "the marker stamp")?,
        unix_ms: input.i64()?,
        game_run_time_us: read_opt_u64(input, "the marker game time flag")?,
        label: read_label(input, "the marker label")?,
    })
}
