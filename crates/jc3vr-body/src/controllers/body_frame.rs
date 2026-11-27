//! The body's handoff data: what the body-bound controllers decided this tick, for the crates
//! above to act on.
//!
//! A [`BodyFrame`] has one section per writer, and each writer publishes its section at its own
//! stage of the tick:
//!
//! - the hand tiers ([`TierFrame`]), once in the input tick's game stage;
//! - the locomotion ([`LocomotionFrame`]), once in the character update, and the modes' liveness
//!   ([`ModeLiveness`]) at each run of a mode's evaluator there;
//! - the rigid grip ([`GripFrame`]), at each of the characters' pose-finalization placements of
//!   the local player's weapons, the last of which leaves the tick's grip;
//! - the aim ([`AimFrame`]), once in the aim control's post-simulation update.
//!
//! The sections are separate publications rather than one value copied at one point of the tick,
//! so a reader sees each section as its writer last left it, wherever in the frame it reads: a
//! copy taken at the game stage would hand a render-time reader the aim of the tick before. The
//! per-hand view across the sections is [`HandFrame`].
//!
//! The debug UI's readouts (`tiers::status`, `grip::status`, `aim::status`, `locomotion::status`)
//! are built from these sections plus their writers' counts and diagnostics; code that acts on the
//! body's decisions reads the frame.

use jc3vr_core::published::{Published, Stage, Stamp};
use jc3vr_input::frame::Hand;

use crate::controllers::{
    aim::{AimFrame, HandAim, assist::AssistCorrection, gun::HeldWeapon},
    hands::{
        grip::{GripFrame, HandGrip},
        tiers::{HandTier, TierFrame},
    },
    locomotion::LocomotionFrame,
};

/// Every section of the body's handoff data, each as its writer last published it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyFrame {
    pub tiers: TierFrame,
    pub grip: GripFrame,
    pub aim: AimFrame,
    pub locomotion: LocomotionFrame,
    pub liveness: ModeLiveness,
}

impl BodyFrame {
    /// Every section as last published.
    pub fn read() -> Self {
        Self {
            tiers: tiers(),
            grip: grip(),
            aim: aim(),
            locomotion: locomotion(),
            liveness: liveness(),
        }
    }

    /// Both hands' view across the sections, indexed by [`Hand::index`].
    pub fn hands(&self) -> [HandFrame; 2] {
        Hand::BOTH.map(|hand| self.hand(hand))
    }

    /// One hand's view across the sections.
    pub fn hand(&self, hand: Hand) -> HandFrame {
        let i = hand.index();
        HandFrame {
            tier: self.tiers.tiers[i],
            effective: self.tiers.effective[i],
            weight: self.tiers.weights[i],
            anchor_weight: self.tiers.anchor_weights[i],
            grip: self.grip.hands[i],
            aim: self.aim.hands[i],
            armed: self.aim.armed[i],
            assist: self.aim.assist[i],
        }
    }
}

/// One hand's handoff data across the sections of the [`BodyFrame`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandFrame {
    /// The tier the classified state assigns the hand.
    pub tier: HandTier,
    /// The tier the hand takes ([`HandTier::effective_in`]).
    pub effective: HandTier,
    /// The hand's controller weight: one on the controller, zero on the animation.
    pub weight: f32,
    /// The hand's share of the anchored pose.
    pub anchor_weight: f32,
    /// The gun the hand's last placement pinned or watched.
    pub grip: Option<HandGrip>,
    /// The hand's aim on the last aim update.
    pub aim: Option<HandAim>,
    /// The weapon armed in the hand beside the wielded one.
    pub armed: Option<HeldWeapon>,
    /// The aim assist's correction of the hand's gun on the last aim update.
    pub assist: Option<AssistCorrection>,
}

/// When each mode's own evaluator last ran for the local player: the on-foot orientation
/// evaluator, which runs every tick on foot, idle included, and never in a vehicle, and the swim
/// movement cores. A reader that compares the time with the one it saw at its previous check knows
/// whether the evaluator ran in between, which says the player is in that mode.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ModeLiveness {
    pub on_foot: Option<Stamp>,
    pub swimming: Option<Stamp>,
}

/// The hand tiers' section.
pub fn tiers() -> TierFrame {
    TIER_FRAME
        .latest()
        .map_or(TierFrame::INITIAL, |stamped| stamped.value)
}

/// The rigid grip's section.
pub fn grip() -> GripFrame {
    GRIP_FRAME
        .latest()
        .map(|stamped| stamped.value)
        .unwrap_or_default()
}

/// The aim's section.
pub fn aim() -> AimFrame {
    AIM_FRAME
        .latest()
        .map(|stamped| stamped.value)
        .unwrap_or_default()
}

/// The locomotion's section.
pub fn locomotion() -> LocomotionFrame {
    LOCOMOTION_FRAME
        .latest()
        .map(|stamped| stamped.value)
        .unwrap_or_default()
}

/// The modes' liveness.
pub fn liveness() -> ModeLiveness {
    ModeLiveness {
        on_foot: ON_FOOT_EVALUATED.latest().map(|stamped| stamped.at),
        swimming: SWIM_EVALUATED.latest().map(|stamped| stamped.at),
    }
}

/// Both hands' view across the sections, indexed by [`Hand::index`].
pub fn hands() -> [HandFrame; 2] {
    BodyFrame::read().hands()
}

pub(crate) static TIER_FRAME: Published<TierFrame> = Published::new(Stage::InputGame);
pub(crate) static LOCOMOTION_FRAME: Published<LocomotionFrame> =
    Published::new(Stage::CharacterUpdate);
pub(crate) static GRIP_FRAME: Published<GripFrame> = Published::new(Stage::PoseFinalize);
pub(crate) static AIM_FRAME: Published<AimFrame> = Published::new(Stage::PostSimulation);

/// Note a run of the on-foot orientation evaluator for the local player.
pub(crate) fn mark_on_foot_evaluated() {
    ON_FOOT_EVALUATED.publish(());
}

/// Note a run of a swim movement core for the local player.
pub(crate) fn mark_swim_evaluated() {
    SWIM_EVALUATED.publish(());
}

static ON_FOOT_EVALUATED: Published<()> = Published::new(Stage::CharacterUpdate);
static SWIM_EVALUATED: Published<()> = Published::new(Stage::CharacterUpdate);
