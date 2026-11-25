//! A script: what a synthetic player does over time, as keyframed tracks.
//!
//! The body's tracks describe the player as a body rather than as three tracked devices: where they
//! stand, which way they face, how far they lean and squat, and where they look. The rig
//! ([`crate::rig`]) turns them into the head and controller poses a headset would report, so the
//! hands hang and swing with the body without a script having to keyframe them. A script that wants
//! a hand somewhere else keyframes the hand's reach on top.

use crate::track::Track;

/// What a synthetic player does.
#[derive(Debug, Clone)]
pub struct Script {
    /// The name the tool and the scenarios know it by, and the recording's file stem.
    pub name: &'static str,
    /// What the player does, for the tool's listing.
    pub summary: &'static str,
    pub body: BodyTracks,
    /// The controllers' reach and inputs, indexed left, right.
    pub hands: [HandTracks; 2],
}

impl Script {
    /// How long the recording runs: until the last key of any track, and then a second of rest.
    pub fn duration_s(&self) -> f32 {
        let body = &self.body;
        let tracks = [
            &body.x_m,
            &body.z_m,
            &body.yaw_deg,
            &body.crouch_m,
            &body.lean_deg,
            &body.head_yaw_deg,
            &body.head_pitch_deg,
            &body.sway,
        ];
        let hands = self.hands.iter().flat_map(|hand| {
            [
                &hand.reach_x_m,
                &hand.reach_y_m,
                &hand.reach_z_m,
                &hand.stick_x,
                &hand.stick_y,
                &hand.trigger,
                &hand.squeeze,
                &hand.squeeze_force,
            ]
        });
        let presses = self
            .hands
            .iter()
            .flat_map(|hand| hand.presses.iter().map(|press| press.until_s));
        tracks
            .into_iter()
            .chain(hands)
            .map(Track::end)
            .chain(presses)
            .fold(0.0, f32::max)
            + TAIL_S
    }
}

/// The body's tracks. Distances are metres and angles degrees, in the cockpit frame's axes (+X
/// right, +Y up, -Z forward) as the player stood at the start.
#[derive(Debug, Clone)]
pub struct BodyTracks {
    /// The body's vertical axis across the floor, from where it started.
    pub x_m: Track,
    pub z_m: Track,
    /// The heading of the body, turning left (counter-clockwise from above) positive.
    pub yaw_deg: Track,
    /// How far the hips drop, as in a squat.
    pub crouch_m: Track,
    /// How far the torso pitches forward about the hips.
    pub lean_deg: Track,
    /// The head's turn on the neck from the torso's heading, left positive.
    pub head_yaw_deg: Track,
    /// The head's pitch on the neck from the torso's, up positive.
    pub head_pitch_deg: Track,
    /// The standing sway's amplitude, as a share of a relaxed stance's.
    pub sway: Track,
}

impl Default for BodyTracks {
    fn default() -> Self {
        BodyTracks {
            x_m: Track::start(0.0),
            z_m: Track::start(0.0),
            yaw_deg: Track::start(0.0),
            crouch_m: Track::start(0.0),
            lean_deg: Track::start(0.0),
            head_yaw_deg: Track::start(0.0),
            head_pitch_deg: Track::start(0.0),
            sway: Track::start(1.0),
        }
    }
}

/// One controller's tracks.
#[derive(Debug, Clone)]
pub struct HandTracks {
    /// Where the grip is moved from where the arm would hang, metres, in the body's frame (+X to
    /// the body's right, +Y up, -Z ahead of the body).
    pub reach_x_m: Track,
    pub reach_y_m: Track,
    pub reach_z_m: Track,
    /// The thumbstick, `-1..=1` each, right and forward positive.
    pub stick_x: Track,
    pub stick_y: Track,
    pub trigger: Track,
    /// How far the fingers close round the handle. A hand holding the controller reads near the
    /// top on the Index's capacitive grip.
    pub squeeze: Track,
    /// How hard the grip is squeezed.
    pub squeeze_force: Track,
    /// The buttons held, and when.
    pub presses: Vec<Press>,
}

impl Default for HandTracks {
    fn default() -> Self {
        HandTracks {
            reach_x_m: Track::start(0.0),
            reach_y_m: Track::start(0.0),
            reach_z_m: Track::start(0.0),
            stick_x: Track::start(0.0),
            stick_y: Track::start(0.0),
            trigger: Track::start(0.0),
            squeeze: Track::start(RESTING_SQUEEZE),
            squeeze_force: Track::start(0.0),
            presses: Vec::new(),
        }
    }
}

/// A button held from one time to another.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Press {
    pub button: PressButton,
    pub from_s: f32,
    pub until_s: f32,
}

/// The buttons a script can press. The touches are derived: a pressed button is touched.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PressButton {
    A,
    B,
    ThumbstickClick,
    System,
}

/// The squeeze value of a hand resting round an Index controller's handle.
pub const RESTING_SQUEEZE: f32 = 0.9;

/// The rest after the last key, so a scenario sees the final pose settle.
const TAIL_S: f32 = 1.0;
