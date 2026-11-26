//! Conservative physical-component attribution of possible bindings, never inferred contributors.
//!
//! A source path the arbiter recognises is checked strictly. An opaque enumeration
//! ([`bound::opaque`]), where the runtime names its sources under no user path, counts as a failed
//! one, as it does for the binding tables' masks: the action's default binding on the hand stands
//! in as its attribution, and a raw protected route, which the mod owns and players are asked not
//! to rebind, is taken at its fixed physical location.

use tracking_format::input::SourceQuery;

use crate::{
    controllers::{
        bindings::policy_a::{CONTROLS, Control},
        frame::Hand,
    },
    vr::input::semantic::{
        bound::{self, Physical},
        defaults::default_binding,
        vocabulary::ActionId,
    },
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
    pub(super) controls: [bool; CONTROLS],
    pub(super) unresolved: bool,
}

impl Attribution {
    /// An attribution of sources not yet enumerated, which claims its whole hand.
    pub(super) fn unresolved() -> Self {
        Self {
            identities: Vec::new(),
            controls: [false; CONTROLS],
            unresolved: true,
        }
    }

    /// The attribution of action `id` on `hand` from its enumerated sources, for a player whose
    /// dominant hand is `dominant` (which places the default binding of an opaque enumeration).
    pub(super) fn of(query: &SourceQuery, id: ActionId, hand: Hand, dominant: Hand) -> Self {
        let mut out = Self {
            identities: Vec::new(),
            controls: [false; CONTROLS],
            unresolved: false,
        };
        let SourceQuery::Paths(paths) = query else {
            out.unresolved = true;
            return out;
        };
        if bound::opaque(paths.iter().map(String::as_str)) {
            if let Some(physical) = default_physical(id, hand, dominant) {
                out.add(SourceIdentity { hand, physical });
            }
            return out;
        }
        for path in paths.iter() {
            match identity(path) {
                Some(source) if source.hand == hand => out.add(source),
                Some(_) => {}
                None => out.unresolved = true,
            }
        }
        out
    }

    fn add(&mut self, source: SourceIdentity) {
        if !self.identities.contains(&source) {
            self.identities.push(source);
        }
        if let Some(control) = protected(source) {
            self.controls[control.index()] = true;
        }
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
    if bound::opaque(paths.iter().map(String::as_str)) {
        return true;
    }
    let mut found = 0;
    for path in paths.iter() {
        match identity(path) {
            Some(source) if source.hand == hand => {
                let suffix = match control {
                    Control::LeftStick | Control::RightStick => "/input/thumbstick/click",
                    Control::RightB => "/input/b/click",
                    Control::RightA => "/input/a/click",
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
        Control::RightA => (Hand::Right, Physical::LowerFace),
    }
}

/// The physical input the default binding of action `id` reads on `hand`, if it covers the hand.
fn default_physical(id: ActionId, hand: Hand, dominant: Hand) -> Option<Physical> {
    default_binding(id.set, id.index())
        .filter(|binding| binding.side.covers(hand, dominant))
        .map(|binding| binding.component.physical())
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
        (Hand::Right, Physical::LowerFace) => Some(Control::RightA),
        _ => None,
    }
}
