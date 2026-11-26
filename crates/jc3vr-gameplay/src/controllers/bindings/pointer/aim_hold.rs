//! The aim hold of a pointer's trigger: a light pull freezes the cursor where it is, and only a full
//! pull clicks.
//!
//! A hand-held ray wanders, and the trigger pull itself jerks it, so a click taken at the press
//! threshold lands a little off where the player aimed. With the hold, the player settles the
//! cursor on a target, rests a finger on the trigger to pin it there, and commits with a full pull
//! wherever the ray has drifted meanwhile. Once the click begins, the cursor moves with the ray
//! again, offset by where it was pinned, so a drag carries on from the pinned point rather than
//! jumping to the live ray. Letting the trigger go frees the cursor.

use jc3vr_view::hud::pointer::panel::PanelHit;

use crate::controllers::tuning::GAMEPLAY;

/// One pointing hand's aim hold.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct AimHold {
    pinned: Option<Pinned>,
    full: bool,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Pinned {
    /// The hit where the cursor was pinned.
    hit: PanelHit,
    /// The pinned point less the live hit when the click began, while a click is held.
    offset: Option<(f32, f32)>,
}

impl AimHold {
    pub(super) const fn new() -> Self {
        Self {
            pinned: None,
            full: false,
        }
    }

    /// Fold in one tick: the live hit, whether the pointer's press is down, and the trigger's
    /// value. Returns the hit the pointer should use and whether its press is down. A press reads
    /// as down only at a full pull, so a light pull aims without clicking. A press reported with
    /// the trigger at rest is a tap that began and ended between two ticks, and clicks at the live
    /// ray.
    pub(super) fn step(
        &mut self,
        hit: Option<PanelHit>,
        down: bool,
        value: f32,
    ) -> (Option<PanelHit>, bool) {
        let tuning = &GAMEPLAY.get().pointer.aim_hold;
        let value = if value.is_finite() { value } else { 0.0 };
        self.full = if self.full {
            value >= tuning.click_release
        } else {
            value >= tuning.click_press
        };
        let tap = down && value < tuning.pin_release;
        let down = down && (self.full || tap);
        if value < tuning.pin_release {
            self.pinned = None;
        } else if self.pinned.is_none() && value >= tuning.pin_press && !down {
            self.pinned = hit.map(|hit| Pinned { hit, offset: None });
        }
        let Some(pinned) = &mut self.pinned else {
            return (hit, down);
        };
        if !down {
            pinned.offset = None;
            // The pinned cursor stays where it was even when the ray leaves the panel's plane.
            let distance = hit.map_or(pinned.hit.distance, |hit| hit.distance);
            return (
                Some(PanelHit {
                    distance,
                    ..pinned.hit
                }),
                false,
            );
        }
        let Some(live) = hit else {
            return (None, true);
        };
        let offset = *pinned
            .offset
            .get_or_insert((pinned.hit.uv.0 - live.uv.0, pinned.hit.uv.1 - live.uv.1));
        let uv = (live.uv.0 + offset.0, live.uv.1 + offset.1);
        let inside = (0.0..=1.0).contains(&uv.0) && (0.0..=1.0).contains(&uv.1);
        (
            Some(PanelHit {
                uv,
                inside,
                distance: live.distance,
            }),
            true,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(u: f32, v: f32) -> Option<PanelHit> {
        Some(PanelHit {
            uv: (u, v),
            inside: true,
            distance: 1.0,
        })
    }

    #[test]
    fn a_light_pull_pins_the_cursor_without_clicking() {
        let mut hold = AimHold::new();
        assert_eq!(hold.step(hit(0.5, 0.5), false, 0.0), (hit(0.5, 0.5), false));
        assert_eq!(hold.step(hit(0.5, 0.5), false, 0.3), (hit(0.5, 0.5), false));
        assert_eq!(hold.step(hit(0.6, 0.4), true, 0.8), (hit(0.5, 0.5), false));
    }

    #[test]
    fn a_full_pull_clicks_at_the_pinned_point_and_drags_from_it() {
        let mut hold = AimHold::new();
        hold.step(hit(0.5, 0.5), false, 0.3);
        assert_eq!(hold.step(hit(0.6, 0.5), true, 1.0), (hit(0.5, 0.5), true));
        let (dragged, down) = hold.step(hit(0.7, 0.5), true, 1.0);
        assert!(down);
        let uv = dragged.unwrap().uv;
        assert!((uv.0 - 0.6).abs() < 1e-6 && (uv.1 - 0.5).abs() < 1e-6);
    }

    #[test]
    fn letting_go_frees_the_cursor() {
        let mut hold = AimHold::new();
        hold.step(hit(0.5, 0.5), false, 0.3);
        hold.step(hit(0.5, 0.5), true, 1.0);
        assert_eq!(hold.step(hit(0.8, 0.2), false, 0.0), (hit(0.8, 0.2), false));
    }

    #[test]
    fn the_click_holds_until_the_pull_falls_below_its_release() {
        let mut hold = AimHold::new();
        hold.step(hit(0.5, 0.5), false, 0.3);
        assert!(hold.step(hit(0.5, 0.5), true, 0.96).1);
        assert!(hold.step(hit(0.5, 0.5), true, 0.85).1);
        assert!(!hold.step(hit(0.5, 0.5), true, 0.7).1);
    }

    #[test]
    fn a_pinned_cursor_survives_the_ray_leaving_the_plane() {
        let mut hold = AimHold::new();
        hold.step(hit(0.5, 0.5), false, 0.3);
        assert_eq!(hold.step(None, false, 0.3), (hit(0.5, 0.5), false));
    }

    #[test]
    fn a_tap_between_ticks_clicks_at_the_live_ray() {
        let mut hold = AimHold::new();
        assert_eq!(hold.step(hit(0.4, 0.4), true, 0.0), (hit(0.4, 0.4), true));
    }

    #[test]
    fn a_pull_straight_to_full_clicks_at_the_live_ray() {
        let mut hold = AimHold::new();
        assert_eq!(hold.step(hit(0.4, 0.4), true, 1.0), (hit(0.4, 0.4), true));
    }
}
