//! The finish half's fire stage: each hand's own fire input, the dead click, and the fire
//! hold-back.

use jc3gi::input::input_action_map::Action;
use jc3vr_body::controllers::{
    body_frame,
    hands::tiers::{HandTier, TierState},
    haptics,
    intent::{
        controls::ButtonControl,
        hold::{Desired, MapKind},
    },
    physics::local_player::LocalPlayer,
    telemetry,
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::{frame::Hand, haptics::cues::Event};

use crate::controllers::{
    loadout::contents::HandContents,
    weapons::{
        begin::with_holder,
        capabilities::{GunHand, gun_hands_now, tracked_hands, trigger_claims},
        contents::sample,
        engine,
        finish::Finish,
        hold,
        hold::tick,
        reload::fire_hold::FireHold,
    },
};

/// Decide each hand's own fire input after every move of the tick, from the hands as they are now:
/// only a hand on a gun's grip fires it, a trigger press that fires nothing clicks dry with its
/// reason, and the fire inputs are held back while the reload holds the gun, an explosive is in a
/// hand, a dock's slide moves the gun, or a weapon action plays.
pub(super) fn fire(game: &GameTick<'_>, f: &mut Finish<'_>) {
    let fire_presses = f.inputs.fire_presses;
    let capabilities = f.context.modes;
    let reload_held = f.reload.as_ref().map_or([None; 2], |reload| reload.held);
    // SAFETY: the local player, in the game tick.
    let current =
        unsafe { LocalPlayer::get(game).and_then(|owner| engine::held_weapon(owner.character())) }
            .map(|weapon| with_holder(weapon, hold::owned_holder(weapon.address)));
    let held = sample(current.as_ref());
    let gun_hands = held.map(|hand| hand.wielded);
    // A gun the reload holds cannot fire. The dual-wield split keeps a held pistol's trigger from
    // the game itself, so the fire input is held back only when no drawn gun may fire.
    let reload_blocks = gun_hands.contains(&true)
        && Hand::BOTH
            .iter()
            .all(|h| !gun_hands[h.index()] || reload_held[h.index()].is_some());

    // Only a hand on the grip fires the gun.
    let holder = current.and_then(|weapon| weapon.holder);
    let docking = hold::act::docking();
    let gun_roles = gun_hands_now();
    let ready = if f.active.is_some() {
        tracked_hands()
    } else {
        [false; 2]
    };
    let claims = trigger_claims(capabilities, gun_roles, ready);
    let fire = HandFire {
        allowed: claims.map(|claim| claim.fire),
        triggers: &fire_presses,
        holder,
        docking,
        action_running: Hand::BOTH.map(weapon_action_running),
    };
    let fired = hand_fire(&fire);
    tick::publish_fire(fired);
    let refusals = refused_fire(&RefusedFire {
        fire: &fire,
        fired,
        reserved: claims.map(|claim| claim.reserved),
        roles: gun_roles,
        ready,
        may_fire: capabilities.guns,
        reload_held,
    });
    for (hand, refusal) in Hand::BOTH
        .into_iter()
        .filter_map(|hand| refusals[hand.index()].map(|r| (hand, r)))
    {
        haptics::play(hand, Event::DryFire);
        telemetry::report("trigger", Some(hand), "the trigger fired nothing", || {
            Some(refusal.label().to_owned())
        });
    }
    let fire_blocked = reload_blocks
        || explosive_holds_fire(&held)
        || holder.is_some_and(|h| docking[h.index()])
        || primary_trigger_idle(current, &fire_presses)
        || primary_action_blocked(holder, Hand::BOTH.map(weapon_action_running));
    if fire_blocked {
        hold_fire_back(f.desired);
    }
    f.current = current;
    f.held = held;
    f.fire_blocked = fire_blocked;
}

/// Whether the fire input is held back for a grenade, a charge, or the detonator in a hand: with no
/// gun in the other hand, the fire input would draw a gun into the hand that holds it.
pub(super) fn explosive_holds_fire(hands: &[HandContents; 2]) -> bool {
    Hand::BOTH
        .into_iter()
        .any(|hand| hands[hand.index()].explosive() && !hands[hand.other().index()].gun())
}

/// The fire inputs a weapon action holds back.
const FIRE_ACTIONS: [Action; 2] = [Action::FIRE_RIGHT, Action::R2];

/// Whether the fire input is held back because no hand holding the wielded weapon pulls its
/// trigger while another hand's does: the one fire action reaches the weapon from either trigger,
/// and only a hand holding the weapon may fire it. The other hand is empty, or holds a gun of its
/// own, which its trigger fires instead.
pub(super) fn primary_trigger_idle(
    weapon: Option<engine::HeldWeapon>,
    triggers: &[ButtonControl; 2],
) -> bool {
    weapon.is_some_and(|w| {
        let hands = w.wielded.hands;
        let pulled = |h: Hand| triggers[h.index()].held();
        hands.contains(&true)
            && Hand::BOTH.into_iter().any(pulled)
            && !Hand::BOTH
                .into_iter()
                .any(|h| hands[h.index()] && pulled(h))
    })
}

/// What decides each hand's own fire input.
pub(super) struct HandFire<'a> {
    pub allowed: [bool; 2],
    pub triggers: &'a [ButtonControl; 2],
    /// The hand holding the wielded weapon.
    pub holder: Option<Hand>,
    /// Each hand whose gun a dock's slide is moving onto its socket, by [`Hand::index`].
    pub docking: [bool; 2],
    /// A reload, a draw, or a holster plays with each hand, and holds its fire back, by
    /// [`Hand::index`].
    pub action_running: [bool; 2],
}

/// Each hand's own fire input, by [`Hand::index`]: its trigger pulled with permission for that
/// hand's firing grip while nothing holds its gun back. The true reload and shooting permission are checked at
/// the gun's update.
pub(super) fn hand_fire(f: &HandFire) -> [bool; 2] {
    Hand::BOTH.map(|hand| {
        let wielding = f.holder == Some(hand);
        f.allowed[hand.index()]
            && f.triggers[hand.index()].held()
            && !f.docking[hand.index()]
            && !(wielding && primary_action_blocked(f.holder, f.action_running))
    })
}

/// Why a hand's trigger press fired nothing, for the dead click and its log line.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum FireRefusal {
    /// The hand braces the gun; only the hand on the grip fires it.
    Bracing,
    /// The hand's controller is not tracked.
    Untracked,
    /// The mode allows no firing.
    Mode,
    /// A dock's slide is moving the gun onto its socket.
    Docking,
    /// A reload, a draw, or a holster plays with the hand.
    Action,
    /// The reload holds the gun, for its reason.
    Reload(FireHold),
    /// The hand holds a gun the mod's fire path does not fire.
    Other,
}

impl FireRefusal {
    pub(super) const fn label(self) -> &'static str {
        match self {
            FireRefusal::Bracing => "the hand braces the gun, and only the hand on the grip fires",
            FireRefusal::Untracked => "the controller is not tracked",
            FireRefusal::Mode => "firing is not allowed here",
            FireRefusal::Docking => "the gun is moving onto its socket",
            FireRefusal::Action => "a reload, draw, or holster is playing",
            FireRefusal::Reload(hold) => hold.label(),
            FireRefusal::Other => "the hand's gun is not fired from its trigger",
        }
    }
}

/// What decides whether a trigger press fired nothing, and why.
pub(super) struct RefusedFire<'a> {
    pub fire: &'a HandFire<'a>,
    /// Each hand's own fire input this tick ([`hand_fire`]).
    pub fired: [bool; 2],
    /// Each hand on some part of a gun, whose trigger belongs to it.
    pub reserved: [bool; 2],
    pub roles: [GunHand; 2],
    pub ready: [bool; 2],
    /// Whether the mode allows firing at all.
    pub(crate) may_fire: bool,
    /// Why the reload holds each hand's gun, when it does.
    pub(crate) reload_held: [Option<FireHold>; 2],
}

/// Each hand whose trigger was pressed this tick, on a gun it holds or braces, without firing it,
/// with the reason, by [`Hand::index`]: the one decision of the dead click. A press on an empty
/// hand, a hand holding something other than a gun, or the native pair's shared trigger on a gun
/// the reload does not hold is not refused here.
pub(super) fn refused_fire(r: &RefusedFire) -> [Option<FireRefusal>; 2] {
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        let pressed = r.fire.triggers[i].pressed;
        if !pressed || !r.reserved[i] {
            return None;
        }
        if let Some(hold) = r.reload_held[i]
            && matches!(r.roles[i], GunHand::Grip | GunHand::NativePair(_))
        {
            return Some(FireRefusal::Reload(hold));
        }
        if matches!(r.roles[i], GunHand::NativePair(_)) {
            return None;
        }
        if r.fired[i] {
            return None;
        }
        Some(if !r.may_fire {
            FireRefusal::Mode
        } else if r.roles[i] == GunHand::Support {
            FireRefusal::Bracing
        } else if !r.ready[i] {
            FireRefusal::Untracked
        } else if r.fire.docking[i] {
            FireRefusal::Docking
        } else if r.fire.holder == Some(hand)
            && primary_action_blocked(r.fire.holder, r.fire.action_running)
        {
            FireRefusal::Action
        } else {
            FireRefusal::Other
        })
    })
}

pub(super) fn primary_action_blocked(holder: Option<Hand>, action_running: [bool; 2]) -> bool {
    holder.is_some_and(|hand| action_running[hand.index()])
}

/// Drop the fire inputs from the tables' output.
pub(super) fn hold_fire_back(desired: &mut Vec<Desired>) {
    desired
        .retain(|d| !(d.target.map == MapKind::Player && FIRE_ACTIONS.contains(&d.target.action)));
}

/// Whether a reload, a draw, or a holster is playing with `gun_hand`, the hand holding the weapon
/// (or the hand the last one rode, during a draw), in the action-runs tier.
fn weapon_action_running(gun_hand: Hand) -> bool {
    let status = body_frame::tiers();
    status.state == Some(TierState::WeaponAction)
        && status.effective[gun_hand.index()] == HandTier::ActionRuns
}
