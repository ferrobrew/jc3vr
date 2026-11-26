//! The rows the controls summary describes: which physical control, worked how, does what. The
//! describing reader in the bindings fills them from the binding tables, the summary labels and
//! draws them, and a subsystem with inputs outside the tables (the reload steps) supplies its own.
//! Pure.

use jc3gi::input::input_action_map::Action;
use jc3vr_input::frame::Hand;
use jc3vr_view::vr::input::semantic::bound::SourceName;

/// Which way a stick is pushed for a row.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StickDir {
    /// Any direction: the stick as a whole.
    Any,
    Vertical,
    Horizontal,
    Down,
}

/// A physical control, on a physical hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Control {
    Trigger(Hand),
    Grip(Hand),
    A(Hand),
    B(Hand),
    Stick(Hand, StickDir),
    StickClick(Hand),
    /// The Index controller's trackpad.
    Trackpad(Hand),
    /// A motion of the hand itself.
    Motion(Hand),
    /// The grip of either hand.
    EitherGrip,
    /// The A of either hand.
    EitherA,
    /// The B of either hand.
    EitherB,
    /// The stick of either hand.
    EitherStick(StickDir),
    /// The trackpad of either hand.
    EitherTrackpad,
    /// A motion of either hand.
    EitherMotion,
    /// Both stick clicks together.
    BothStickClicks,
    /// The system (menu) button, whichever hand reports it.
    System,
    /// The right B pressed while the left stick click is held.
    LeftStickClickAndRightB,
    /// A source the mod has no glyph for, by the runtime's name for it, on the hand whose user path
    /// it is under.
    Named(Option<Hand>, SourceName),
    /// Nothing: the action is bound to no input.
    Unbound,
}

impl Control {
    /// The hand the control is on, or `None` for a control of either hand.
    pub const fn hand(self) -> Option<Hand> {
        match self {
            Control::Trigger(h)
            | Control::Grip(h)
            | Control::A(h)
            | Control::B(h)
            | Control::Stick(h, _)
            | Control::StickClick(h)
            | Control::Trackpad(h)
            | Control::Motion(h) => Some(h),
            Control::Named(h, _) => h,
            Control::EitherGrip
            | Control::EitherA
            | Control::EitherB
            | Control::EitherStick(_)
            | Control::EitherTrackpad
            | Control::EitherMotion
            | Control::BothStickClicks
            | Control::System
            | Control::LeftStickClickAndRightB
            | Control::Unbound => None,
        }
    }

    /// The control's place in a listing: the triggers first, then the grips, the face buttons, the
    /// stick, the trackpad, the motions, the sources without a glyph, the system button, and the
    /// unbound actions last.
    pub const fn order(self) -> u8 {
        match self {
            Control::Trigger(_) => 0,
            Control::Grip(_) | Control::EitherGrip => 1,
            Control::A(_) | Control::EitherA => 2,
            Control::B(_) | Control::EitherB => 3,
            Control::Stick(_, StickDir::Any) | Control::EitherStick(StickDir::Any) => 4,
            Control::Stick(_, StickDir::Vertical) | Control::EitherStick(StickDir::Vertical) => 5,
            Control::Stick(_, StickDir::Horizontal)
            | Control::EitherStick(StickDir::Horizontal) => 6,
            Control::Stick(_, StickDir::Down) | Control::EitherStick(StickDir::Down) => 7,
            Control::StickClick(_)
            | Control::BothStickClicks
            | Control::LeftStickClickAndRightB => 8,
            Control::Trackpad(_) | Control::EitherTrackpad => 9,
            Control::Motion(_) | Control::EitherMotion => 10,
            Control::Named(..) => 11,
            Control::System => 12,
            Control::Unbound => 13,
        }
    }
}

/// How the control is worked for a row.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Gesture {
    /// Pressed, pulled, or pushed.
    Press,
    /// Held down past the long-press time.
    Hold,
    /// Pushed up and held.
    HoldUp,
    /// Flicked and let go.
    Flick,
    /// Touched.
    Touch,
    /// Pressed on the left half.
    PressLeftHalf,
    /// Pressed on the right half.
    PressRightHalf,
    /// Pressed on the top quadrant.
    PressUp,
    /// Pressed on the bottom quadrant.
    PressDown,
    /// Pressed on the left quadrant.
    PressLeft,
    /// Pressed on the right quadrant.
    PressRight,
    /// A sharp pull of the hand back toward the body.
    Yank,
    /// Pressed twice in quick succession.
    DoubleTap,
    /// Pressed while both grips are held.
    WithBothGrips,
    /// Pressed while the same hand's grip is held.
    WithGrip,
}

/// What the mod, rather than the game, does with an input.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ModAction {
    /// Turn the body.
    Turn,
    /// Toggle the seated crouch.
    SeatedCrouch,
    /// Capture the held weapon's sight offset.
    SightCapture,
    /// Draw or holster a weapon at a body holster.
    Holster,
    /// Release every tether: the grapple's double tap.
    ReleaseTethers,
    /// Tighten the tethers: the grapple held with nothing new under the aim.
    TightenTethers,
    /// Steady a two-handed weapon by its foregrip.
    Foregrip,
    /// Take a round from the belt pouch and push it into the gun's well.
    Magazine,
    /// Push the round in the hand into the gun's well, marked by a gold ring on the gun.
    InsertRound,
    /// Knock the gun's old magazine out with the one in the hand, or eject it, before the fresh one
    /// goes in.
    ClearWell,
    /// Push the gun's well down onto the belt pouch to reload it in one hand.
    BeltReload,
    /// Flick the gun to eject its magazine.
    FlickEject,
    /// Pull back the slide, the charging handle, or the bolt to chamber a round.
    WorkAction,
    /// Take a grenade from its pouch, and throw it by letting go.
    Grenade,
    /// Pull the pin of the grenade in the hand, starting its fuse.
    PullPin,
    /// Take a C4 charge from its chest pouch, and throw it by letting go.
    C4,
    /// Grip an unheld climbing surface within reach.
    Climb,
    /// Grip an unheld latched vehicle door to open it by hand.
    OpenVehicleDoor,
    /// Detonate the placed charges with the trigger of the hand holding a C4 charge, which stays
    /// in the hand.
    DetonateWithCharge,
    /// Detonate the placed charges with the detonator in the hand.
    Detonate,
    /// Take a parachute handle, and pull it down to steer, or both to brake.
    ParachuteHandles,
    /// Pull a held parachute handle down to steer or contribute to braking.
    PullParachuteHandle,
    /// Take the ripcord at the hip and pull it out to open the parachute.
    Ripcord,
    /// Fire the gun the hand holds, from the hand's vehicle weapon action.
    FireHandGun,
    /// Yank an armed remote target, optionally holding the grapple button to swing it.
    SnatchYank,
    /// Swing the attached target and release grip to detach, or B to snatch.
    SnatchSwing,
    /// Keep the grip closed while preparing the snatch.
    SnatchGrip,
    /// Release the grip to detach with the target's current momentum.
    SnatchThrow,
    /// Hold the resolved grapple button before the yank.
    SnatchModifier,
    /// Release the resolved grapple button to resume the pull.
    SnatchResume,
    /// Spread the arms in the wingsuit and lower or raise them to bank, dive, and climb.
    ArmSteering,
    /// Lower both arms, then spread them to request wingsuit deployment.
    GestureDeploy,
    /// After arming with lowered then spread arms, withdraw both arms to request closure.
    GestureClose,
    /// After arming with lowered then spread arms, turn both spread palms forward to brake.
    GestureBrake,
    /// Hold the car's or the boat's wheel to steer.
    Wheel,
    /// Hold the handlebars to steer.
    Handlebars,
    /// Hold the helicopter's cyclic and collective to fly.
    HelicopterControls,
    /// Hold the plane's flight stick and throttle to fly.
    PlaneControls,
    /// Open the weapon wheel, point the hand to choose, and let go to equip.
    WeaponWheel,
    /// Show the controls summary.
    ShowSummary,
    /// Recenter the VR view.
    Recenter,
    /// Show or hide the mod's debug panel.
    DebugPanel,
    /// Zoom the native map.
    MapZoom,
}

/// What a row does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Drives these game actions; the game's context picks the one that applies.
    Actions(Vec<Action>),
    /// Drives the mod.
    Mod(ModAction),
}

/// One input and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub control: Control,
    pub gesture: Gesture,
    pub effect: Effect,
}
