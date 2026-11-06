//! The docked guns' in-world diagnostic: each gun the loadout table has on a socket of the body,
//! marked where its last placement put it, with a short line along its barrel, green while the gun's
//! model is shown and red while it is hidden. It tells apart a docked gun the game hides from one
//! placed off the body or turned into it (`docs/mod/input/two-handed-inventory.md`, "The docked
//! guns' view").

use std::time::Duration;

use glam::Vec3;
use jc3gi::{character::character::Character, graphics_engine::model_instance::ModelInstanceFlags};

use crate::{
    config::Config,
    controllers::{
        aim::gun,
        loadout::live,
        overlay::{
            primitive::{Anchor, DepthMode, Facing, Primitive, Rgba, Shape, Source, SpriteSize},
            store,
        },
        weapons::docks,
    },
};

/// Submit the docked guns' marks for `owner`, or clear them while the view is off.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update.
pub(crate) unsafe fn submit(owner: &Character) {
    let on =
        Config::lock_query(|c| c.controllers.overlay.enabled && c.controllers.overlay.docked_guns);
    let Some(snapshot) = live::snapshot_now().filter(|_| on) else {
        store::clear(Source::DockedGuns);
        return;
    };
    let mut primitives = Vec::new();
    // SAFETY: forwarded from the caller; a slot or the carry set keeps each weapon alive.
    for (_, weapon) in unsafe { docks::socket_weapons(owner, &snapshot) } {
        for gun in unsafe { gun::guns(weapon) }.flatten() {
            let matrix = gun.m_WorldMatrixT1.data;
            let at = Vec3::new(matrix[12], matrix[13], matrix[14]);
            let forward = -Vec3::new(matrix[8], matrix[9], matrix[10]);
            // SAFETY: the gun's model lives as long as the gun.
            let shown = unsafe { gun.m_BaseModelInstance.as_ref() }
                .is_some_and(|model| model.m_Flags.contains(ModelInstanceFlags::VISIBLE));
            primitives.extend(marks(at, forward, shown));
        }
    }
    if primitives.is_empty() {
        store::clear(Source::DockedGuns);
    } else {
        store::submit(Source::DockedGuns, &mut primitives, TTL);
    }
}

/// How long a submission is drawn without a newer one.
const TTL: Duration = Duration::from_millis(150);
/// The ring's diameter, metres.
const RING_M: f32 = 0.04;
/// The barrel line's length, metres.
const BARREL_M: f32 = 0.15;
const SHOWN: Rgba = [0.3, 1.0, 0.4, 0.9];
const HIDDEN: Rgba = [1.0, 0.25, 0.2, 0.9];
/// Seen through the body, which a gun turned into the thigh is behind.
const DEPTH: DepthMode = DepthMode::XRay {
    occluded_alpha: 0.5,
};

/// The marks for a gun at `at` whose barrel points along `forward`, shown or hidden. A gun with a
/// degenerate matrix gets no barrel line.
fn marks(at: Vec3, forward: Vec3, shown: bool) -> Vec<Primitive> {
    if !at.is_finite() {
        return Vec::new();
    }
    let color = if shown { SHOWN } else { HIDDEN };
    let mut marks = vec![Primitive::Sprite {
        at: Anchor::World(at),
        size: SpriteSize::World(RING_M),
        facing: Facing::Billboard,
        shape: Shape::Ring { thickness: 0.3 },
        color,
        depth: DEPTH,
    }];
    if let Some(direction) = forward.try_normalize() {
        marks.push(Primitive::Line {
            from: Anchor::World(at),
            to: Anchor::World(at + direction * BARREL_M),
            width: 0.004,
            color,
            depth: DEPTH,
        });
    }
    marks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shown_gun_gets_a_green_ring_and_a_barrel_line() {
        let marks = marks(Vec3::new(1.0, 2.0, 3.0), Vec3::Z * 2.0, true);
        assert_eq!(marks.len(), 2);
        let Primitive::Line {
            from, to, color, ..
        } = marks[1]
        else {
            panic!("the second mark is the barrel line");
        };
        assert_eq!(color, SHOWN);
        assert_eq!(from, Anchor::World(Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(to, Anchor::World(Vec3::new(1.0, 2.0, 3.0 + BARREL_M)));
    }

    #[test]
    fn a_hidden_gun_is_red_and_a_degenerate_one_has_no_line() {
        let marks = marks(Vec3::ZERO, Vec3::ZERO, false);
        assert_eq!(marks.len(), 1);
        let Primitive::Sprite { color, .. } = marks[0] else {
            panic!("the mark is the ring");
        };
        assert_eq!(color, HIDDEN);
        assert!(super::marks(Vec3::NAN, Vec3::Z, true).is_empty());
    }
}
