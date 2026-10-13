//! The weapon-agnostic body: on foot under the controllers, the character's body plays what it
//! plays unarmed, whatever is held, and the controller hands alone place the arms and the weapons.
//!
//! The weapon reaches the body through three routes (see `docs/engine/character/animation-layers.md`):
//!
//! - the wielded weapon's animation modifiers, which select the base layer's weapon clip variants
//!   (the bladed stances, the held arms, the off-hand on a rifle's foregrip). While the unarmed
//!   body applies they reach only the upper-body action layer, whose clip lengths keep the
//!   reloads' and draws' timing ([`crate::hooks::character::modifiers`]);
//! - the partial and additive layers (the upper-body actions, the turns toward the aim target, and
//!   the fire additives). They keep running, so their animation events (a reload's refill, a
//!   holster's unequip) and segments still drive the game, but the layers set in
//!   [`UnarmedBodyConfig`] are left out of the pose ([`crate::hooks::character::layers`]);
//! - the game's off-hand weapon grip, switched off for both hands at the `SECONDARY` pass's entry
//!   ([`crate::controllers::hands::ik::before_secondary`]).
//!
//! The decision is made once per tick at the entry of the local player's pose-finalization pass
//! ([`tick`]), which also publishes the masked layers' blenders for the pose job. The modifier pass
//! and the pose job of the next tick read it. The settings are `controllers.body.unarmed`.

use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

use jc3gi::character::character::{AnimationLayer, Character};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    controllers::{
        hands::tiers::{self, TierState},
        scheme,
    },
    headpose, hooks,
};

/// Which of the game's weapon animations reach the character's body on foot.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WeaponAnimations {
    /// The game's own: the weapon's stances, the upper-body actions, and the additive layers.
    Native,
    /// The body plays what it plays unarmed, whatever is held: the weapon's animation modifiers stay
    /// off every layer but the upper-body actions, and the layers set in [`UnarmedBodyConfig`] are
    /// left out of the pose while still running, so reloads, draws, and holsters keep their timing
    /// and their animation events.
    #[default]
    Unarmed,
}

impl WeaponAnimations {
    pub const ALL: [WeaponAnimations; 2] = [WeaponAnimations::Native, WeaponAnimations::Unarmed];

    pub const fn label(self) -> &'static str {
        match self {
            WeaponAnimations::Native => "native",
            WeaponAnimations::Unarmed => "unarmed",
        }
    }
}

/// The weapon-agnostic body. Acts only under the controller scheme with the headset driving the
/// view, in gameplay, while the hand tiers classify the character as on foot (including a reload,
/// draw, or holster), so it defaults on.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UnarmedBodyConfig {
    pub weapon_animations: WeaponAnimations,
    /// Leave the upper-body action layer (draw, holster, switch, reload, pump, bolt, pickup, and
    /// throws) out of the pose.
    pub mask_actions: bool,
    /// Leave the synchronised upper-body layer (the turns toward the aim target, and the locomotion
    /// partials) out of the pose.
    pub mask_synced_partial: bool,
    /// Leave the synchronised additive layer (leans, landings, and the grapple, grenade, and launcher
    /// fire additives) out of the pose.
    pub mask_additive: bool,
    /// Switch the game's off-hand weapon grip off for both hands, driven or not, so that only the
    /// foregrip grab puts a hand on a two-handed weapon.
    pub release_weapon_grip: bool,
}

impl UnarmedBodyConfig {
    pub const fn new() -> Self {
        Self {
            weapon_animations: WeaponAnimations::Unarmed,
            mask_actions: true,
            mask_synced_partial: true,
            mask_additive: true,
            release_weapon_grip: true,
        }
    }
}

impl Default for UnarmedBodyConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether the unarmed body applies, and why not when it does not.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum UnarmedVerdict {
    Applies,
    StandDown(UnarmedStandDown),
    /// The local player's pose finalization has not run yet.
    #[default]
    NotRun,
}

/// Why the unarmed body stands down, in the order the conditions are tested.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum UnarmedStandDown {
    /// The weapon animations are set to the game's own.
    Native,
    NotControllers,
    NotVr,
    NotInGameplay,
    /// The hand tiers do not classify the character as on foot.
    NotOnFoot,
}

impl UnarmedStandDown {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Native => "weapon animations set to native",
            Self::NotControllers => "not the controller scheme",
            Self::NotVr => "the headset does not drive the view",
            Self::NotInGameplay => "not in gameplay",
            Self::NotOnFoot => "not on foot",
        }
    }
}

/// The conditions [`decide`] tests.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct GateInputs {
    pub controllers: bool,
    pub vr: bool,
    pub gameplay: bool,
    /// The hand tiers' classification of the character's state.
    pub tier: Option<TierState>,
}

/// Decide whether the unarmed body applies.
pub fn decide(weapon_animations: WeaponAnimations, inputs: &GateInputs) -> UnarmedVerdict {
    let stand_down = if weapon_animations == WeaponAnimations::Native {
        Some(UnarmedStandDown::Native)
    } else if !inputs.controllers {
        Some(UnarmedStandDown::NotControllers)
    } else if !inputs.vr {
        Some(UnarmedStandDown::NotVr)
    } else if !inputs.gameplay {
        Some(UnarmedStandDown::NotInGameplay)
    } else if !matches!(
        inputs.tier,
        Some(TierState::OnFoot | TierState::WeaponAction)
    ) {
        Some(UnarmedStandDown::NotOnFoot)
    } else {
        None
    };
    stand_down.map_or(UnarmedVerdict::Applies, UnarmedVerdict::StandDown)
}

/// The layers the unarmed body can leave out of the pose, in the order of [`masked_layers`].
pub const MASKABLE_LAYERS: [AnimationLayer; 3] = [
    AnimationLayer::GLOBAL_PARTIAL,
    AnimationLayer::SYNCED_PARTIAL,
    AnimationLayer::SYNCED_ADDITIVE,
];

/// Which of [`MASKABLE_LAYERS`] are left out of the pose.
pub fn masked_layers(
    config: &UnarmedBodyConfig,
    verdict: UnarmedVerdict,
) -> [bool; MASKABLE_LAYERS.len()] {
    let applies = verdict == UnarmedVerdict::Applies;
    [
        applies && config.mask_actions,
        applies && config.mask_synced_partial,
        applies && config.mask_additive,
    ]
}

pub const fn layer_label(layer: AnimationLayer) -> &'static str {
    match layer {
        AnimationLayer::MAINBODY => "base",
        AnimationLayer::SYNCED_PARTIAL => "synced partial",
        AnimationLayer::GLOBAL_PARTIAL => "upper-body actions",
        AnimationLayer::SYNCED_ADDITIVE => "synced additive",
        AnimationLayer::GLOBAL_ADDITIVE => "global additive",
        AnimationLayer::VOCALS => "vocals",
    }
}

/// Which rule systems the wielded weapon's animation modifiers are kept off.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ModifierSuppression {
    /// Every rule system takes them: the game's own behaviour.
    None,
    /// Only the upper-body action layer takes them (the unarmed body).
    AllButActions,
}

/// Whether the rule system at `layer` takes the weapon's modifiers under `suppression`.
pub fn keeps_weapon_modifier(suppression: ModifierSuppression, layer: usize) -> bool {
    match suppression {
        ModifierSuppression::None => true,
        ModifierSuppression::AllButActions => layer == AnimationLayer::GLOBAL_PARTIAL as usize,
    }
}

/// What the unarmed body decided on its last tick, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct UnarmedStatus {
    pub at: Option<Instant>,
    pub verdict: UnarmedVerdict,
    /// Which of [`MASKABLE_LAYERS`] were published for masking.
    pub masked: [bool; MASKABLE_LAYERS.len()],
    /// How many animation layers (body parts) the character has.
    pub layers: usize,
    /// How many times the pose job left each of [`MASKABLE_LAYERS`] out, since injection.
    pub skipped: [u64; MASKABLE_LAYERS.len()],
    /// Whether the latest modifier pass kept the weapon's modifiers off the body.
    pub weapon_suppressed: bool,
}

/// The latest status.
pub fn status() -> UnarmedStatus {
    let mut status = *STATE.lock();
    status.skipped = hooks::character::layers::skipped();
    status
}

/// Decide whether the unarmed body applies for the local player and publish the masked layers'
/// blenders for the pose job. Run at the entry of the pose-finalization pass.
///
/// # Safety
///
/// `character` must be null or a live character at the entry of its pose-finalization pass.
pub unsafe fn tick(character: *mut Character) {
    let Some(character) = (unsafe { character.as_ref() }).filter(|c| c.m_IsLocalCharacter) else {
        return;
    };
    let config = Config::lock_query(|c| c.controllers.body.unarmed);
    let inputs = GateInputs {
        controllers: scheme::is_controllers(),
        vr: headpose::source() == headpose::Source::Vr,
        gameplay: hooks::in_gameplay(),
        tier: tiers::status().state,
    };
    let verdict = decide(config.weapon_animations, &inputs);
    let masked = masked_layers(&config, verdict);
    let blenders = unsafe { layer_blenders(character) };
    let published = std::array::from_fn(|i| {
        if masked[i] {
            blenders
                .get(MASKABLE_LAYERS[i] as usize)
                .copied()
                .unwrap_or(0)
        } else {
            0
        }
    });
    hooks::character::layers::publish(published);
    let mut state = STATE.lock();
    *state = UnarmedStatus {
        at: Some(Instant::now()),
        verdict,
        masked: published.map(|b| b != 0),
        layers: blenders.len(),
        skipped: [0; MASKABLE_LAYERS.len()],
        weapon_suppressed: state.weapon_suppressed,
    };
}

/// Record whether the modifier pass kept the wielded weapon's modifiers off the body, for the
/// readout.
pub fn record_modifiers(weapon_suppressed: bool) {
    STATE.lock().weapon_suppressed = weapon_suppressed;
}

/// Whether the unarmed body applied on the local player's latest pose finalization, recently enough
/// to hold for the next tick's modifier pass.
pub fn applies() -> bool {
    let status = *STATE.lock();
    status.verdict == UnarmedVerdict::Applies && status.at.is_some_and(|at| at.elapsed() < FRESH)
}

/// Whether the game's off-hand weapon grip is switched off for both hands this tick.
pub fn release_grip() -> bool {
    Config::lock_query(|c| c.controllers.body.unarmed.release_weapon_grip) && applies()
}

/// How long a decision holds for the modifier pass, which runs earlier in the next tick.
const FRESH: Duration = Duration::from_millis(250);

static STATE: LazyLock<Mutex<UnarmedStatus>> =
    LazyLock::new(|| Mutex::new(UnarmedStatus::default()));

/// Each animation layer's blender address, indexed by layer, zero where a layer has none.
///
/// # Safety
///
/// `character` must be a live character.
unsafe fn layer_blenders(character: &Character) -> Vec<usize> {
    let Some(controller) = (unsafe { character.m_AnimatedModel.m_AnimationController.as_ref() })
    else {
        return Vec::new();
    };
    unsafe { controller.m_BodyParts.as_slice() }
        .iter()
        .map(|part| {
            unsafe { part.px.as_ref() }
                .and_then(|part| unsafe { part.m_AnimationDriver.as_ref() })
                .map_or(0, |driver| driver.m_Blender.px as usize)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on_foot() -> GateInputs {
        GateInputs {
            controllers: true,
            vr: true,
            gameplay: true,
            tier: Some(TierState::OnFoot),
        }
    }

    #[test]
    fn applies_on_foot_and_during_weapon_actions() {
        assert_eq!(
            decide(WeaponAnimations::Unarmed, &on_foot()),
            UnarmedVerdict::Applies
        );
        let action = GateInputs {
            tier: Some(TierState::WeaponAction),
            ..on_foot()
        };
        assert_eq!(
            decide(WeaponAnimations::Unarmed, &action),
            UnarmedVerdict::Applies
        );
    }

    #[test]
    fn stands_down_in_order() {
        let cases = [
            (
                WeaponAnimations::Native,
                on_foot(),
                UnarmedStandDown::Native,
            ),
            (
                WeaponAnimations::Unarmed,
                GateInputs {
                    controllers: false,
                    vr: false,
                    ..on_foot()
                },
                UnarmedStandDown::NotControllers,
            ),
            (
                WeaponAnimations::Unarmed,
                GateInputs {
                    vr: false,
                    gameplay: false,
                    ..on_foot()
                },
                UnarmedStandDown::NotVr,
            ),
            (
                WeaponAnimations::Unarmed,
                GateInputs {
                    gameplay: false,
                    tier: None,
                    ..on_foot()
                },
                UnarmedStandDown::NotInGameplay,
            ),
        ];
        for (setting, inputs, reason) in cases {
            assert_eq!(
                decide(setting, &inputs),
                UnarmedVerdict::StandDown(reason),
                "{inputs:?}"
            );
        }
    }

    #[test]
    fn stands_down_off_foot() {
        let off_foot = [
            None,
            Some(TierState::VehicleSeated),
            Some(TierState::VehicleTransition),
            Some(TierState::Parachute),
            Some(TierState::Wingsuit),
            Some(TierState::Swimming),
            Some(TierState::GrappleHang),
            Some(TierState::GrappleReel),
            Some(TierState::Cutscene),
            Some(TierState::Ragdoll),
        ];
        for tier in off_foot {
            let inputs = GateInputs { tier, ..on_foot() };
            assert_eq!(
                decide(WeaponAnimations::Unarmed, &inputs),
                UnarmedVerdict::StandDown(UnarmedStandDown::NotOnFoot),
                "{tier:?}"
            );
        }
    }

    #[test]
    fn masks_only_while_applying_and_switched_on() {
        let all = UnarmedBodyConfig::new();
        assert_eq!(
            masked_layers(&all, UnarmedVerdict::Applies),
            [true, true, true]
        );
        assert_eq!(
            masked_layers(&all, UnarmedVerdict::StandDown(UnarmedStandDown::NotOnFoot)),
            [false; 3]
        );
        assert_eq!(masked_layers(&all, UnarmedVerdict::NotRun), [false; 3]);
        let actions_only = UnarmedBodyConfig {
            mask_synced_partial: false,
            mask_additive: false,
            ..UnarmedBodyConfig::new()
        };
        assert_eq!(
            masked_layers(&actions_only, UnarmedVerdict::Applies),
            [true, false, false]
        );
    }

    #[test]
    fn the_actions_layer_keeps_the_weapon_modifiers() {
        let layers = [
            AnimationLayer::MAINBODY,
            AnimationLayer::SYNCED_PARTIAL,
            AnimationLayer::GLOBAL_PARTIAL,
            AnimationLayer::SYNCED_ADDITIVE,
        ];
        for layer in layers {
            let index = layer as usize;
            assert!(keeps_weapon_modifier(ModifierSuppression::None, index));
            assert_eq!(
                keeps_weapon_modifier(ModifierSuppression::AllButActions, index),
                layer == AnimationLayer::GLOBAL_PARTIAL,
            );
        }
    }
}
