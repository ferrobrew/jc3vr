//! The gun handling's tuning, read from the shipped `tuning/handling.toml` ([`HANDLING`]): the
//! weight of a long gun, the recoil, the foregrip, aiming down the sights, the scope, the push out
//! of walls, the grip watch, and when a throwing hand opens. Each section's type lives with the
//! subsystem that reads it and documents its values.

use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range, vec3},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::controllers::weapons::{
    ads::solve::AdsTuning,
    collide::CollideTuning,
    foregrip::ForegripTuning,
    grip_watch::GripWatchTuning,
    heft::{BracedTuning, Heft, HeftTuning},
    opening::OpeningTuning,
    recoil::RecoilTuning,
    scope::ScopeTuning,
};

jc3vr_core::tuning_file! {
    /// The gun handling's tuning file.
    pub static HANDLING: HandlingTuning = "handling.toml";
}

/// The gun handling's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandlingTuning {
    pub heft: Heft,
    pub recoil: RecoilTuning,
    pub foregrip: ForegripTuning,
    pub ads: AdsTuning,
    pub scope: ScopeTuning,
    pub collide: CollideTuning,
    pub grip_watch: GripWatchTuning,
    pub opening: OpeningTuning,
}

impl Tuning for HandlingTuning {
    fn validate(&self) -> Result<(), Invalid> {
        validate_heft(&self.heft)?;
        validate_recoil(&self.recoil)?;
        validate_foregrip(&self.foregrip)?;
        validate_ads(&self.ads)?;
        validate_scope(&self.scope)?;
        validate_collide(&self.collide)?;
        validate_grip_watch(&self.grip_watch)?;
        validate_opening(&self.opening)
    }
}

/// The longest a freshness window or a gap may be, milliseconds.
const MAX_WINDOW_MS: u64 = 10_000;

fn validate_heft(heft: &Heft) -> Result<(), Invalid> {
    nonnegative("heft.reach_near_m", heft.reach_near_m, 2.0)?;
    range(
        "heft.reach_far_m",
        heft.reach_far_m,
        heft.reach_near_m + f32::EPSILON,
        3.0,
    )?;
    nonnegative("heft.sway_hz", heft.sway_hz, 20.0)?;
    positive("heft.sway_yaw_ratio", heft.sway_yaw_ratio, 10.0)?;
    range(
        "heft.sway_yaw_phase",
        heft.sway_yaw_phase,
        -std::f32::consts::TAU,
        std::f32::consts::TAU,
    )?;
    range("heft.sway_yaw_share", heft.sway_yaw_share, 0.0, 1.0)?;
    range("heft.sway_near_share", heft.sway_near_share, 0.0, 1.0)?;
    range("heft.max_angle_deg", heft.max_angle_deg, 0.0, 180.0)?;
    heft.target_ms.validate("heft.target_ms", MAX_WINDOW_MS)?;
    heft.max_gap_ms.validate("heft.max_gap_ms", MAX_WINDOW_MS)?;
    validate_one_hand("heft.sidearm", &heft.sidearm)?;
    validate_one_hand("heft.rifle", &heft.rifle)?;
    validate_one_hand("heft.heavy", &heft.heavy)?;
    let braced = &heft.braced;
    nonnegative("heft.braced.strength", braced.strength, 4.0)?;
    range(
        "heft.braced.max_angle_deg",
        braced.max_angle_deg,
        0.0,
        180.0,
    )?;
    validate_braced("heft.braced.smg", &braced.smg)?;
    validate_braced("heft.braced.rifle", &braced.rifle)?;
    validate_braced("heft.braced.heavy", &braced.heavy)
}

fn validate_one_hand(section: &str, t: &HeftTuning) -> Result<(), Invalid> {
    range(&format!("{section}.sag_deg"), t.sag_deg, 0.0, 90.0)?;
    range(
        &format!("{section}.reach_sag_deg"),
        t.reach_sag_deg,
        0.0,
        90.0,
    )?;
    nonnegative(
        &format!("{section}.lag_stiffness"),
        t.lag_stiffness,
        100_000.0,
    )?;
    nonnegative(
        &format!("{section}.lag_damping_ratio"),
        t.lag_damping_ratio,
        10.0,
    )?;
    range(&format!("{section}.sway_deg"), t.sway_deg, 0.0, 30.0)?;
    nonnegative(&format!("{section}.kick_scale"), t.kick_scale, 10.0)?;
    nonnegative(&format!("{section}.spread_scale"), t.spread_scale, 10.0)
}

fn validate_braced(section: &str, t: &BracedTuning) -> Result<(), Invalid> {
    range(&format!("{section}.sag_deg"), t.sag_deg, 0.0, 90.0)?;
    nonnegative(&format!("{section}.stiffness"), t.stiffness, 100_000.0)
}

fn validate_recoil(recoil: &RecoilTuning) -> Result<(), Invalid> {
    nonnegative(
        "recoil.kick_back_m_per_rad",
        recoil.kick_back_m_per_rad,
        5.0,
    )?;
    range("recoil.max_angle_deg", recoil.max_angle_deg, 0.0, 90.0)?;
    nonnegative(
        "recoil.spring_stiffness",
        recoil.spring_stiffness,
        100_000.0,
    )?;
    nonnegative(
        "recoil.spring_damping_ratio",
        recoil.spring_damping_ratio,
        10.0,
    )?;
    nonnegative(
        "recoil.two_hand_stiffness_scale",
        recoil.two_hand_stiffness_scale,
        10.0,
    )?;
    nonnegative(
        "recoil.two_hand_kick_scale",
        recoil.two_hand_kick_scale,
        10.0,
    )?;
    recoil
        .max_gap_ms
        .validate("recoil.max_gap_ms", MAX_WINDOW_MS)
}

fn validate_foregrip(foregrip: &ForegripTuning) -> Result<(), Invalid> {
    positive("foregrip.radius_m", foregrip.radius_m, 1.0)?;
    positive(
        "foregrip.release_distance_m",
        foregrip.release_distance_m,
        2.0,
    )?;
    nonnegative("foregrip.blend_s", foregrip.blend_s, 5.0)?;
    positive(
        "foregrip.fallback_distance_m",
        foregrip.fallback_distance_m,
        2.0,
    )?;
    positive("foregrip.min_span_m", foregrip.min_span_m, 1.0)?;
    range("foregrip.max_turn_deg", foregrip.max_turn_deg, 0.0, 180.0)
}

fn validate_ads(ads: &AdsTuning) -> Result<(), Invalid> {
    nonnegative("ads.lock_m", ads.lock_m, 1.0)?;
    range("ads.engage_m", ads.engage_m, ads.lock_m + f32::EPSILON, 1.0)?;
    positive("ads.max_eye_behind_m", ads.max_eye_behind_m, 2.0)?;
    range(
        "ads.eye_behind_fade_m",
        ads.eye_behind_fade_m,
        f32::EPSILON,
        ads.max_eye_behind_m * 0.5,
    )?;
    nonnegative("ads.full_view_angle_deg", ads.full_view_angle_deg, 180.0)?;
    range(
        "ads.max_view_angle_deg",
        ads.max_view_angle_deg,
        ads.full_view_angle_deg + f32::EPSILON,
        180.0,
    )?;
    nonnegative("ads.pivot_ahead_m", ads.pivot_ahead_m, 2.0)?;
    nonnegative("ads.butt_behind_grip_m", ads.butt_behind_grip_m, 2.0)?;
    nonnegative("ads.shoulder_lock_m", ads.shoulder_lock_m, 1.0)?;
    range(
        "ads.shoulder_engage_m",
        ads.shoulder_engage_m,
        ads.shoulder_lock_m + f32::EPSILON,
        1.0,
    )?;
    nonnegative("ads.weld_lock_m", ads.weld_lock_m, 1.0)?;
    range(
        "ads.weld_engage_m",
        ads.weld_engage_m,
        ads.weld_lock_m + f32::EPSILON,
        1.0,
    )?;
    nonnegative("ads.ease_s", ads.ease_s, 5.0)?;
    positive("ads.max_ease_step_s", ads.max_ease_step_s, 5.0)?;
    nonnegative("ads.half_ipd_m", ads.half_ipd_m, 0.1)?;
    range("ads.taken_share", ads.taken_share, 0.5, 1.0)?;
    vec3("ads.pocket_offset_m", ads.pocket_offset_m, 0.5)
}

fn validate_scope(scope: &ScopeTuning) -> Result<(), Invalid> {
    scope.scoped_ms.validate("scope.scoped_ms", MAX_WINDOW_MS)?;
    let lens = &scope.lens;
    positive("scope.lens.zoom", lens.zoom, 100.0)?;
    positive("scope.lens.lens_radius_m", lens.lens_radius_m, 0.5)?;
    nonnegative("scope.lens.eyepiece_back_m", lens.eyepiece_back_m, 3.0)?;
    positive(
        "scope.lens.max_eye_distance_m",
        lens.max_eye_distance_m,
        2.0,
    )?;
    range("scope.lens.vignette_start", lens.vignette_start, 0.0, 1.0)?;
    positive("scope.lens.min_focus_m", lens.min_focus_m, 1000.0)
}

fn validate_collide(collide: &CollideTuning) -> Result<(), Invalid> {
    nonnegative("collide.tip_margin_m", collide.tip_margin_m, 1.0)?;
    range("collide.max_turn_deg", collide.max_turn_deg, 0.0, 180.0)?;
    nonnegative("collide.max_retract_m", collide.max_retract_m, 2.0)?;
    nonnegative("collide.keep_s", collide.keep_s, 10.0)
}

fn validate_grip_watch(watch: &GripWatchTuning) -> Result<(), Invalid> {
    positive("grip_watch.flip_deg", watch.flip_deg, 180.0)?;
    positive("grip_watch.sweep_deg", watch.sweep_deg, 180.0)?;
    range(
        "grip_watch.sweep_rearm_share",
        watch.sweep_rearm_share,
        0.0,
        1.0,
    )?;
    watch
        .sweep_hold_ms
        .validate("grip_watch.sweep_hold_ms", MAX_WINDOW_MS)?;
    watch
        .record_ms
        .validate("grip_watch.record_ms", MAX_WINDOW_MS)
}

fn validate_opening(opening: &OpeningTuning) -> Result<(), Invalid> {
    positive("opening.closed", opening.closed, 1.0)?;
    range(
        "opening.open",
        opening.open,
        f32::EPSILON,
        opening.closed - f32::EPSILON,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(HANDLING.get().validate(), Ok(()));
    }
}
