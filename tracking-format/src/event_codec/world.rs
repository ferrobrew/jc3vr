//! The encoding of the world's version 8 records: the controller state's view, grapple, vehicle,
//! and frame groups, and the bodies, intro, held, and force kinds.
//!
//! A body sample is its id, a flag byte (bit 0 at rest, bits 1 and 2 held by the left and right
//! hand), its offset from the sample's origin as three `i16` millimetres, its rotation as its
//! smallest three components, and its velocities as three `i16`s each ([`crate::quant`]): 29
//! bytes.

use crate::{
    codec::{hand_code, read_hand},
    event::{
        BodiesSample, BodyIntro, BodySample, CameraState, Force, ForceKind, FrameState,
        GrappleState, HeldMotion, HeldSample, HoldReport, SlingState, VehicleState, ViewState,
        WireEndSample,
    },
    event_codec::{
        bools, invalid, label, list, opt, opt_f32, opt_label, opt_u32, opt_u64, opt_vec3,
        read_bools, read_label, read_list, read_opt, read_opt_f32, read_opt_label, read_opt_u32,
        read_opt_u64, read_opt_vec3, read_world_motion, world_motion,
    },
    quant::{self, ANGULAR_VELOCITY_STEP, BODY_POSITION_STEP, LINEAR_VELOCITY_STEP},
    wire::{DecodeError, Decoder, Encoder},
};

pub(super) fn view_state(out: &mut Encoder, view: &ViewState) {
    out.pose(&view.head_world);
    out.vec3(view.head_cockpit);
    out.quat(view.view);
    out.f32(view.world_scale);
    opt(out, view.camera.as_ref(), |out, camera| {
        out.pose(&camera.previous);
        out.pose(&camera.current);
        out.f32(camera.fraction);
        out.pose(&camera.rendered);
    });
}

pub(super) fn read_view_state(input: &mut Decoder) -> Result<ViewState, DecodeError> {
    Ok(ViewState {
        head_world: input.pose()?,
        head_cockpit: input.vec3()?,
        view: input.quat()?,
        world_scale: input.f32()?,
        camera: read_opt(input, "the camera flag", |input| {
            Ok(CameraState {
                previous: input.pose()?,
                current: input.pose()?,
                fraction: input.f32()?,
                rendered: input.pose()?,
            })
        })?,
    })
}

pub(super) fn grapple_state(out: &mut Encoder, grapple: &GrappleState) {
    label(out, &grapple.hook_state);
    label(out, &grapple.wire_state);
    out.f32(grapple.max_length_m);
    wire_end(out, &grapple.hook);
    wire_end(out, &grapple.device);
    opt_u64(out, grapple.target_object);
    opt(out, grapple.sling.as_ref(), |out, sling| {
        out.f32(sling.length_m);
        out.f32(sling.tension_n);
        out.f32(sling.max_tension_n);
        out.f32(sling.mass_kg);
        bools(out, [sling.braced, sling.tethered]);
        out.vec3(sling.anchor);
        out.vec3(sling.centre);
    });
}

pub(super) fn read_grapple_state(input: &mut Decoder) -> Result<GrappleState, DecodeError> {
    Ok(GrappleState {
        hook_state: read_label(input, "the hook state")?,
        wire_state: read_label(input, "the wire state")?,
        max_length_m: input.f32()?,
        hook: read_wire_end(input)?,
        device: read_wire_end(input)?,
        target_object: read_opt_u64(input, "the hook target flag")?,
        sling: read_opt(input, "the sling flag", |input| {
            let length_m = input.f32()?;
            let tension_n = input.f32()?;
            let max_tension_n = input.f32()?;
            let mass_kg = input.f32()?;
            let [braced, tethered] = read_bools(input, "the sling flags")?;
            Ok(SlingState {
                length_m,
                tension_n,
                max_tension_n,
                mass_kg,
                braced,
                tethered,
                anchor: input.vec3()?,
                centre: input.vec3()?,
            })
        })?,
    })
}

fn wire_end(out: &mut Encoder, end: &WireEndSample) {
    label(out, &end.state);
    out.vec3(end.position);
    out.bool(end.attached);
}

fn read_wire_end(input: &mut Decoder) -> Result<WireEndSample, DecodeError> {
    Ok(WireEndSample {
        state: read_label(input, "the wire end state")?,
        position: input.vec3()?,
        attached: input.bool("the wire end's attachment")?,
    })
}

pub(super) fn vehicle_state(out: &mut Encoder, vehicle: &VehicleState) {
    label(out, &vehicle.class);
    opt_u64(out, vehicle.object);
    opt(out, vehicle.transform.as_ref(), |out, p| out.pose(p));
    opt(out, vehicle.rendered.as_ref(), |out, p| out.pose(p));
    opt_vec3(out, vehicle.step_m);
    opt_vec3(out, vehicle.steering);
    bools(
        out,
        [
            vehicle.driver,
            vehicle.handling,
            vehicle.mounted,
            vehicle.mounted_holds[0],
            vehicle.mounted_holds[1],
        ],
    );
    for hold in &vehicle.holds {
        opt_label(out, hold);
    }
    opt_vec3(out, vehicle.mounted_aim);
}

pub(super) fn read_vehicle_state(input: &mut Decoder) -> Result<VehicleState, DecodeError> {
    let class = read_label(input, "the vehicle class")?;
    let object = read_opt_u64(input, "the vehicle object flag")?;
    let transform = read_opt(input, "the vehicle transform flag", |input| input.pose())?;
    let rendered = read_opt(input, "the vehicle render flag", |input| input.pose())?;
    let step_m = read_opt_vec3(input, "the vehicle step flag")?;
    let steering = read_opt_vec3(input, "the vehicle steering flag")?;
    let [driver, handling, mounted, left, right] = read_bools(input, "the vehicle flags")?;
    Ok(VehicleState {
        class,
        driver,
        object,
        transform,
        rendered,
        step_m,
        steering,
        handling,
        holds: [
            read_opt_label(input, "the left hand's vehicle control")?,
            read_opt_label(input, "the right hand's vehicle control")?,
        ],
        mounted,
        mounted_holds: [left, right],
        mounted_aim: read_opt_vec3(input, "the mounted aim flag")?,
    })
}

pub(super) fn frame_state(out: &mut Encoder, frame: &FrameState) {
    opt_u32(out, frame.render_frame);
    opt_u64(out, frame.mirror_frame);
    opt_f32(out, frame.interval_ms);
    out.u32(frame.ticks);
    for haptic in frame.haptics {
        opt_f32(out, haptic);
    }
}

pub(super) fn read_frame_state(input: &mut Decoder) -> Result<FrameState, DecodeError> {
    Ok(FrameState {
        render_frame: read_opt_u32(input, "the render frame flag")?,
        mirror_frame: read_opt_u64(input, "the mirror frame flag")?,
        interval_ms: read_opt_f32(input, "the frame interval flag")?,
        ticks: input.u32()?,
        haptics: [
            read_opt_f32(input, "the left haptic flag")?,
            read_opt_f32(input, "the right haptic flag")?,
        ],
    })
}

pub(super) fn bodies(out: &mut Encoder, bodies: &BodiesSample) {
    out.u64(bodies.step);
    out.vec3(bodies.origin);
    out.u32(bodies.walked);
    out.u32(bodies.peak);
    out.u32(bodies.cost_us);
    list(out, &bodies.bodies, body_sample);
}

pub(super) fn read_bodies(input: &mut Decoder) -> Result<BodiesSample, DecodeError> {
    Ok(BodiesSample {
        step: input.u64()?,
        origin: input.vec3()?,
        walked: input.u32()?,
        peak: input.u32()?,
        cost_us: input.u32()?,
        bodies: read_list(input, "the body count", read_body_sample)?,
    })
}

fn body_sample(out: &mut Encoder, body: &BodySample) {
    out.u32(body.id);
    bools(out, [body.resting, body.held[0], body.held[1]]);
    quant::fixed3(out, body.offset, BODY_POSITION_STEP);
    quant::rotation(out, body.orientation);
    quant::fixed3(out, body.linear, LINEAR_VELOCITY_STEP);
    quant::fixed3(out, body.angular, ANGULAR_VELOCITY_STEP);
}

fn read_body_sample(input: &mut Decoder) -> Result<BodySample, DecodeError> {
    let id = input.u32()?;
    let [resting, left, right] = read_bools(input, "the body flags")?;
    Ok(BodySample {
        id,
        offset: quant::read_fixed3(input, BODY_POSITION_STEP)?,
        orientation: quant::read_rotation(input)?,
        linear: quant::read_fixed3(input, LINEAR_VELOCITY_STEP)?,
        angular: quant::read_fixed3(input, ANGULAR_VELOCITY_STEP)?,
        resting,
        held: [left, right],
    })
}

pub(super) fn body_intro(out: &mut Encoder, intro: &BodyIntro) {
    out.u32(intro.id);
    opt_u64(out, intro.object);
    label(out, &intro.kind);
    opt_f32(out, intro.mass_kg);
    label(out, &intro.shape);
    out.vec3(intro.half_extents);
    out.u32(intro.filter);
    out.bool(intro.repeat);
}

pub(super) fn read_body_intro(input: &mut Decoder) -> Result<BodyIntro, DecodeError> {
    Ok(BodyIntro {
        id: input.u32()?,
        object: read_opt_u64(input, "the body owner flag")?,
        kind: read_label(input, "the body kind")?,
        mass_kg: read_opt_f32(input, "the body mass flag")?,
        shape: read_label(input, "the body shape")?,
        half_extents: input.vec3()?,
        filter: input.u32()?,
        repeat: input.bool("the body intro repeat flag")?,
    })
}

pub(super) fn held(out: &mut Encoder, held: &HeldSample) {
    out.u8(hand_code(held.hand));
    label(out, &held.kind);
    out.u64(held.object);
    out.u32(held.body);
    opt(out, held.motion.as_ref(), |out, motion| {
        out.pose(&motion.pose);
        out.vec3(motion.linear);
        out.vec3(motion.angular);
    });
    opt(out, held.report.as_ref(), |out, report| {
        out.f32(report.mass_kg);
        out.vec3(report.centre);
        out.vec3(report.anchor);
        out.vec3(report.target);
        out.f32(report.lag_m);
        out.f32(report.load);
        out.f32(report.twist);
        bools(out, [report.joined, report.shared, report.liftable]);
    });
}

pub(super) fn read_held(input: &mut Decoder) -> Result<HeldSample, DecodeError> {
    Ok(HeldSample {
        hand: read_hand(input, "the holding hand")?,
        kind: read_label(input, "the hold kind")?,
        object: input.u64()?,
        body: input.u32()?,
        motion: read_opt(input, "the held motion flag", |input| {
            Ok(HeldMotion {
                pose: input.pose()?,
                linear: input.vec3()?,
                angular: input.vec3()?,
            })
        })?,
        report: read_opt(input, "the hold report flag", |input| {
            let mass_kg = input.f32()?;
            let centre = input.vec3()?;
            let anchor = input.vec3()?;
            let target = input.vec3()?;
            let lag_m = input.f32()?;
            let load = input.f32()?;
            let twist = input.f32()?;
            let [joined, shared, liftable] = read_bools(input, "the hold report flags")?;
            Ok(HoldReport {
                mass_kg,
                centre,
                anchor,
                target,
                lag_m,
                load,
                twist,
                joined,
                shared,
                liftable,
            })
        })?,
    })
}

pub(super) fn force(out: &mut Encoder, force: &Force) {
    label(out, &force.source);
    out.u32(force.line);
    match &force.kind {
        ForceKind::Velocity { linear, angular } => {
            out.u8(0);
            out.vec3(*linear);
            out.vec3(*angular);
        }
        ForceKind::Impulse { impulse, at } => {
            out.u8(1);
            out.vec3(*impulse);
            out.vec3(*at);
        }
    }
    out.u32(force.body);
    opt_u64(out, force.object);
    opt_f32(out, force.mass_kg);
    opt(out, force.before.as_ref(), world_motion);
}

pub(super) fn read_force(input: &mut Decoder) -> Result<Force, DecodeError> {
    let source = read_label(input, "the force source")?;
    let line = input.u32()?;
    let offset = input.offset();
    let kind = match input.u8()? {
        0 => ForceKind::Velocity {
            linear: input.vec3()?,
            angular: input.vec3()?,
        },
        1 => ForceKind::Impulse {
            impulse: input.vec3()?,
            at: input.vec3()?,
        },
        value => return Err(invalid(offset, "the force kind", value.into())),
    };
    Ok(Force {
        source,
        line,
        kind,
        body: input.u32()?,
        object: read_opt_u64(input, "the force owner flag")?,
        mass_kg: read_opt_f32(input, "the force mass flag")?,
        before: read_opt(input, "the force's prior motion flag", read_world_motion)?,
    })
}
