//! Conservative physical-component attribution of possible bindings, never inferred contributors.

use tracking_format::input::SourceQuery;

use crate::{
    controllers::{bindings::policy_a::Control, frame::Hand},
    vr::input::semantic::bound::Physical,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct SourceIdentity {
    pub(super) hand: Hand,
    pub(super) physical: Physical,
}

impl SourceIdentity {
    pub(super) fn bit(self) -> u32 {
        1 << (self.hand.index() * 8 + self.physical as usize)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Attribution {
    pub(super) identities: Vec<SourceIdentity>,
    pub(super) controls: [bool; 3],
    pub(super) unresolved: bool,
}

impl Attribution {
    pub(super) fn of(query: &SourceQuery, hand: Hand) -> Self {
        let mut out = Self {
            identities: Vec::new(),
            controls: [false; 3],
            unresolved: false,
        };
        let SourceQuery::Paths(paths) = query else {
            out.unresolved = true;
            return out;
        };
        for path in paths.iter() {
            match identity(path) {
                Some(source) if source.hand == hand => {
                    if !out.identities.contains(&source) {
                        out.identities.push(source);
                    }
                    if let Some(control) = protected(source) {
                        out.controls[control.index()] = true;
                    }
                }
                Some(_) => {}
                None => out.unresolved = true,
            }
        }
        out
    }

    pub(super) fn single_control(&self) -> Option<Control> {
        if self.unresolved || self.identities.len() != 1 {
            return None;
        }
        Control::ALL
            .into_iter()
            .find(|control| self.controls[control.index()])
    }

    pub(super) fn mask(&self) -> u32 {
        self.identities
            .iter()
            .fold(0, |mask, source| mask | source.bit())
    }
    pub(super) fn claim_mask(&self, hand: Hand) -> u32 {
        if self.unresolved {
            0xff << (hand.index() * 8)
        } else {
            self.mask()
        }
    }
    pub(super) fn intersects(&self, mask: u32) -> bool {
        self.mask() & mask != 0
    }
}

pub(super) fn raw_verified(query: &SourceQuery, control: Control) -> bool {
    let (hand, physical) = raw_location(control);
    let SourceQuery::Paths(paths) = query else {
        return false;
    };
    let mut found = 0;
    for path in paths.iter() {
        match identity(path) {
            Some(source) if source.hand == hand => {
                let suffix = match control {
                    Control::LeftStick | Control::RightStick => "/input/thumbstick/click",
                    Control::RightB => "/input/b/click",
                };
                if source.physical != physical || !path.ends_with(suffix) {
                    return false;
                }
                found += 1;
            }
            Some(_) => {}
            None => return false,
        }
    }
    found == 1
}

pub(super) fn raw_location(control: Control) -> (Hand, Physical) {
    match control {
        Control::LeftStick => (Hand::Left, Physical::ThumbstickClick),
        Control::RightStick => (Hand::Right, Physical::ThumbstickClick),
        Control::RightB => (Hand::Right, Physical::UpperFace),
    }
}

fn identity(path: &str) -> Option<SourceIdentity> {
    let (hand, component) = if let Some(rest) = path.strip_prefix("/user/hand/left/input/") {
        (Hand::Left, rest)
    } else {
        (Hand::Right, path.strip_prefix("/user/hand/right/input/")?)
    };
    let physical = match component {
        "trigger/click" | "trigger/value" | "trigger/touch" => Physical::Trigger,
        "squeeze/click" | "squeeze/value" | "squeeze/force" => Physical::Grip,
        "thumbstick" | "thumbstick/x" | "thumbstick/y" | "thumbstick/touch" => Physical::Thumbstick,
        "thumbstick/click" => Physical::ThumbstickClick,
        "trackpad" | "trackpad/x" | "trackpad/y" | "trackpad/click" | "trackpad/touch"
        | "trackpad/force" => Physical::Trackpad,
        "a/click" | "x/click" | "a/touch" | "x/touch" => Physical::LowerFace,
        "b/click" | "y/click" | "b/touch" | "y/touch" => Physical::UpperFace,
        "system/click" | "menu/click" => Physical::System,
        _ => return None,
    };
    Some(SourceIdentity { hand, physical })
}

fn protected(source: SourceIdentity) -> Option<Control> {
    match (source.hand, source.physical) {
        (Hand::Left, Physical::ThumbstickClick) => Some(Control::LeftStick),
        (Hand::Right, Physical::ThumbstickClick) => Some(Control::RightStick),
        (Hand::Right, Physical::UpperFace) => Some(Control::RightB),
        _ => None,
    }
}
