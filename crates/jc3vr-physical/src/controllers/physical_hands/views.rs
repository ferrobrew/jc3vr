//! The two in-world diagnostic views' tuning, read from the shipped `tuning/views.toml`
//! ([`VIEWS`]): the collider view's reach, budgets, and geometry
//! ([`crate::controllers::physical_hands::collider_view`]), and the grab view's
//! ([`crate::controllers::physical_hands::grab_view`]). Their colours are their identity, not
//! their tuning, and stay with the code that draws them.

use std::{fmt, time::Duration};

use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range},
    published::Window,
    tuning::Tuning,
};
use serde::Deserialize;

use crate::controllers::physical_hands::collider_view::draw::PRIMITIVE_CEILING;

jc3vr_core::tuning_file! {
    /// The diagnostic views' tuning file.
    pub static VIEWS: ViewsTuning = "views.toml";
}

/// The diagnostic views' tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewsTuning {
    pub collider_view: ColliderViewTuning,
    pub grab_view: GrabViewTuning,
}

impl Tuning for ViewsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.collider_view.validate()?;
        self.grab_view.validate()
    }
}

/// The collider view's reach, budgets, and timing.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColliderViewTuning {
    /// How far from the player a body's box may be and still be drawn, metres.
    pub radius_m: f32,
    /// How far the gaze looks for a body, as a multiple of [`Self::radius_m`].
    pub(crate) gaze_reach_per_radius: f32,
    /// The most bodies drawn, the nearest first.
    pub max_bodies: usize,
    /// The most overlay primitives the view submits in a frame, at most
    /// [`PRIMITIVE_CEILING`]. The overlay's own cap covers every readout together, and the view's
    /// slot comes last, so the view gives way first.
    pub(crate) max_primitives: usize,
    /// How long the view's overlay slot lives, milliseconds: the view submits every rendered
    /// frame, so its slot lives a little past one.
    pub ttl_ms: u64,
    /// The most contacts kept flashing at once.
    pub(crate) max_flashes: usize,
    pub draw: ColliderDrawTuning,
}

impl ColliderViewTuning {
    /// [`Self::ttl_ms`] as a duration.
    pub fn ttl(&self) -> Duration {
        Duration::from_millis(self.ttl_ms)
    }

    fn validate(&self) -> Result<(), Invalid> {
        positive("collider_view.radius_m", self.radius_m, 1000.0)?;
        positive(
            "collider_view.gaze_reach_per_radius",
            self.gaze_reach_per_radius,
            100.0,
        )?;
        count("collider_view.max_bodies", self.max_bodies, 1, 65_536)?;
        count(
            "collider_view.max_primitives",
            self.max_primitives,
            0,
            PRIMITIVE_CEILING,
        )?;
        count("collider_view.ttl_ms", self.ttl_ms, 1, 10_000)?;
        count("collider_view.max_flashes", self.max_flashes, 1, 4096)?;
        self.draw.validate()
    }
}

/// The collider view's primitives' sizes and limits.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColliderDrawTuning {
    /// How long a contact flashes, seconds.
    pub(crate) flash_s: f32,
    /// A flash's ring diameter at no impulse, metres.
    pub(crate) flash_ring_m: f32,
    /// How much a flash's ring grows per newton second of the contact's impulse, metres.
    pub(crate) flash_ring_per_ns_m: f32,
    /// The impulse past which a flash's ring grows no more, newton seconds.
    pub(crate) flash_max_impulse_ns: f32,
    /// A flash's ring thickness, as a share of its radius.
    pub(crate) flash_ring_thickness: f32,
    /// A body's box's line width, metres.
    pub(crate) box_width_m: f32,
    /// The line width of a flashed or gazed body's box, as a multiple of [`Self::box_width_m`].
    pub(crate) highlight_box_width: f32,
    /// The line width of the player's ground's box, as a multiple of [`Self::box_width_m`].
    pub(crate) ground_box_width: f32,
    /// A body's shape's line width, metres.
    pub(crate) shape_width_m: f32,
    /// A sphere's or a capsule end's ring thickness, as a share of its radius.
    pub(crate) shape_ring_thickness: f32,
    /// A joint's and the ground normal's line width, metres.
    pub(crate) joint_width_m: f32,
    /// A joint's end disc's diameter, metres.
    pub(crate) joint_end_m: f32,
    /// A face-less hull's vertex disc's diameter, metres.
    pub(crate) point_m: f32,
    /// The ground normal's length, metres.
    pub(crate) normal_m: f32,
    /// The most edges of one hull drawn.
    pub(crate) max_hull_edges: usize,
    /// The most vertices of one hull without faces drawn.
    pub(crate) max_points: usize,
}

impl ColliderDrawTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("collider_view.draw.flash_s", self.flash_s, 60.0)?;
        nonnegative("collider_view.draw.flash_ring_m", self.flash_ring_m, 10.0)?;
        nonnegative(
            "collider_view.draw.flash_ring_per_ns_m",
            self.flash_ring_per_ns_m,
            10.0,
        )?;
        nonnegative(
            "collider_view.draw.flash_max_impulse_ns",
            self.flash_max_impulse_ns,
            1.0e6,
        )?;
        thickness(
            "collider_view.draw.flash_ring_thickness",
            self.flash_ring_thickness,
        )?;
        positive("collider_view.draw.box_width_m", self.box_width_m, 1.0)?;
        positive(
            "collider_view.draw.highlight_box_width",
            self.highlight_box_width,
            100.0,
        )?;
        positive(
            "collider_view.draw.ground_box_width",
            self.ground_box_width,
            100.0,
        )?;
        positive("collider_view.draw.shape_width_m", self.shape_width_m, 1.0)?;
        thickness(
            "collider_view.draw.shape_ring_thickness",
            self.shape_ring_thickness,
        )?;
        positive("collider_view.draw.joint_width_m", self.joint_width_m, 1.0)?;
        positive("collider_view.draw.joint_end_m", self.joint_end_m, 1.0)?;
        positive("collider_view.draw.point_m", self.point_m, 1.0)?;
        nonnegative("collider_view.draw.normal_m", self.normal_m, 10.0)?;
        count(
            "collider_view.draw.max_hull_edges",
            self.max_hull_edges,
            0,
            PRIMITIVE_CEILING,
        )?;
        count(
            "collider_view.draw.max_points",
            self.max_points,
            0,
            PRIMITIVE_CEILING,
        )
    }
}

/// The grab view's reach, budgets, and timing.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrabViewTuning {
    /// How long the view's overlay slot lives, milliseconds: the view is submitted at the input
    /// tick's rate, so it lives a little past one tick.
    pub ttl_ms: u64,
    /// How long the palms' last tick is drawn after it ran, milliseconds: they tick every
    /// simulation tick while they act, so an older record means they have stopped.
    pub(crate) record_max_age_ms: u64,
    /// How old the palms' last tick may be and still describe the hands, milliseconds.
    pub(crate) palm_stale_ms: u64,
    /// How often the world is read for candidates, milliseconds: a pass over every body slot, a
    /// few times a second rather than every tick.
    pub(crate) candidate_period_ms: u64,
    /// How long the candidates are drawn after their read, milliseconds, past which they are taken
    /// as stale.
    pub(crate) candidate_max_age_ms: u64,
    /// How far from a hand a candidate's box may be and still be drawn, metres.
    pub(crate) candidate_radius_m: f32,
    /// The most bodies a read keeps, the nearest the hands first, before they are told apart.
    pub(crate) scan_max_bodies: u32,
    /// The most candidates drawn, the nearest first.
    pub(crate) max_candidates: usize,
    /// The most refused grabs flashing at once.
    pub(crate) max_flashes: usize,
    /// How long a refused grab flashes, seconds.
    pub(crate) refusal_flash_s: f32,
    pub draw: GrabDrawTuning,
}

impl GrabViewTuning {
    /// [`Self::ttl_ms`] as a duration.
    pub fn ttl(&self) -> Duration {
        Duration::from_millis(self.ttl_ms)
    }

    /// [`Self::record_max_age_ms`] as a freshness window.
    pub(crate) fn record_max_age(&self) -> Window {
        Window::from_millis(self.record_max_age_ms)
    }

    /// [`Self::candidate_period_ms`] as a duration.
    pub(crate) fn candidate_period(&self) -> Duration {
        Duration::from_millis(self.candidate_period_ms)
    }

    /// [`Self::candidate_max_age_ms`] as a freshness window.
    pub(crate) fn candidate_max_age(&self) -> Window {
        Window::from_millis(self.candidate_max_age_ms)
    }

    fn validate(&self) -> Result<(), Invalid> {
        count("grab_view.ttl_ms", self.ttl_ms, 1, 10_000)?;
        count(
            "grab_view.record_max_age_ms",
            self.record_max_age_ms,
            0,
            60_000,
        )?;
        count("grab_view.palm_stale_ms", self.palm_stale_ms, 0, 60_000)?;
        count(
            "grab_view.candidate_period_ms",
            self.candidate_period_ms,
            0,
            60_000,
        )?;
        count(
            "grab_view.candidate_max_age_ms",
            self.candidate_max_age_ms,
            0,
            60_000,
        )?;
        positive(
            "grab_view.candidate_radius_m",
            self.candidate_radius_m,
            100.0,
        )?;
        count("grab_view.scan_max_bodies", self.scan_max_bodies, 1, 65_536)?;
        count("grab_view.max_candidates", self.max_candidates, 0, 4096)?;
        count("grab_view.max_flashes", self.max_flashes, 1, 4096)?;
        positive("grab_view.refusal_flash_s", self.refusal_flash_s, 60.0)?;
        self.draw.validate()
    }
}

/// The grab view's primitives' sizes and limits.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrabDrawTuning {
    /// The most primitives the view submits in a tick.
    pub primitive_budget: usize,
    /// A refusal's flash at its largest, metres.
    pub(crate) refusal_mark_m: f32,
    /// A refusal's flash's size as it starts, as a share of [`Self::refusal_mark_m`]: it grows to
    /// the whole as it fades.
    pub(crate) refusal_start_share: f32,
    /// A refusal's ring thickness, as a share of its radius.
    pub(crate) refusal_ring_thickness: f32,
    /// A reach ray's line width, metres.
    pub(crate) ray_width_m: f32,
    /// A reach ray's hit disc's diameter, metres.
    pub(crate) hit_mark_m: f32,
    /// A door handle's ring diameter, metres.
    pub(crate) handle_mark_m: f32,
    /// A door handle's ring thickness, as a share of its radius.
    pub handle_ring_thickness: f32,
    /// A line from a hand to a grip target's width, metres.
    pub(crate) target_line_width_m: f32,
    /// The smallest a target's end marker is drawn, metres.
    pub(crate) min_target_mark_m: f32,
    /// The largest a target's end marker is drawn, metres.
    pub(crate) max_target_mark_m: f32,
    /// The ring thickness of the end marker of a target in reach or out of it, as a share of its
    /// radius.
    pub(crate) target_ring_thickness: f32,
    /// The most target lines per hand.
    pub(crate) max_target_lines: usize,
    /// How far from a hand a grip target out of its reach still gets a line, metres.
    pub(crate) target_radius_m: f32,
    /// A candidate's box's and its line's width, metres.
    pub(crate) box_width_m: f32,
    /// The magazine zone's and the knock zone's axis width, metres.
    pub(crate) zone_line_width_m: f32,
    /// The magazine zone's end rings' thickness, as a share of their radius.
    pub(crate) zone_ring_thickness: f32,
    /// The handover reach's ring thickness, as a share of its radius.
    pub(crate) handover_ring_thickness: f32,
    /// The knock zone's top ring's thickness, as a share of its radius.
    pub(crate) knock_top_ring_thickness: f32,
    /// The knock zone's base ring's thickness, as a share of its radius.
    pub(crate) knock_base_ring_thickness: f32,
    /// The held tip's marker diameter, metres at the player's scale.
    pub(crate) knock_tip_m: f32,
}

impl GrabDrawTuning {
    fn validate(&self) -> Result<(), Invalid> {
        count(
            "grab_view.draw.primitive_budget",
            self.primitive_budget,
            0,
            PRIMITIVE_CEILING,
        )?;
        positive("grab_view.draw.refusal_mark_m", self.refusal_mark_m, 10.0)?;
        range(
            "grab_view.draw.refusal_start_share",
            self.refusal_start_share,
            0.0,
            1.0,
        )?;
        thickness(
            "grab_view.draw.refusal_ring_thickness",
            self.refusal_ring_thickness,
        )?;
        positive("grab_view.draw.ray_width_m", self.ray_width_m, 1.0)?;
        positive("grab_view.draw.hit_mark_m", self.hit_mark_m, 1.0)?;
        positive("grab_view.draw.handle_mark_m", self.handle_mark_m, 1.0)?;
        thickness(
            "grab_view.draw.handle_ring_thickness",
            self.handle_ring_thickness,
        )?;
        positive(
            "grab_view.draw.target_line_width_m",
            self.target_line_width_m,
            1.0,
        )?;
        positive(
            "grab_view.draw.min_target_mark_m",
            self.min_target_mark_m,
            1.0,
        )?;
        range(
            "grab_view.draw.max_target_mark_m",
            self.max_target_mark_m,
            self.min_target_mark_m,
            1.0,
        )?;
        thickness(
            "grab_view.draw.target_ring_thickness",
            self.target_ring_thickness,
        )?;
        count(
            "grab_view.draw.max_target_lines",
            self.max_target_lines,
            0,
            4096,
        )?;
        positive(
            "grab_view.draw.target_radius_m",
            self.target_radius_m,
            100.0,
        )?;
        positive("grab_view.draw.box_width_m", self.box_width_m, 1.0)?;
        positive(
            "grab_view.draw.zone_line_width_m",
            self.zone_line_width_m,
            1.0,
        )?;
        thickness(
            "grab_view.draw.zone_ring_thickness",
            self.zone_ring_thickness,
        )?;
        thickness(
            "grab_view.draw.handover_ring_thickness",
            self.handover_ring_thickness,
        )?;
        thickness(
            "grab_view.draw.knock_top_ring_thickness",
            self.knock_top_ring_thickness,
        )?;
        thickness(
            "grab_view.draw.knock_base_ring_thickness",
            self.knock_base_ring_thickness,
        )?;
        positive("grab_view.draw.knock_tip_m", self.knock_tip_m, 1.0)
    }
}

/// Check that a ring's `thickness`, a share of its radius, is in `(0, 1]`.
fn thickness(name: &str, value: f32) -> Result<(), Invalid> {
    positive(name, value, 1.0)
}

/// Check that a count or a whole number of milliseconds is in `[min, max]`.
fn count<T: PartialOrd + fmt::Display>(
    name: &str,
    value: T,
    min: T,
    max: T,
) -> Result<(), Invalid> {
    if value < min || value > max {
        return Err(Invalid(format!(
            "{name} must be in [{min}, {max}], got {value}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(VIEWS.get().validate(), Ok(()));
    }
}
