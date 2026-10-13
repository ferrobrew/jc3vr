//! A kinematic world for testing the rig without the game: a root stepped at the simulation rate
//! that integrates the composed velocity, optional wall planes that stop it, a frame half at the
//! headset's rate with the push-back, and the pose pair the engine interpolates the camera across.
//!
//! The rig's own pure parts run unchanged: the ledger, the cockpit shift, the chase's measurement
//! and velocity, and the lean. The world stands in for the engine: the proxy's solve is a straight
//! integration clamped by the walls, and the camera is the root pair carried by the shifted cockpit
//! pair and interpolated by the time since the last tick.
//!
//! The player's room is a frame that moves through the world with the stick (the stick moves the
//! root and the camera together), so the real head's world position is the room's origin plus the
//! head's cockpit position. The camera should stay on it; the root should stay under it.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::body::{
    procedural::head::head_drop,
    rig::{
        config::RigConfig,
        lean::pivot_height,
        root::{
            chase::{
                self, ChaseFrame, HeadVelocity, MeasureInput, Measurement, measure, to_cockpit,
                to_world,
            },
            ledger::{Movement, Requests},
            owner::RootOwner,
            pushback::{Pushback, PushbackInput, PushbackOutcome},
            shift::{CockpitBasis, CockpitShift},
        },
    },
    torso::head_heading,
};

/// The simulation rate, ticks per second.
pub(crate) const TICK_HZ: f32 = 30.0;
/// The headset's frame rate, frames per second.
pub(crate) const FRAME_HZ: f32 = 90.0;

/// The player's head in the room: the eye midpoint, cockpit frame, and its orientation.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Head {
    pub(crate) eyes: Vec3,
    pub(crate) orientation: Quat,
}

impl Head {
    /// Standing upright at the cockpit's height, looking ahead, eyes at `(x, z)`.
    pub(crate) fn standing(x: f32, z: f32) -> Self {
        Self {
            eyes: Vec3::new(x, 0.0, z),
            orientation: Quat::IDENTITY,
        }
    }
}

/// A wall: the root stays where `normal · p <= offset`, world `(x, z)`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Wall {
    pub(crate) normal: Vec2,
    pub(crate) offset: f32,
}

/// What one frame saw.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Sample {
    /// World time, seconds.
    pub(crate) t: f32,
    /// How far the ground point under the upright eyes is from the root, world metres.
    pub(crate) gap_m: f32,
    /// How far it is outside the lean, world metres.
    pub(crate) residual_m: f32,
    /// How far the interpolated camera is from the real head, world metres, horizontally.
    pub(crate) camera_error_m: f32,
    pub(crate) root: Vec2,
    pub(crate) measurement: Measurement,
    pub(crate) pushback: PushbackOutcome,
    /// The push-back's slide in its current episode, metres.
    pub(crate) debt_m: f32,
}

/// The kinematic world.
pub(crate) struct World {
    pub(crate) config: RigConfig,
    pub(crate) owner: RootOwner,
    /// The stick's (and the hands') velocity, world `(x, z)`, metres per second.
    pub(crate) stick: Vec2,
    pub(crate) walls: Vec<Wall>,
    view: Quat,
    world_scale: f32,
    frame_hz: f32,
    time: f64,
    last_tick: f64,
    root_prev: Vec2,
    root: Vec2,
    room_prev: Vec2,
    room: Vec2,
    ledger: Requests,
    shift: CockpitShift,
    head_velocity: HeadVelocity,
    published: Option<ChaseFrame>,
    pushback: Pushback,
    ticks: u64,
}

impl World {
    pub(crate) fn new() -> Self {
        Self {
            config: RigConfig::new(),
            owner: RootOwner::Player,
            stick: Vec2::ZERO,
            walls: Vec::new(),
            view: Quat::IDENTITY,
            world_scale: 1.0,
            frame_hz: FRAME_HZ,
            time: 0.0,
            last_tick: 0.0,
            root_prev: Vec2::ZERO,
            root: Vec2::ZERO,
            room_prev: Vec2::ZERO,
            room: Vec2::ZERO,
            ledger: Requests::new(),
            shift: CockpitShift::new(),
            head_velocity: HeadVelocity::new(),
            published: None,
            pushback: Pushback::new(),
            ticks: 0,
        }
    }

    /// Compose the cockpit with the body rotation `view` and `world_scale` world metres per
    /// cockpit metre.
    pub(crate) fn with_view(mut self, view: Quat, world_scale: f32) -> Self {
        self.view = view;
        self.world_scale = world_scale;
        self
    }

    /// Render at `hz` frames per second.
    pub(crate) fn with_frame_rate(mut self, hz: f32) -> Self {
        self.frame_hz = hz;
        self
    }

    /// Place the root and the room far from the world origin, where positions round coarsely.
    pub(crate) fn at(mut self, origin: Vec2) -> Self {
        self.root = origin;
        self.root_prev = origin;
        self.room = origin;
        self.room_prev = origin;
        self
    }

    pub(crate) fn time(&self) -> f32 {
        self.time as f32
    }

    pub(crate) fn root(&self) -> Vec2 {
        self.root
    }

    pub(crate) fn ticks(&self) -> u64 {
        self.ticks
    }

    pub(crate) fn ledger(&self) -> &Requests {
        &self.ledger
    }

    pub(crate) fn pushback(&self) -> &Pushback {
        &self.pushback
    }

    /// The real head's world `(x, z)` for its cockpit position `eyes`, now.
    pub(crate) fn real_head(&self, eyes: Vec3) -> Vec2 {
        self.room + to_world(Vec2::new(eyes.x, eyes.z), self.view, self.world_scale)
    }

    /// The camera's world `(x, z)` for the cockpit position `eyes` on the current side of the pair.
    pub(crate) fn camera(&self, eyes: Vec3) -> Vec2 {
        let (_, cur) = self.shift.apply(eyes);
        self.root + to_world(Vec2::new(cur.x, cur.z), self.view, self.world_scale)
    }

    /// Run for `seconds`, with the head in the room at `head(t)` for the world time `t`, calling
    /// `each` with every frame's sample.
    pub(crate) fn run(
        &mut self,
        seconds: f32,
        head: impl Fn(f32) -> Head,
        mut each: impl FnMut(&Sample),
    ) {
        let frame_dt = 1.0 / f64::from(self.frame_hz);
        let tick_dt = 1.0 / f64::from(TICK_HZ);
        let end = self.time + f64::from(seconds);
        while self.time + frame_dt <= end + 1e-9 {
            self.time += frame_dt;
            while self.last_tick + tick_dt <= self.time + 1e-9 {
                self.last_tick += tick_dt;
                self.tick(tick_dt as f32);
            }
            let alpha = ((self.time - self.last_tick) / tick_dt).clamp(0.0, 1.0) as f32;
            let sample = self.frame(head(self.time as f32), frame_dt as f32, alpha);
            each(&sample);
        }
    }

    /// One simulation tick: the chase on the latest frame's measurement, the root integrated and
    /// stopped by the walls, and the step completed.
    fn tick(&mut self, dt: f32) {
        self.ticks += 1;
        let chase = match self.published.filter(|_| self.owner.chases()) {
            Some(frame) => chase::velocity(&frame, self.ledger.completed(), dt, &self.config.chase),
            None => {
                self.ledger.clear();
                Vec2::ZERO
            }
        };
        let movement = Movement::new(self.stick, chase, dt);
        if self.owner.chases() {
            self.ledger.submit(movement);
        }
        let mut next = self.root + movement.task + movement.chase;
        for wall in &self.walls {
            let over = wall.normal.dot(next) - wall.offset;
            if over > 0.0 {
                next -= wall.normal * over;
            }
        }
        self.root_prev = self.root;
        self.root = next;
        self.room_prev = self.room;
        self.room += movement.task;
        let flat = |v: Vec2| Vec3::new(v.x, 0.0, v.y);
        self.ledger.complete(flat(self.root_prev), flat(self.root));
    }

    /// One frame half: the shift folds in the completed chase, the gap is measured and published,
    /// and the camera is composed and interpolated by `alpha`.
    fn frame(&mut self, head: Head, dt: f32, alpha: f32) -> Sample {
        let basis = CockpitBasis {
            body: self.view,
            world_scale: self.world_scale,
        };
        self.shift.observe(&basis, self.ledger.completed());
        let torso_yaw = head_heading(head.orientation).unwrap_or(0.0);
        let drop_m = head_drop(pivot_height(head.eyes, head.orientation), 0.0, 0.0);
        let mut input = MeasureInput {
            raw: head.eyes,
            shift: self.shift.current(),
            observed: self.shift.observed(),
            head: head.orientation,
            torso_yaw,
            view: self.view,
            world_scale: self.world_scale,
            drop_m,
            dt,
        };
        let mut measurement = measure(&input, &mut self.head_velocity, &self.config.lean);
        let pushback = self.pushback.step(
            &PushbackInput {
                gap: measurement.frame.gap,
                owner: Some(self.owner),
                stopped: self.ledger.obstructed(),
                dt,
            },
            &self.config.pushback,
        );
        if pushback.push != Vec2::ZERO {
            self.shift
                .slide_view(to_cockpit(pushback.push, self.view, self.world_scale));
            input.shift = self.shift.current();
            input.dt = 0.0;
            measurement = measure(&input, &mut self.head_velocity, &self.config.lean);
        }
        self.published = Some(measurement.frame);

        let (prev, cur) = self.shift.apply(head.eyes);
        let world = |v: Vec3| to_world(Vec2::new(v.x, v.z), self.view, self.world_scale);
        let camera = (self.root_prev + world(prev)).lerp(self.root + world(cur), alpha);
        let real = self.room_prev.lerp(self.room, alpha) + world(head.eyes);
        let scale = self.world_scale;
        Sample {
            t: self.time as f32,
            gap_m: measurement.gap_m() * scale,
            residual_m: measurement.residual_m() * scale,
            camera_error_m: camera.distance(real),
            root: self.root,
            measurement,
            pushback,
            debt_m: self.pushback.debt(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_still_head_moves_nothing() {
        let mut world = World::new();
        world.run(
            2.0,
            |_| Head::standing(0.0, 0.0),
            |s| {
                assert_eq!(s.root, Vec2::ZERO);
                assert_eq!(s.gap_m, 0.0);
                assert_eq!(s.camera_error_m, 0.0);
            },
        );
        assert_eq!(world.ticks(), 60);
    }

    #[test]
    fn the_stick_moves_the_root_and_the_camera_together() {
        let mut world = World::new();
        world.stick = Vec2::new(0.0, -3.0);
        world.run(
            1.0,
            |_| Head::standing(0.0, 0.0),
            |s| {
                assert!(s.gap_m < 1e-5, "{s:?}");
                assert!(s.camera_error_m < 1e-5, "{s:?}");
            },
        );
        assert!((world.root() - Vec2::new(0.0, -3.0)).length() < 1e-4);
    }

    #[test]
    fn a_wall_stops_the_root() {
        let mut world = World::new();
        world.walls.push(Wall {
            normal: Vec2::new(0.0, -1.0),
            offset: 0.5,
        });
        world.stick = Vec2::new(0.0, -2.0);
        world.run(1.0, |_| Head::standing(0.0, 0.0), |_| {});
        assert!((world.root().y + 0.5).abs() < 1e-5, "{}", world.root());
    }

    #[test]
    fn a_step_in_the_room_is_followed() {
        let mut world = World::new();
        world.run(1.0, |_| Head::standing(0.3, 0.0), |_| {});
        assert!((world.root() - Vec2::new(0.3, 0.0)).length() < 1e-3);
    }
}
