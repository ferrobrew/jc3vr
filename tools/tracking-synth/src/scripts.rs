//! The built-in scripts, each the recording of one scenario in `testing/scenarios/`.
//!
//! Every script starts and ends at rest, standing where it started, and holds still for a moment
//! first so a replay sees the stance before anything moves.

use std::mem::replace;

use crate::{
    rig,
    script::{BodyTracks, HandTracks, Script},
    track::Track,
};

/// Every built-in script, in the order the tool lists them.
pub fn all() -> Vec<Script> {
    vec![
        stand_still(),
        walk_room_forward(),
        walk_room_square(),
        lean_forward(),
        crouch(),
        turn_in_place(),
        stick_walk(),
        stick_strafe_and_turn(),
        look_around(),
    ]
}

/// The built-in script named `name`.
pub fn named(name: &str) -> Option<Script> {
    all().into_iter().find(|script| script.name == name)
}

fn stand_still() -> Script {
    Script {
        name: "stand_still",
        summary: "Stand still for twelve seconds with a relaxed stance's sway.",
        body: BodyTracks {
            sway: Track::start(1.0).hold(12.0),
            ..BodyTracks::default()
        },
        hands: Default::default(),
    }
}

fn walk_room_forward() -> Script {
    let mut walk = Walk::new();
    walk.pause(1.0);
    walk.walk_to(2.2, 0.0, -ROOM_WALK_M);
    walk.pause(2.0);
    // Back the way it came without turning round, a little slower, as one steps backwards.
    walk.walk_to(2.8, 0.0, 0.0);
    script(
        "walk_room_forward",
        "Walk 1.5 m forward across the room, pause, and step back to the start.",
        walk.body(),
    )
}

fn walk_room_square() -> Script {
    let mut walk = Walk::new();
    walk.pause(1.0);
    let corners = [
        (0.0, -ROOM_WALK_M),
        (ROOM_WALK_M, -ROOM_WALK_M),
        (ROOM_WALK_M, 0.0),
        (0.0, 0.0),
    ];
    for (corner, (x, z)) in corners.into_iter().enumerate() {
        walk.walk_to(2.2, x, z);
        walk.pause(0.3);
        // A right turn at each corner, so the square goes clockwise seen from above.
        walk.turn_to(1.2, -90.0 * (corner as f32 + 1.0));
        walk.pause(0.3);
    }
    script(
        "walk_room_square",
        "Walk a 1.5 m square clockwise, turning the body right at each corner.",
        walk.body(),
    )
}

fn lean_forward() -> Script {
    let (lean, crouch) = rig::hip_hinge(0.35, 0.20);
    let pose = |value: f32| {
        Track::start(0.0)
            .hold(1.5)
            .ease(1.5, value)
            .hold(3.0)
            .ease(1.5, 0.0)
    };
    script(
        "lean_forward",
        "Lean from the hips over a table, the head 35 cm forward and 20 cm down, then return.",
        BodyTracks {
            lean_deg: pose(lean),
            crouch_m: pose(crouch),
            ..BodyTracks::default()
        },
    )
}

fn crouch() -> Script {
    let pose = |value: f32| {
        Track::start(0.0)
            .hold(1.5)
            .ease(1.4, value)
            .hold(2.0)
            .ease(1.4, 0.0)
    };
    script(
        "crouch",
        "Squat down by 45 cm at the hips, leaning a little to balance, and stand up.",
        BodyTracks {
            crouch_m: pose(0.45),
            lean_deg: pose(15.0),
            head_pitch_deg: pose(5.0),
            ..BodyTracks::default()
        },
    )
}

fn turn_in_place() -> Script {
    script(
        "turn_in_place",
        "Turn the whole body 90 degrees left on the spot, hold, and turn back.",
        BodyTracks {
            yaw_deg: Track::start(0.0)
                .hold(1.0)
                .ease(1.6, 90.0)
                .hold(2.0)
                .ease(1.6, 0.0),
            // The head leads each turn and the body catches up.
            head_yaw_deg: Track::start(0.0)
                .hold(1.0)
                .ease(0.4, 15.0)
                .ease(1.2, 0.0)
                .hold(2.0)
                .ease(0.4, -15.0)
                .ease(1.2, 0.0),
            ..BodyTracks::default()
        },
    )
}

fn stick_walk() -> Script {
    let left = HandTracks {
        stick_y: Track::start(0.0)
            .hold(1.5)
            .ease(STICK_RAMP_S, 1.0)
            .hold(3.0)
            .ease(STICK_RAMP_S, 0.5)
            .hold(3.0)
            .ease(STICK_RAMP_S, 0.0)
            .hold(1.5),
        ..HandTracks::default()
    };
    Script {
        name: "stick_walk",
        summary: "Stand still and push the left stick forward: 3 s at full, 3 s at half, release.",
        body: BodyTracks::default(),
        hands: [left, HandTracks::default()],
    }
}

fn stick_strafe_and_turn() -> Script {
    let left = HandTracks {
        stick_x: Track::start(0.0)
            .hold(1.0)
            .ease(STICK_RAMP_S, 1.0)
            .hold(2.0)
            .ease(2.0 * STICK_RAMP_S, -1.0)
            .hold(2.0)
            .ease(STICK_RAMP_S, 0.0),
        ..HandTracks::default()
    };
    // Two flicks right, which snap turn under the snap scheme, then a held push left, which turns
    // smoothly under the smooth scheme (and snaps repeatedly under the snap one).
    let flick = |track: Track| track.snap(1.0).hold(0.15).snap(0.0).hold(0.8);
    let right = HandTracks {
        stick_x: flick(flick(Track::start(0.0).hold(6.5)))
            .ease(0.2, -0.7)
            .hold(2.0)
            .ease(0.2, 0.0)
            .hold(1.0),
        ..HandTracks::default()
    };
    Script {
        name: "stick_strafe_and_turn",
        summary: "Strafe right then left with the left stick, flick the right stick right twice, \
                  then hold it left.",
        body: BodyTracks::default(),
        hands: [left, right],
    }
}

fn look_around() -> Script {
    let yaw = Track::start(0.0)
        .hold(1.0)
        .ease(1.5, 70.0)
        .hold(1.0)
        .ease(2.5, -70.0)
        .hold(1.0)
        .ease(1.5, 0.0);
    let pitch = Track::start(0.0)
        .hold(yaw.end() + 0.5)
        .ease(1.5, -60.0)
        .hold(1.5)
        .ease(1.5, 0.0);
    script(
        "look_around",
        "Turn the head 70 degrees left and right, then look down at the floor, the torso still.",
        BodyTracks {
            head_yaw_deg: yaw,
            head_pitch_deg: pitch,
            ..BodyTracks::default()
        },
    )
}

/// A script that moves only the body, the hands hanging with it.
fn script(name: &'static str, summary: &'static str, body: BodyTracks) -> Script {
    Script {
        name,
        summary,
        body,
        hands: Default::default(),
    }
}

/// How far the room-scale walks go along each side.
const ROOM_WALK_M: f32 = 1.5;

/// How long a stick takes to reach a new deflection.
const STICK_RAMP_S: f32 = 0.15;

/// The body's position and heading, keyed together so that each move holds what does not move.
struct Walk {
    x: Track,
    z: Track,
    yaw: Track,
}

impl Walk {
    fn new() -> Walk {
        Walk {
            x: Track::start(0.0),
            z: Track::start(0.0),
            yaw: Track::start(0.0),
        }
    }

    fn pause(&mut self, seconds: f32) {
        self.key(
            |x| x.hold(seconds),
            |z| z.hold(seconds),
            |yaw| yaw.hold(seconds),
        );
    }

    fn walk_to(&mut self, seconds: f32, x: f32, z: f32) {
        self.key(
            |track| track.ease(seconds, x),
            |track| track.ease(seconds, z),
            |yaw| yaw.hold(seconds),
        );
    }

    fn turn_to(&mut self, seconds: f32, yaw_deg: f32) {
        self.key(
            |x| x.hold(seconds),
            |z| z.hold(seconds),
            |yaw| yaw.ease(seconds, yaw_deg),
        );
    }

    fn key(
        &mut self,
        x: impl FnOnce(Track) -> Track,
        z: impl FnOnce(Track) -> Track,
        yaw: impl FnOnce(Track) -> Track,
    ) {
        self.x = x(replace(&mut self.x, Track::start(0.0)));
        self.z = z(replace(&mut self.z, Track::start(0.0)));
        self.yaw = yaw(replace(&mut self.yaw, Track::start(0.0)));
    }

    fn body(self) -> BodyTracks {
        BodyTracks {
            x_m: self.x,
            z_m: self.z,
            yaw_deg: self.yaw,
            ..BodyTracks::default()
        }
    }
}
