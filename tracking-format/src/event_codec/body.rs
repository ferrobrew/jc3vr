//! The encoding of the body's version 8 kinds: the pose samples and the grounded rig.
//!
//! A pose sample's bones are a `u32` mask of the bones present, then each present bone's
//! model-space position as three `i16`s of a tenth of a millimetre and its rotation as its smallest
//! three components ([`crate::quant`]).

use glam::Vec2;

use crate::{
    event::{
        BoneSet, IkGoal, IkTarget, PoseSample, PoseStage, RigConfig, RigInput, RigResult, RigSample,
    },
    event_codec::{
        invalid, label, list, opt, opt_label, read_label, read_list, read_opt, read_opt_label,
    },
    model::Pose,
    quant::{self, BONE_POSITION_STEP},
    wire::{DecodeError, Decoder, Encoder},
};

pub(super) fn pose(out: &mut Encoder, pose: &PoseSample) {
    out.u64(pose.pass);
    out.u8(match pose.stage {
        PoseStage::PreIk => 0,
        PoseStage::Final => 1,
    });
    out.pose(&pose.character);
    out.u8(match pose.set {
        BoneSet::Body => 1,
    });
    let present =
        |bone: &Option<Pose>| bone.filter(|b| quant::fits(b.position, BONE_POSITION_STEP));
    let mut mask = 0u32;
    for (index, bone) in pose.bones.iter().enumerate().take(32) {
        if present(bone).is_some() {
            mask |= 1 << index;
        }
    }
    out.u32(mask);
    for bone in pose.bones.iter().take(32).filter_map(present) {
        quant::fixed3(out, bone.position, BONE_POSITION_STEP);
        quant::rotation(out, bone.orientation);
    }
    list(out, &pose.targets, ik_target);
}

pub(super) fn read_pose(input: &mut Decoder) -> Result<PoseSample, DecodeError> {
    let pass = input.u64()?;
    let offset = input.offset();
    let stage = match input.u8()? {
        0 => PoseStage::PreIk,
        1 => PoseStage::Final,
        value => return Err(invalid(offset, "the pose stage", value.into())),
    };
    let character = input.pose()?;
    let offset = input.offset();
    let set = match input.u8()? {
        1 => BoneSet::Body,
        value => return Err(invalid(offset, "the bone set", value.into())),
    };
    let count = set.names().len();
    let offset = input.offset();
    let mask = input.u32()?;
    if count < 32 && mask >> count != 0 {
        return Err(invalid(offset, "the bone mask", mask.into()));
    }
    let bones = (0..count)
        .map(|index| {
            if mask & (1 << index) == 0 {
                return Ok(None);
            }
            Ok(Some(Pose {
                position: quant::read_fixed3(input, BONE_POSITION_STEP)?,
                orientation: quant::read_rotation(input)?,
            }))
        })
        .collect::<Result<_, DecodeError>>()?;
    Ok(PoseSample {
        pass,
        stage,
        character,
        set,
        bones,
        targets: read_list(input, "the IK target count", read_ik_target)?,
    })
}

fn ik_target(out: &mut Encoder, target: &IkTarget) {
    out.i32(target.effector);
    match &target.goal {
        IkGoal::Position(position) => {
            out.u8(0);
            out.vec3(*position);
        }
        IkGoal::Rotation { axis, angle_rad } => {
            out.u8(1);
            out.vec3(*axis);
            out.f32(*angle_rad);
        }
    }
    out.f32(target.reach);
    out.f32(target.pull);
}

fn read_ik_target(input: &mut Decoder) -> Result<IkTarget, DecodeError> {
    let effector = input.i32()?;
    let offset = input.offset();
    let goal = match input.u8()? {
        0 => IkGoal::Position(input.vec3()?),
        1 => IkGoal::Rotation {
            axis: input.vec3()?,
            angle_rad: input.f32()?,
        },
        value => return Err(invalid(offset, "the IK goal", value.into())),
    };
    Ok(IkTarget {
        effector,
        goal,
        reach: input.f32()?,
        pull: input.f32()?,
    })
}

pub(super) fn rig(out: &mut Encoder, rig: &RigSample) {
    let input = &rig.input;
    out.vec3(input.eyes);
    out.quat(input.head);
    for hand in &input.hands {
        opt(out, hand.as_ref(), |out, p| out.vec3(*p));
    }
    out.vec2(input.pelvis);
    super::bools(out, [input.moving, input.sample_height]);
    out.f32(input.dt_s);
    let o = &rig.output;
    out.vec3(o.neck);
    out.vec3(o.waist);
    out.f32(o.spine_m);
    out.vec2(o.offset);
    out.vec2(o.bend);
    out.f32(o.lean_reach_m);
    out.vec2(o.step_demand);
    out.f32(o.crouch_m);
    super::bools(out, [o.hands_reaching, o.turning]);
    label(out, &o.phase);
    out.vec2(o.gap);
    out.f32(o.body_yaw);
    out.f32(o.chest_yaw);
    out.f32(o.yaw_target);
    opt_label(out, &o.turn_cause);
    out.f32(o.standing_neck_m);
    out.u64(o.steps);
    out.f32(rig.floor_m);
    opt(out, rig.snapped_to.as_ref(), |out, p| out.vec2(*p));
}

pub(super) fn read_rig(input: &mut Decoder) -> Result<RigSample, DecodeError> {
    let eyes = input.vec3()?;
    let head = input.quat()?;
    let hands = [
        read_opt(input, "the rig's left hand flag", |input| input.vec3())?,
        read_opt(input, "the rig's right hand flag", |input| input.vec3())?,
    ];
    let pelvis = input.vec2()?;
    let [moving, sample_height] = super::read_bools(input, "the rig's input flags")?;
    let dt_s = input.f32()?;
    let neck = input.vec3()?;
    let waist = input.vec3()?;
    let spine_m = input.f32()?;
    let offset = input.vec2()?;
    let bend = input.vec2()?;
    let lean_reach_m = input.f32()?;
    let step_demand = input.vec2()?;
    let crouch_m = input.f32()?;
    let [hands_reaching, turning] = super::read_bools(input, "the rig's output flags")?;
    Ok(RigSample {
        input: RigInput {
            eyes,
            head,
            hands,
            pelvis,
            moving,
            sample_height,
            dt_s,
        },
        output: RigResult {
            neck,
            waist,
            spine_m,
            offset,
            bend,
            lean_reach_m,
            step_demand,
            crouch_m,
            hands_reaching,
            phase: read_label(input, "the step phase")?,
            gap: input.vec2()?,
            body_yaw: input.f32()?,
            chest_yaw: input.f32()?,
            yaw_target: input.f32()?,
            turning,
            turn_cause: read_opt_label(input, "the turn cause")?,
            standing_neck_m: input.f32()?,
            steps: input.u64()?,
        },
        floor_m: input.f32()?,
        snapped_to: read_opt(input, "the rig's snap flag", |input| -> Result<Vec2, _> {
            input.vec2()
        })?,
    })
}

pub(super) fn rig_config(out: &mut Encoder, config: &RigConfig) {
    out.f32(config.height_m);
    out.f32(config.eye_height_m);
    out.f32(config.body_eye_height_m);
    out.f32(config.body_hip_height_m);
    out.long_str(&config.tuning_json);
    out.bool(config.repeat);
}

pub(super) fn read_rig_config(input: &mut Decoder) -> Result<RigConfig, DecodeError> {
    Ok(RigConfig {
        height_m: input.f32()?,
        eye_height_m: input.f32()?,
        body_eye_height_m: input.f32()?,
        body_hip_height_m: input.f32()?,
        tuning_json: input.long_str("the rig tuning")?.into(),
        repeat: input.bool("the rig config repeat flag")?,
    })
}
