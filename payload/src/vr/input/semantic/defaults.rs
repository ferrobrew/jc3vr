//! The semantic actions' suggested bindings on the shipped interaction profiles.
//!
//! The table is written once, per action, as a side (the dominant hand, the off-hand, both hands,
//! or a fixed physical hand) and a [`Component`] named by its position on the controller, which
//! resolves to a component path per profile. It encodes the layout the binding tables
//! (`controllers::bindings::tables`) use today, so a semantic action reads the same physical input
//! as the raw input the tables read for that meaning.
//!
//! The right-handed table is resolved directly; the left-handed one is generated from it by
//! [`mirror`], which moves every row that follows the handedness to the other hand. Rows on both
//! hands or on a fixed hand (the menu's previous and next pages, for example, keep their physical
//! sides) are left as they are. The table is chosen from `controllers.dominant_hand` when the
//! actions are created, since suggestions are fixed once the sets are attached.

use crate::{
    controllers::frame::Hand,
    vr::input::{
        layout::Profile,
        semantic::{
            SemanticSet,
            vocabulary::{
                GlobalAction, HelicopterAction, LandVehicleAction, MenuAction, OnFootAction,
                PlaneAction, WingsuitAction,
            },
        },
    },
};

/// The hand or hands a default binding is on.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Side {
    Dom,
    Off,
    Both,
    Left,
    Right,
}

impl Side {
    /// Whether this side includes `hand` for a player whose dominant hand is `dominant`.
    pub(crate) const fn covers(self, hand: Hand, dominant: Hand) -> bool {
        let dom = matches!(
            (hand, dominant),
            (Hand::Left, Hand::Left) | (Hand::Right, Hand::Right)
        );
        match self {
            Side::Dom => dom,
            Side::Off => !dom,
            Side::Both => true,
            Side::Left => matches!(hand, Hand::Left),
            Side::Right => matches!(hand, Hand::Right),
        }
    }

    /// Whether the side moves with the handedness, so the mirrored table swaps its hand.
    const fn follows_handedness(self) -> bool {
        matches!(self, Side::Dom | Side::Off)
    }
}

/// A physical input, named by its position on the controller.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Component {
    TriggerValue,
    /// The deliberate grip press: the squeeze force on a force-sensing grip (Index), whose
    /// capacitive squeeze value is full whenever the hand rests round the handle, and the squeeze
    /// value elsewhere.
    GripPress,
    Thumbstick,
    ThumbstickX,
    ThumbstickY,
    ThumbstickClick,
    Trackpad,
    TrackpadForce,
    TrackpadTouch,
    /// The lower face button: A, or X on the left Touch controller.
    LowerFace,
    /// The upper face button: B, or Y on the left Touch controller.
    UpperFace,
    /// The system button (Index), or the menu button on the left Touch controller. The right
    /// Touch controller's system button is reserved for the runtime.
    System,
}

impl Component {
    /// The component path, relative to the hand's user path, on `hand` of `profile`, or `None`
    /// when the controller lacks the input.
    pub(crate) const fn path(self, profile: Profile, hand: Hand) -> Option<&'static str> {
        let left = matches!(hand, Hand::Left);
        let touch = matches!(profile, Profile::OculusTouch);
        Some(match self {
            Component::TriggerValue => "input/trigger/value",
            Component::GripPress if touch => "input/squeeze/value",
            Component::GripPress => "input/squeeze/force",
            Component::Thumbstick => "input/thumbstick",
            Component::ThumbstickX => "input/thumbstick/x",
            Component::ThumbstickY => "input/thumbstick/y",
            Component::ThumbstickClick => "input/thumbstick/click",
            Component::Trackpad | Component::TrackpadForce | Component::TrackpadTouch if touch => {
                return None;
            }
            Component::Trackpad => "input/trackpad",
            Component::TrackpadForce => "input/trackpad/force",
            Component::TrackpadTouch => "input/trackpad/touch",
            Component::LowerFace if touch && left => "input/x/click",
            Component::LowerFace => "input/a/click",
            Component::UpperFace if touch && left => "input/y/click",
            Component::UpperFace => "input/b/click",
            Component::System if touch && left => "input/menu/click",
            Component::System if touch => return None,
            Component::System => "input/system/click",
        })
    }
}

/// An action's default binding.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DefaultBinding {
    pub(crate) side: Side,
    pub(crate) component: Component,
}

/// The default binding of action `index` of `set`, or `None` for an action that ships unbound.
pub(crate) fn default_binding(set: SemanticSet, index: usize) -> Option<DefaultBinding> {
    match set {
        SemanticSet::Global => global(GlobalAction::ALL[index]),
        SemanticSet::OnFoot => on_foot(OnFootAction::ALL[index]),
        SemanticSet::Wingsuit => wingsuit(WingsuitAction::ALL[index]),
        SemanticSet::LandVehicle => land_vehicle(LandVehicleAction::ALL[index]),
        SemanticSet::Helicopter => helicopter(HelicopterAction::ALL[index]),
        SemanticSet::Plane => plane(PlaneAction::ALL[index]),
        SemanticSet::Menu => menu(MenuAction::ALL[index]),
    }
}

/// One suggested binding: action `index` of `set` on `hand`, at `path` relative to the hand's
/// user path.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Suggestion {
    pub(crate) set: SemanticSet,
    pub(crate) index: usize,
    pub(crate) hand: Hand,
    pub(crate) path: &'static str,
    pub(crate) binding: DefaultBinding,
}

/// The suggested bindings on `profile` for a player whose dominant hand is `dominant`: the
/// right-handed table, or for a left-handed player that table [`mirror`]ed.
pub(crate) fn suggestions(profile: Profile, dominant: Hand) -> Vec<Suggestion> {
    let right_handed = resolve(profile, Hand::Right);
    match dominant {
        Hand::Right => right_handed,
        Hand::Left => mirror(profile, &right_handed),
    }
}

/// The default path of action `index` of `set` on `hand` of `profile`, for a player whose
/// dominant hand is `dominant`.
pub(crate) fn default_path(
    set: SemanticSet,
    index: usize,
    profile: Profile,
    hand: Hand,
    dominant: Hand,
) -> Option<&'static str> {
    let binding = default_binding(set, index)?;
    if !binding.side.covers(hand, dominant) {
        return None;
    }
    binding.component.path(profile, hand)
}

/// Resolve the table on `profile` for a player whose dominant hand is `dominant`.
pub(super) fn resolve(profile: Profile, dominant: Hand) -> Vec<Suggestion> {
    let mut out = Vec::new();
    for set in SemanticSet::ALL {
        for index in 0..set.actions().len() {
            let Some(binding) = default_binding(set, index) else {
                continue;
            };
            for hand in Hand::BOTH {
                if !binding.side.covers(hand, dominant) {
                    continue;
                }
                if let Some(path) = binding.component.path(profile, hand) {
                    out.push(Suggestion {
                        set,
                        index,
                        hand,
                        path,
                        binding,
                    });
                }
            }
        }
    }
    out
}

/// Mirror a resolved table: move every row that follows the handedness to the other hand, at the
/// same position on that hand's controller.
pub(super) fn mirror(profile: Profile, table: &[Suggestion]) -> Vec<Suggestion> {
    table
        .iter()
        .filter_map(|s| {
            if !s.binding.side.follows_handedness() {
                return Some(*s);
            }
            let hand = s.hand.other();
            let path = s.binding.component.path(profile, hand)?;
            Some(Suggestion { hand, path, ..*s })
        })
        .collect()
}

const fn on(side: Side, component: Component) -> Option<DefaultBinding> {
    Some(DefaultBinding { side, component })
}

fn global(action: GlobalAction) -> Option<DefaultBinding> {
    use Component::*;
    use GlobalAction::*;
    use Side::*;
    match action {
        Pause => on(Both, System),
        OpenCommlink | RecenterView | ToggleDebugPanel => None,
        ShowControlsSummary => on(Off, TrackpadTouch),
        // The game's skip prompt names the gamepad's B: the right controller's upper face button
        // is B on both the Index and the Touch, whichever hand is dominant.
        SkipCutscene => on(Right, UpperFace),
    }
}

fn on_foot(action: OnFootAction) -> Option<DefaultBinding> {
    use Component::*;
    use OnFootAction::*;
    use Side::*;
    match action {
        Move => on(Off, Thumbstick),
        Turn => on(Dom, ThumbstickX),
        CrouchFlick => on(Dom, ThumbstickY),
        Fire => on(Dom, TriggerValue),
        FireOffHandPistol | RetractTethers => on(Off, TriggerValue),
        RetractTethersAlt | NextWeapon => on(Off, ThumbstickClick),
        Grapple => on(Off, UpperFace),
        Jump => on(Dom, LowerFace),
        Use => on(Dom, UpperFace),
        Reload => on(Off, LowerFace),
        Detonate => on(Dom, ThumbstickClick),
        // A double tap of the grapple releases the tethers, and the grapple is on a button.
        ReleaseTethers | ThrowGrenade | FireGrapple => None,
        WeaponSelectPad => on(Off, Trackpad),
        WeaponSelectPress => on(Off, TrackpadForce),
        WeaponWheel => on(Dom, ThumbstickY),
    }
}

fn wingsuit(action: WingsuitAction) -> Option<DefaultBinding> {
    use Component::*;
    use Side::*;
    use WingsuitAction::*;
    match action {
        Steer => on(Off, Thumbstick),
        Turn => on(Dom, ThumbstickX),
        AirBrake => on(Off, TriggerValue),
        FireGuns => on(Dom, TriggerValue),
        FireSecondary => on(Dom, GripPress),
        Boost => on(Dom, UpperFace),
        OpenParachute => on(Dom, LowerFace),
        Use => on(Off, LowerFace),
        Grapple => on(Off, UpperFace),
        ReleaseTethers | FireGrapple => None,
    }
}

fn land_vehicle(action: LandVehicleAction) -> Option<DefaultBinding> {
    use Component::*;
    use LandVehicleAction::*;
    use Side::*;
    match action {
        Steer => on(Off, Thumbstick),
        Turn => on(Dom, ThumbstickX),
        Accelerate | Fire => on(Dom, TriggerValue),
        Reverse | FireOffHandPistol => on(Off, TriggerValue),
        FirePrimary => on(Dom, GripPress),
        FireSecondary => on(Off, GripPress),
        Stunt => on(Dom, LowerFace),
        ExitVehicle => on(Dom, UpperFace),
        Grapple => on(Off, UpperFace),
        // The grapple took the off-hand's upper face; nitrous is also on the cockpit's button.
        VehicleMod => on(Dom, ThumbstickClick),
        Handbrake => on(Off, LowerFace),
        Horn => on(Off, ThumbstickClick),
    }
}

fn helicopter(action: HelicopterAction) -> Option<DefaultBinding> {
    use Component::*;
    use HelicopterAction::*;
    use Side::*;
    match action {
        Cyclic => on(Off, Thumbstick),
        Yaw => on(Dom, ThumbstickX),
        CollectiveUp | Fire => on(Dom, TriggerValue),
        CollectiveDown | FireOffHandPistol => on(Off, TriggerValue),
        FirePrimary => on(Dom, GripPress),
        FireSecondary => on(Off, GripPress),
        Stunt => on(Dom, LowerFace),
        ExitVehicle => on(Dom, UpperFace),
        Grapple => on(Off, UpperFace),
        VehicleMod => on(Dom, ThumbstickClick),
        Horn => on(Off, ThumbstickClick),
    }
}

fn plane(action: PlaneAction) -> Option<DefaultBinding> {
    use Component::*;
    use PlaneAction::*;
    use Side::*;
    match action {
        PitchRoll => on(Off, Thumbstick),
        Rudder => on(Dom, ThumbstickX),
        ThrustUp | Fire => on(Dom, TriggerValue),
        ThrustDown | FireOffHandPistol => on(Off, TriggerValue),
        FirePrimary => on(Dom, GripPress),
        FireSecondary => on(Off, GripPress),
        Stunt => on(Dom, LowerFace),
        ExitVehicle => on(Dom, UpperFace),
        Grapple => on(Off, UpperFace),
        Horn => on(Off, ThumbstickClick),
    }
}

fn menu(action: MenuAction) -> Option<DefaultBinding> {
    use Component::*;
    use MenuAction::*;
    use Side::*;
    match action {
        MenuConfirm => on(Both, LowerFace),
        MenuBack => on(Both, UpperFace),
        MenuModifier => on(Both, GripPress),
        MenuNavigate => on(Both, Thumbstick),
        MenuTabPad => on(Both, Trackpad),
        MenuTabPress => on(Both, TrackpadForce),
        MenuPagePrev => on(Left, TriggerValue),
        MenuPageNext => on(Right, TriggerValue),
        PointerSelect => on(Both, TriggerValue),
        MenuFocus => on(Left, ThumbstickClick),
        MenuRecenter => on(Right, ThumbstickClick),
        MapZoom => on(Off, ThumbstickY),
    }
}
