//! The standing stance's pure logic: whether the character counts as standing still, the blend
//! that eases the squaring in and out, the turns measured from the animated pose, the squared
//! footprint, and which of the idle's clip variants the engine's modifiers select.
//!
//! Positions are in character-model space, which shares the character's up axis (+Y) and faces
//! -Z. Turns are about +Y, positive when +X turns toward -Z.

use glam::Vec3;

/// What the stance logic decided on a tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum Verdict {
    /// Standing still: the squaring may act.
    StandingStill,
    /// Something rules the stance out.
    StandDown(StandDown),
    /// The logic has not run for the local player yet.
    #[default]
    NotRun,
}

/// Why the stance stands down, in the order the conditions are tested.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StandDown {
    NotControllers,
    NotVr,
    NotInGameplay,
    NotOnFoot,
    NotIdleState,
    Aiming,
    GrappleTether,
    Crouched,
    Moving,
}

impl StandDown {
    /// Whether the condition ends the squaring at once rather than blending it out: the character
    /// left the ground's idle for a seat, the air, or a cutscene, or the headset or the scheme
    /// stopped driving it.
    pub fn is_hard(self) -> bool {
        matches!(
            self,
            Self::NotControllers | Self::NotVr | Self::NotInGameplay | Self::NotOnFoot
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NotControllers => "not the controller scheme",
            Self::NotVr => "the headset does not drive the view",
            Self::NotInGameplay => "not in gameplay",
            Self::NotOnFoot => "not on foot",
            Self::NotIdleState => "not in the idle state",
            Self::Aiming => "aiming",
            Self::GrappleTether => "holding a grapple tether",
            Self::Crouched => "crouched",
            Self::Moving => "moving",
        }
    }
}

/// The conditions [`classify`] tests, read from the engine and the mod each tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StillInputs {
    pub controllers: bool,
    pub vr: bool,
    pub gameplay: bool,
    pub on_foot: bool,
    /// The body's rule system is in the base idle state.
    pub idle_state: bool,
    /// The aim flags report a weapon or the grapple aiming.
    pub aiming: bool,
    /// The grapple hook holds a live tether.
    pub tether: bool,
    pub crouched: bool,
    /// The character's horizontal speed, metres per second, or `None` when it could not be
    /// measured this tick.
    pub speed_mps: Option<f32>,
}

/// Decide whether the character stands still, given the speed below which it counts as still.
pub fn classify(inputs: &StillInputs, still_speed_mps: f32) -> Verdict {
    let reason = [
        (!inputs.controllers, StandDown::NotControllers),
        (!inputs.vr, StandDown::NotVr),
        (!inputs.gameplay, StandDown::NotInGameplay),
        (!inputs.on_foot, StandDown::NotOnFoot),
        (!inputs.idle_state, StandDown::NotIdleState),
        (inputs.aiming, StandDown::Aiming),
        (inputs.tether, StandDown::GrappleTether),
        (inputs.crouched, StandDown::Crouched),
        (
            !inputs
                .speed_mps
                .is_some_and(|speed| speed < still_speed_mps.max(0.0)),
            StandDown::Moving,
        ),
    ]
    .into_iter()
    .find_map(|(failed, reason)| failed.then_some(reason));
    match reason {
        Some(reason) => Verdict::StandDown(reason),
        None => Verdict::StandingStill,
    }
}

/// The horizontal speed between two positions `dt` seconds apart, or `None` for a step that cannot
/// carry a speed.
pub fn horizontal_speed(previous: Vec3, current: Vec3, dt: f32) -> Option<f32> {
    if !(dt.is_finite() && dt > 0.0) {
        return None;
    }
    let delta = current - previous;
    let speed = delta.x.hypot(delta.z) / dt;
    speed.is_finite().then_some(speed)
}

/// The blend's timing, seconds.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BlendTiming {
    pub settle_s: f32,
    pub blend_in_s: f32,
    pub blend_out_s: f32,
}

/// The squaring's blend: it waits for the character to have stood still for the settle time,
/// ramps in, and ramps out as soon as the character stops standing still.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StanceBlend {
    still_for_s: f32,
    ramp: f32,
}

impl StanceBlend {
    /// Advance by `dt` seconds with the tick's verdict, and return the eased weight. A tick with no
    /// usable `dt` (a paused game) holds the blend.
    pub fn step(&mut self, still: bool, dt: f32, timing: BlendTiming) -> f32 {
        if dt.is_finite() && dt > 0.0 {
            if still {
                self.still_for_s += dt;
                if self.still_for_s >= timing.settle_s.max(0.0) {
                    self.ramp += rate(dt, timing.blend_in_s);
                }
            } else {
                self.still_for_s = 0.0;
                self.ramp -= rate(dt, timing.blend_out_s);
            }
            self.ramp = self.ramp.clamp(0.0, 1.0);
        }
        self.weight()
    }

    /// The eased weight, `0..=1`.
    pub fn weight(&self) -> f32 {
        smoothstep(self.ramp)
    }

    /// Drop the blend at once, for a tick on which the stance cannot be read at all.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The turn of the line from `left` to `right` away from the model's lateral axis, radians, in
/// `-π/2..=π/2`; `None` when the points coincide horizontally. Wrapping to a half turn makes the
/// measure independent of which side of the model the rig puts its left.
pub fn lateral_twist(left: Vec3, right: Vec3) -> Option<f32> {
    let v = right - left;
    half_turn_yaw(v.x, -v.z)
}

/// The turn of the direction from `from` to `to` away from the model's forward (-Z), radians, in
/// `-π/2..=π/2`; `None` when the points coincide horizontally.
pub fn forward_yaw(from: Vec3, to: Vec3) -> Option<f32> {
    let v = to - from;
    half_turn_yaw(-v.z, -v.x)
}

/// The turn that removes `share` of a measured `turn`, scaled by the blend `weight`, and bounded
/// to [`MAX_CORRECTION_RAD`].
pub fn correction(turn: f32, share: f32, weight: f32) -> f32 {
    if !turn.is_finite() {
        return 0.0;
    }
    let share = if share.is_finite() {
        share.clamp(0.0, 1.0)
    } else {
        0.0
    };
    (-turn * share * weight.clamp(0.0, 1.0)).clamp(-MAX_CORRECTION_RAD, MAX_CORRECTION_RAD)
}

/// The largest turn the squaring applies to any one part, radians: a bladed stance turns the
/// pelvis well inside it, and anything larger is a pose the squaring misread.
pub const MAX_CORRECTION_RAD: f32 = std::f32::consts::FRAC_PI_4;

/// The two ankle targets.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Footprint {
    pub left: Vec3,
    pub right: Vec3,
}

/// The animated feet's layout across and along the body.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FeetLayout {
    /// The distance between the ankles across the body, metres.
    pub spacing_m: f32,
    /// How far the right ankle sits ahead of the left along the body's forward, metres.
    pub stagger_m: f32,
}

/// Measure the animated ankles' spacing and stagger.
pub fn feet_layout(left: Vec3, right: Vec3) -> FeetLayout {
    FeetLayout {
        spacing_m: (right.x - left.x).abs(),
        stagger_m: left.z - right.z,
    }
}

/// Square the animated ankles: both at their midpoint's depth, `width_m` apart across the body
/// about the midpoint (the animated spacing when `width_m` is not positive), each at its own
/// animated height, then blended from the animated positions by `weight`. Each ankle stays on the
/// side the animation put it.
pub fn square_feet(left: Vec3, right: Vec3, width_m: f32, weight: f32) -> Footprint {
    let weight = if weight.is_finite() {
        weight.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mid = (left + right) * 0.5;
    let half = if width_m.is_finite() && width_m > 0.0 {
        width_m * 0.5
    } else {
        (right.x - left.x).abs() * 0.5
    };
    // The left ankle's side of the body, from the animation: -X unless the rig puts the left on +X.
    let left_side = if left.x > right.x { 1.0 } else { -1.0 };
    let squared = |ankle: Vec3, side: f32| Vec3::new(mid.x + side * half, ankle.y, mid.z);
    Footprint {
        left: left.lerp(squared(left, left_side), weight),
        right: right.lerp(squared(right, -left_side), weight),
    }
}

/// Which of the base idle's clip variants the engine's modifiers select this tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum IdleVariant {
    /// No weapon modifier on the body: the unarmed relaxed idle.
    #[default]
    Relaxed,
    /// The wielded weapon's held modifier: the weapon's held stance.
    Held,
    /// The wielded weapon's aim modifier: the aimed stance.
    Aimed,
    /// The grapple modifier: the aimed stance with the grapple arm.
    Grapple,
}

impl IdleVariant {
    pub fn label(self) -> &'static str {
        match self {
            Self::Relaxed => "relaxed (unarmed clip)",
            Self::Held => "weapon held (held stance, or the aimed stance for an explosive)",
            Self::Aimed => "weapon aimed (aimed stance)",
            Self::Grapple => "grapple (aimed stance)",
        }
    }
}

/// The variant the modifiers select: the grapple's while the hook aims, none from a weapon whose
/// modifiers were kept off the body, and otherwise the weapon's aimed or held one.
pub fn idle_variant(
    weapon: bool,
    trying_to_aim: bool,
    tether: bool,
    suppressed: bool,
) -> IdleVariant {
    match (tether, weapon && !suppressed, trying_to_aim) {
        (true, _, _) => IdleVariant::Grapple,
        (false, false, _) => IdleVariant::Relaxed,
        (false, true, true) => IdleVariant::Aimed,
        (false, true, false) => IdleVariant::Held,
    }
}

fn rate(dt: f32, span_s: f32) -> f32 {
    if span_s.is_finite() && span_s > 0.0 {
        dt / span_s
    } else {
        1.0
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The angle of `(x, y)` from +x, wrapped to `-π/2..=π/2`.
fn half_turn_yaw(x: f32, y: f32) -> Option<f32> {
    if x.hypot(y) < 1.0e-5 || !x.is_finite() || !y.is_finite() {
        return None;
    }
    let angle = y.atan2(x);
    let half = std::f32::consts::FRAC_PI_2;
    Some(if angle > half {
        angle - std::f32::consts::PI
    } else if angle <= -half {
        angle + std::f32::consts::PI
    } else {
        angle
    })
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    const TIMING: BlendTiming = BlendTiming {
        settle_s: 0.4,
        blend_in_s: 0.5,
        blend_out_s: 0.1,
    };

    fn still() -> StillInputs {
        StillInputs {
            controllers: true,
            vr: true,
            gameplay: true,
            on_foot: true,
            idle_state: true,
            aiming: false,
            tether: false,
            crouched: false,
            speed_mps: Some(0.0),
        }
    }

    #[test]
    fn a_still_idle_character_stands_still() {
        assert_eq!(classify(&still(), 0.15), Verdict::StandingStill);
    }

    #[test]
    fn each_condition_stands_the_stance_down() {
        let cases = [
            (
                StillInputs {
                    controllers: false,
                    ..still()
                },
                StandDown::NotControllers,
            ),
            (
                StillInputs {
                    vr: false,
                    ..still()
                },
                StandDown::NotVr,
            ),
            (
                StillInputs {
                    gameplay: false,
                    ..still()
                },
                StandDown::NotInGameplay,
            ),
            (
                StillInputs {
                    on_foot: false,
                    ..still()
                },
                StandDown::NotOnFoot,
            ),
            (
                StillInputs {
                    idle_state: false,
                    ..still()
                },
                StandDown::NotIdleState,
            ),
            (
                StillInputs {
                    aiming: true,
                    ..still()
                },
                StandDown::Aiming,
            ),
            (
                StillInputs {
                    tether: true,
                    ..still()
                },
                StandDown::GrappleTether,
            ),
            (
                StillInputs {
                    crouched: true,
                    ..still()
                },
                StandDown::Crouched,
            ),
            (
                StillInputs {
                    speed_mps: Some(0.5),
                    ..still()
                },
                StandDown::Moving,
            ),
            (
                StillInputs {
                    speed_mps: None,
                    ..still()
                },
                StandDown::Moving,
            ),
            (
                StillInputs {
                    speed_mps: Some(f32::NAN),
                    ..still()
                },
                StandDown::Moving,
            ),
        ];
        for (inputs, reason) in cases {
            assert_eq!(classify(&inputs, 0.15), Verdict::StandDown(reason));
        }
    }

    #[test]
    fn leaving_the_ground_or_the_headset_stops_at_once() {
        assert!(StandDown::NotOnFoot.is_hard());
        assert!(StandDown::NotVr.is_hard());
        assert!(!StandDown::Moving.is_hard());
        assert!(!StandDown::Aiming.is_hard());
    }

    #[test]
    fn the_first_failed_condition_is_reported() {
        let inputs = StillInputs {
            on_foot: false,
            aiming: true,
            ..still()
        };
        assert_eq!(
            classify(&inputs, 0.15),
            Verdict::StandDown(StandDown::NotOnFoot)
        );
    }

    #[test]
    fn speed_is_horizontal_distance_over_time() {
        let speed = horizontal_speed(Vec3::ZERO, Vec3::new(0.3, 5.0, 0.4), 0.5).unwrap();
        assert!((speed - 1.0).abs() < 1e-6);
        assert_eq!(horizontal_speed(Vec3::ZERO, Vec3::X, 0.0), None);
        assert_eq!(horizontal_speed(Vec3::ZERO, Vec3::X, f32::NAN), None);
    }

    fn run(blend: &mut StanceBlend, still: bool, seconds: f32) -> f32 {
        let mut weight = blend.weight();
        for _ in 0..(seconds / 0.01).round() as usize {
            weight = blend.step(still, 0.01, TIMING);
        }
        weight
    }

    #[test]
    fn the_blend_waits_for_the_settle_time() {
        let mut blend = StanceBlend::default();
        assert_eq!(run(&mut blend, true, 0.35), 0.0);
        assert!(run(&mut blend, true, 0.2) > 0.0);
    }

    #[test]
    fn the_blend_reaches_full_after_settling_and_blending_in() {
        let mut blend = StanceBlend::default();
        assert!((run(&mut blend, true, 1.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_blend_eases_and_never_overshoots() {
        let mut blend = StanceBlend::default();
        run(&mut blend, true, 0.4);
        let mut last = 0.0;
        for _ in 0..100 {
            let weight = blend.step(true, 0.01, TIMING);
            assert!((0.0..=1.0).contains(&weight));
            assert!(weight >= last);
            last = weight;
        }
        let quarter = {
            let mut b = StanceBlend::default();
            run(&mut b, true, 0.4 + 0.125)
        };
        assert!(
            quarter < 0.25,
            "smoothstep starts slower than linear: {quarter}"
        );
    }

    #[test]
    fn moving_blends_out_fast_and_restarts_the_settle() {
        let mut blend = StanceBlend::default();
        run(&mut blend, true, 1.0);
        assert_eq!(run(&mut blend, false, 0.1), 0.0);
        assert_eq!(run(&mut blend, true, 0.3), 0.0);
    }

    #[test]
    fn a_paused_tick_holds_the_blend() {
        let mut blend = StanceBlend::default();
        let weight = run(&mut blend, true, 0.7);
        assert_eq!(blend.step(false, 0.0, TIMING), weight);
        assert_eq!(blend.step(false, f32::NAN, TIMING), weight);
    }

    #[test]
    fn zero_spans_switch_at_once() {
        let timing = BlendTiming {
            settle_s: 0.0,
            blend_in_s: 0.0,
            blend_out_s: 0.0,
        };
        let mut blend = StanceBlend::default();
        assert_eq!(blend.step(true, 0.01, timing), 1.0);
        assert_eq!(blend.step(false, 0.01, timing), 0.0);
    }

    fn turned(v: Vec3, angle: f32) -> Vec3 {
        Quat::from_rotation_y(angle) * v
    }

    #[test]
    fn the_lateral_twist_is_the_turn_about_up() {
        for angle in [0.0, 0.3, -0.5] {
            let left = turned(Vec3::new(-0.15, 1.0, 0.0), angle);
            let right = turned(Vec3::new(0.15, 1.0, 0.0), angle);
            assert!((lateral_twist(left, right).unwrap() - angle).abs() < 1e-5);
        }
    }

    #[test]
    fn the_lateral_twist_does_not_depend_on_the_rigs_handedness() {
        let left = turned(Vec3::new(0.15, 1.0, 0.0), 0.3);
        let right = turned(Vec3::new(-0.15, 1.0, 0.0), 0.3);
        assert!((lateral_twist(left, right).unwrap() - 0.3).abs() < 1e-5);
    }

    #[test]
    fn a_right_side_forward_is_a_positive_twist() {
        let left = Vec3::new(-0.15, 1.0, 0.05);
        let right = Vec3::new(0.15, 1.0, -0.05);
        assert!(lateral_twist(left, right).unwrap() > 0.0);
    }

    #[test]
    fn the_forward_yaw_is_the_turn_from_forward() {
        for angle in [0.0, 0.4, -0.2] {
            let toe = turned(Vec3::new(0.0, 0.0, -0.15), angle);
            assert!((forward_yaw(Vec3::ZERO, toe).unwrap() - angle).abs() < 1e-5);
        }
    }

    #[test]
    fn coincident_points_have_no_turn() {
        assert_eq!(lateral_twist(Vec3::Y, Vec3::Y * 2.0), None);
        assert_eq!(forward_yaw(Vec3::ZERO, Vec3::Y), None);
    }

    #[test]
    fn a_correction_undoes_the_turn() {
        let angle = 0.3;
        let fix = correction(angle, 1.0, 1.0);
        let lateral = Quat::from_rotation_y(fix) * turned(Vec3::X, angle);
        assert!((lateral - Vec3::X).length() < 1e-5);
    }

    #[test]
    fn a_correction_takes_its_share_and_weight_and_is_bounded() {
        assert!((correction(0.4, 0.5, 0.5) + 0.1).abs() < 1e-6);
        assert_eq!(correction(3.0, 1.0, 1.0), -MAX_CORRECTION_RAD);
        assert_eq!(correction(f32::NAN, 1.0, 1.0), 0.0);
        assert_eq!(correction(0.4, f32::NAN, 1.0), 0.0);
    }

    #[test]
    fn squared_feet_sit_side_by_side_at_the_width() {
        let left = Vec3::new(-0.1, 0.08, 0.12);
        let right = Vec3::new(0.12, 0.10, -0.18);
        let feet = square_feet(left, right, 0.3, 1.0);
        assert!((feet.left.z - feet.right.z).abs() < 1e-6);
        assert!((feet.right.x - feet.left.x - 0.3).abs() < 1e-6);
        assert!((feet.left.z - (-0.03)).abs() < 1e-6);
        assert!(((feet.left.x + feet.right.x) * 0.5 - 0.01).abs() < 1e-6);
        assert_eq!(feet.left.y, 0.08);
        assert_eq!(feet.right.y, 0.10);
    }

    #[test]
    fn zero_width_keeps_the_animated_spacing() {
        let left = Vec3::new(-0.1, 0.0, 0.1);
        let right = Vec3::new(0.14, 0.0, -0.1);
        let feet = square_feet(left, right, 0.0, 1.0);
        assert!((feet.right.x - feet.left.x - 0.24).abs() < 1e-6);
        assert!((feet.left.z - feet.right.z).abs() < 1e-6);
    }

    #[test]
    fn the_feet_keep_their_sides_on_a_mirrored_rig() {
        let left = Vec3::new(0.1, 0.0, 0.1);
        let right = Vec3::new(-0.1, 0.0, -0.1);
        let feet = square_feet(left, right, 0.3, 1.0);
        assert!(feet.left.x > feet.right.x);
    }

    #[test]
    fn the_feet_blend_from_the_animation() {
        let left = Vec3::new(-0.1, 0.0, 0.1);
        let right = Vec3::new(0.1, 0.0, -0.1);
        let none = square_feet(left, right, 0.3, 0.0);
        assert_eq!(none, Footprint { left, right });
        let half = square_feet(left, right, 0.3, 0.5);
        assert!((half.left.z - 0.05).abs() < 1e-6);
        assert_eq!(
            square_feet(left, right, 0.3, f32::NAN),
            Footprint { left, right }
        );
    }

    #[test]
    fn the_layout_reports_spacing_and_a_right_foot_forward_stagger() {
        let layout = feet_layout(Vec3::new(-0.1, 0.0, 0.1), Vec3::new(0.1, 0.0, -0.1));
        assert!((layout.spacing_m - 0.2).abs() < 1e-6);
        assert!((layout.stagger_m - 0.2).abs() < 1e-6);
    }

    #[test]
    fn the_variant_follows_the_engines_modifier_rules() {
        let cases = [
            ((false, false, false, false), IdleVariant::Relaxed),
            ((true, false, false, false), IdleVariant::Held),
            ((true, true, false, false), IdleVariant::Aimed),
            ((true, true, true, false), IdleVariant::Grapple),
            ((false, false, true, false), IdleVariant::Grapple),
            ((true, false, false, true), IdleVariant::Relaxed),
            ((true, true, false, true), IdleVariant::Relaxed),
        ];
        for ((weapon, trying_to_aim, tether, suppressed), variant) in cases {
            assert_eq!(
                idle_variant(weapon, trying_to_aim, tether, suppressed),
                variant
            );
        }
    }
}
