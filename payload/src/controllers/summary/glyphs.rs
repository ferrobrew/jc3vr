//! The button glyphs: Kenney's Input Prompts (CC0, `payload/assets/glyphs/kenney-input-prompts`),
//! the Valve Index set, with the grips from the same pack's Steam Frame set, since the Index set has
//! no grip glyph. Embedded in the payload. Pure.

use crate::controllers::{
    bindings::tables::describe::{Control, StickDir},
    frame::Hand,
};

/// Embed a glyph PNG by its file stem.
macro_rules! glyph {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/glyphs/kenney-input-prompts/",
            $name,
            ".png"
        ))
    };
}

/// One glyph image.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Glyph {
    TriggerLeft,
    TriggerRight,
    GripLeft,
    GripRight,
    ButtonA,
    ButtonB,
    System,
    StickLeft,
    StickLeftVertical,
    StickLeftHorizontal,
    StickLeftDown,
    StickLeftPress,
    StickRight,
    StickRightVertical,
    StickRightHorizontal,
    StickRightDown,
    StickRightPress,
    Trackpad,
    ControllerLeft,
    ControllerRight,
}

impl Glyph {
    pub const ALL: [Glyph; 20] = [
        Glyph::TriggerLeft,
        Glyph::TriggerRight,
        Glyph::GripLeft,
        Glyph::GripRight,
        Glyph::ButtonA,
        Glyph::ButtonB,
        Glyph::System,
        Glyph::StickLeft,
        Glyph::StickLeftVertical,
        Glyph::StickLeftHorizontal,
        Glyph::StickLeftDown,
        Glyph::StickLeftPress,
        Glyph::StickRight,
        Glyph::StickRightVertical,
        Glyph::StickRightHorizontal,
        Glyph::StickRightDown,
        Glyph::StickRightPress,
        Glyph::Trackpad,
        Glyph::ControllerLeft,
        Glyph::ControllerRight,
    ];

    /// The glyph's PNG.
    pub const fn png(self) -> &'static [u8] {
        match self {
            Glyph::TriggerLeft => glyph!("index_lt"),
            Glyph::TriggerRight => glyph!("index_rt"),
            Glyph::GripLeft => glyph!("frame_lg"),
            Glyph::GripRight => glyph!("frame_rg"),
            Glyph::ButtonA => glyph!("index_button_a"),
            Glyph::ButtonB => glyph!("index_button_b"),
            Glyph::System => glyph!("index_button"),
            Glyph::StickLeft => glyph!("index_stick_l"),
            Glyph::StickLeftVertical => glyph!("index_stick_l_vertical"),
            Glyph::StickLeftHorizontal => glyph!("index_stick_l_horizontal"),
            Glyph::StickLeftDown => glyph!("index_stick_l_down"),
            Glyph::StickLeftPress => glyph!("index_stick_l_press"),
            Glyph::StickRight => glyph!("index_stick_r"),
            Glyph::StickRightVertical => glyph!("index_stick_r_vertical"),
            Glyph::StickRightHorizontal => glyph!("index_stick_r_horizontal"),
            Glyph::StickRightDown => glyph!("index_stick_r_down"),
            Glyph::StickRightPress => glyph!("index_stick_r_press"),
            Glyph::Trackpad => glyph!("index_track"),
            Glyph::ControllerLeft => glyph!("controller_index_left"),
            Glyph::ControllerRight => glyph!("controller_index_right"),
        }
    }

    /// The glyph for a whole controller, heading its hand's group.
    pub const fn controller(hand: Hand) -> Glyph {
        match hand {
            Hand::Left => Glyph::ControllerLeft,
            Hand::Right => Glyph::ControllerRight,
        }
    }
}

/// The glyphs drawn for `control`: one, or one per hand for a control of either hand.
pub fn glyphs(control: Control) -> &'static [Glyph] {
    let sided = |left: &'static [Glyph], right: &'static [Glyph], hand: Hand| match hand {
        Hand::Left => left,
        Hand::Right => right,
    };
    match control {
        Control::Trigger(hand) => sided(&[Glyph::TriggerLeft], &[Glyph::TriggerRight], hand),
        Control::Grip(hand) => sided(&[Glyph::GripLeft], &[Glyph::GripRight], hand),
        Control::A(_) => &[Glyph::ButtonA],
        Control::B(_) => &[Glyph::ButtonB],
        Control::Stick(hand, dir) => match dir {
            StickDir::Any => sided(&[Glyph::StickLeft], &[Glyph::StickRight], hand),
            StickDir::Vertical => sided(
                &[Glyph::StickLeftVertical],
                &[Glyph::StickRightVertical],
                hand,
            ),
            StickDir::Horizontal => sided(
                &[Glyph::StickLeftHorizontal],
                &[Glyph::StickRightHorizontal],
                hand,
            ),
            StickDir::Down => sided(&[Glyph::StickLeftDown], &[Glyph::StickRightDown], hand),
        },
        Control::StickClick(hand) => {
            sided(&[Glyph::StickLeftPress], &[Glyph::StickRightPress], hand)
        }
        // The Index trackpad glyph is not sided, so either hand's trackpad draws it once.
        Control::Trackpad(_) | Control::EitherTrackpad => &[Glyph::Trackpad],
        Control::Motion(hand) => sided(&[Glyph::ControllerLeft], &[Glyph::ControllerRight], hand),
        Control::EitherGrip => &[Glyph::GripLeft, Glyph::GripRight],
        Control::EitherMotion => &[Glyph::ControllerLeft, Glyph::ControllerRight],
        Control::System => &[Glyph::System],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_a_png() {
        for glyph in Glyph::ALL {
            assert!(glyph.png().starts_with(b"\x89PNG"), "{glyph:?}");
        }
    }

    #[test]
    fn sided_controls_take_their_hand_and_shared_ones_both() {
        assert_eq!(glyphs(Control::Trigger(Hand::Left)), &[Glyph::TriggerLeft]);
        assert_eq!(
            glyphs(Control::Stick(Hand::Right, StickDir::Horizontal)),
            &[Glyph::StickRightHorizontal]
        );
        assert_eq!(glyphs(Control::EitherGrip).len(), 2);
    }
}
