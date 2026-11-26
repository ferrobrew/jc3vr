//! The two sides of the body, for a foot, an arm, or a shoulder.

use glam::Vec3;

/// A side of the body. The left is index 0 in the per-side arrays.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    /// Both sides, in index order.
    pub const BOTH: [Side; 2] = [Side::Left, Side::Right];

    /// The index of this side in the per-side arrays.
    pub const fn index(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
        }
    }

    /// The other side.
    pub const fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }

    /// Which way along the body's right this side lies from the centre: -1 left, 1 right.
    pub const fn across(self) -> f32 {
        match self {
            Side::Left => -1.0,
            Side::Right => 1.0,
        }
    }

    /// The side's lowercase name, for logs and the debug UI.
    pub const fn label(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }

    /// A vector given for the right side, as it is for this side: the left mirrors it across X.
    pub fn mirror_from_right(self, v: Vec3) -> Vec3 {
        match self {
            Side::Left => Vec3::new(-v.x, v.y, v.z),
            Side::Right => v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_left_mirrors_what_is_given_for_the_right() {
        for side in Side::BOTH {
            assert_eq!(side.mirror_from_right(Vec3::X).x, side.across());
        }
        let v = Vec3::new(0.2, 0.3, 0.4);
        assert_eq!(Side::Right.mirror_from_right(v), v);
        assert_eq!(Side::Left.mirror_from_right(v), Vec3::new(-0.2, 0.3, 0.4));
    }
}
