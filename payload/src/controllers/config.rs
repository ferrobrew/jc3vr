//! The motion-controller configuration: the master switch, the scheme, and one sub-struct per
//! subsystem of the plan of record (`docs/mod/input/controllers-and-roomscale.md`), each holding its
//! toggle and the tuning beside it.
//!
//! Defaults follow one rule. A subsystem that acts only under the controller scheme defaults on,
//! because it changes nothing until controllers are in use. A subsystem that would change the
//! gamepad scheme's behaviour, or the behaviour of the headset alone, defaults off, so that the mod
//! behaves as it did before controllers existed until the player opts in.
//!
//! Stick turning is not configured here: the controller scheme's right stick drives the same body
//! yaw as the gamepad's look input, so it shares
//! [`VrTurnConfig`](crate::headpose::config::VrTurnConfig) (`headpose.vr_turn`) rather than keeping
//! a second copy of the smooth and snap settings.

use serde::{Deserialize, Serialize};

use crate::controllers::{
    bindings::yank::YankConfig,
    fingers::config::FingersConfig,
    frame::{Hand, PressThresholds},
    hands::{anchor::AnchorConfig, tiers::HandTierTable},
    vehicles::config::VehiclesConfig,
    weapons::config::{
        DualWieldConfig, ForegripConfig, GrenadeConfig, HapticsConfig, HolstersConfig,
        MagazineConfig, MeleeStrikeConfig, RecoilConfig, ScopeConfig,
    },
};

/// All motion-controller settings, added to [`crate::config::Config`] as `controllers`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct ControllersConfig {
    /// Master switch. Off, the mod reads no controller input and publishes no controller frame, so
    /// every controller subsystem is inert and the gamepad scheme is in effect. The OpenXR actions
    /// are still created and attached with the session, because attachment is possible only once
    /// per session and turning this on later must not require a new one.
    pub enabled: bool,
    /// Which control scheme is in effect (see [`SchemeSetting`]).
    pub scheme: SchemeSetting,
    /// The hand that holds and fires the guns. The other hand fires the grapple. Mirroring swaps the
    /// bindings and the weapon attach bones; the grapple gauntlet stays on the model's left arm.
    pub dominant_hand: Hand,
    /// Pin the game's "the gamepad is the active input device" setting to the gamepad while the
    /// controller scheme is in effect, so that the game applies its gamepad rules (aim assist,
    /// tap-style melee) and shows gamepad prompts. The setting is restored when the scheme ends.
    pub gamepad_semantics: bool,
    /// Raw input interpretation: the derived press thresholds and hand tracking.
    pub input: InputConfig,
    /// On-foot stick locomotion.
    pub locomotion: LocomotionConfig,
    /// The per-tick mode detector.
    pub mode: ModeConfig,
    /// The controller-to-game bindings.
    pub bindings: BindingsConfig,
    /// The aim-state pin and the mental-state cuts.
    pub aim_state: AimStateConfig,
    /// Controller-owned hands.
    pub hands: HandsConfig,
    /// Per-hand aiming.
    pub aim: AimConfig,
    /// The world-space overlay layer's readouts.
    pub overlay: OverlayConfig,
    /// Physical weapon handling.
    pub weapons: WeaponsConfig,
    /// Body scale remapping (see [`crate::controllers::body`]).
    pub body: BodyRemapConfig,
    /// Posture and roomscale locomotion.
    pub roomscale: RoomscaleConfig,
    /// Vehicles, mounted guns, the wingsuit, and the parachute.
    pub vehicles: VehiclesConfig,
    /// Finger curls on the rendered hands.
    pub fingers: FingersConfig,
}

impl ControllersConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            scheme: SchemeSetting::Auto,
            dominant_hand: Hand::Right,
            gamepad_semantics: true,
            input: InputConfig::new(),
            locomotion: LocomotionConfig::new(),
            mode: ModeConfig::new(),
            bindings: BindingsConfig::new(),
            aim_state: AimStateConfig::new(),
            hands: HandsConfig::new(),
            aim: AimConfig::new(),
            overlay: OverlayConfig::new(),
            weapons: WeaponsConfig::new(),
            body: BodyRemapConfig::new(),
            roomscale: RoomscaleConfig::new(),
            vehicles: VehiclesConfig::new(),
            fingers: FingersConfig::new(),
        }
    }
}

impl Default for ControllersConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Which control scheme is in effect: the controller scheme (hand aiming, controller bindings,
/// controller-owned hands) or the gamepad scheme (gaze aiming, the game's own gamepad and keyboard
/// input, animated hands).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SchemeSetting {
    /// Switch on the most recent input: controller input selects the controller scheme, and
    /// gamepad, keyboard, or mouse input selects the gamepad scheme.
    #[default]
    Auto,
    /// Always the controller scheme, even when no controller is present.
    Controllers,
    /// Always the gamepad scheme.
    Gamepad,
}

/// Raw input interpretation.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    /// The trigger values that press and release
    /// [`Button::TriggerPress`](crate::controllers::frame::Button::TriggerPress).
    pub trigger_press: PressThresholds,
    /// The squeeze values that press and release
    /// [`Button::SqueezePress`](crate::controllers::frame::Button::SqueezePress).
    /// The grip is used only as an edge, so the press threshold sits high enough that resting a
    /// hand on the grip does not press it.
    pub squeeze_press: PressThresholds,
    /// Thumbstick and trackpad magnitudes below this count as centred when deciding whether the
    /// controllers are in use (see [`crate::controllers::scheme`]).
    pub stick_deadzone: f32,
    /// Locate the hand skeleton through `XR_EXT_hand_tracking` when the runtime offers it. The
    /// joints feed the finger curls; with this off, or without runtime support, the curls are
    /// estimated from the inputs.
    pub hand_tracking: bool,
}

impl InputConfig {
    pub const fn new() -> Self {
        Self {
            trigger_press: PressThresholds {
                press: 0.7,
                release: 0.55,
            },
            squeeze_press: PressThresholds {
                press: 0.75,
                release: 0.5,
            },
            stick_deadzone: 0.3,
            hand_tracking: true,
        }
    }
}

/// On-foot stick locomotion. Turning is configured in `headpose.vr_turn` (see the module docs).
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct LocomotionConfig {
    /// The frame the off-hand stick's direction is relative to.
    pub direction: LocomotionDirection,
}

impl LocomotionConfig {
    pub const fn new() -> Self {
        Self {
            direction: LocomotionDirection::Head,
        }
    }
}

/// The frame the locomotion stick's direction is relative to.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LocomotionDirection {
    /// Forward is where the head faces (yaw only).
    #[default]
    Head,
    /// Forward is where the off-hand controller points (yaw only).
    OffHand,
}

/// The per-tick mode detector (see [`crate::controllers::mode`]). It reads the game's state and
/// selects the OpenXR action set whenever the controllers are enabled, and changes nothing the game
/// sees.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct ModeConfig {
    /// How many consecutive ticks a new gameplay mode must be detected before it takes effect. One
    /// switches at once; the default rides out a one-tick flicker during an animation blend.
    pub debounce_ticks: u32,
}

impl ModeConfig {
    pub const fn new() -> Self {
        Self { debounce_ticks: 2 }
    }
}

/// The controller-to-game bindings (see [`crate::controllers::bindings`]). They inject only under
/// the controller scheme while a controller is active, so they default on.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct BindingsConfig {
    /// Inject the controllers' input into the game's actions.
    pub enabled: bool,
    /// The radial deadzone of the stick that moves, steers, and navigates, `0..1`. The output is
    /// rescaled so that it starts from zero at the deadzone's edge.
    pub move_deadzone: f32,
    /// The trigger pull below which an analog trigger action (throttle, collective, thrust) reads
    /// as zero, `0..1`.
    pub trigger_deadzone: f32,
    /// How long the system button must be held to open the commlink rather than pause, seconds.
    pub long_press_s: f32,
    /// How far the move stick must be pushed when the wingsuit's A is pressed for the press to
    /// evade rather than open the parachute, `0..1`.
    pub evade_flick: f32,
    /// How far down the dominant stick must be flicked on foot to toggle the seated crouch, `0..1`.
    /// The stick must come back above half of it before the next flick counts.
    pub crouch_flick: f32,
    /// The yank gesture: a sharp pull of the grapple hand toward the body zips along an attached
    /// line.
    pub yank: YankConfig,
}

impl BindingsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            move_deadzone: 0.15,
            trigger_deadzone: 0.05,
            long_press_s: 0.5,
            evade_flick: 0.8,
            crouch_flick: 0.8,
            yank: YankConfig::new(),
        }
    }
}

/// The aim-state pin and the mental-state cuts (see [`crate::controllers::aim_state`]). Each part
/// has its own toggle so that it can be tested alone, and all of them act only under the
/// controller scheme unless [`without_controllers`](Self::without_controllers) is set.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct AimStateConfig {
    /// Pin the local player's weapon aim clear, so that the body stays in the no-aim animation
    /// family while firing still works.
    pub pin: bool,
    /// Also pin the grapple aim clear, which the weapon pin does not reach. It changes how the
    /// body behaves on a held tether: the body no longer turns to face the hook.
    pub pin_grapple: bool,
    /// Skip the local player's arm and upper-body aim IK tasks, because the controllers own the
    /// arms.
    pub skip_arm_ik: bool,
    /// Hold the body's aim point neutral, ahead of the body, so that nothing turns or twists the
    /// body toward where the gun points. The weapons keep aiming where the game aims them.
    pub neutral_aim_point: bool,
    /// How far ahead of the head the neutral aim point sits, metres.
    pub neutral_aim_distance_m: f32,
    /// Clear the wall-penetration block on the player's held weapon, which the game computes from
    /// where the animation would put the barrel rather than where the hand holds it.
    pub clear_wall_block: bool,
    /// Remove the combat mental state's effects on the player's body: no automatic draw when
    /// combat starts and no automatic holster when it ends. The mental state itself keeps running
    /// for AI perception, and a deliberate holster still works.
    pub mental_state_cuts: bool,
    /// Apply every enabled part above under the gamepad scheme too, for testing the pin without
    /// controllers. Off by default: it changes how the body behaves for every player.
    pub without_controllers: bool,
}

impl AimStateConfig {
    pub const fn new() -> Self {
        Self {
            pin: true,
            pin_grapple: true,
            skip_arm_ik: true,
            neutral_aim_point: true,
            neutral_aim_distance_m: 25.0,
            clear_wall_block: true,
            mental_state_cuts: true,
            without_controllers: false,
        }
    }
}

/// Controller-owned hands (see [`crate::controllers::hands`]). Every part acts only under the
/// controller scheme and only for a hand whose controller is tracked, so the parts default on.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct HandsConfig {
    /// Drive the arms with HumanIK from the shoulders to the controller poses, with the wrists
    /// taking the controller rotations.
    pub ik: bool,
    /// Re-pose the hand and forearm bones and the held weapon at render time from the latest
    /// controller pose, removing the sim tick's lag behind the controllers.
    pub late_update: bool,
    /// Switch off the game's secondary hand IK (the weapon's off-hand grip, the grapple fire arm,
    /// and the parachute handles) for every hand the controllers drive.
    pub suppress_secondary_ik: bool,
    /// The wrist targets' positional reach, `0..=1`, before the tier weight scales it.
    pub reach_t: f32,
    /// The wrist targets' rotational reach, `0..=1`, before the tier weight scales it.
    pub reach_r: f32,
    /// How far a wrist target out of reach pulls the torso after it, `0..=1`.
    pub pull: f32,
    /// How strongly a wrist resists being dragged by other effectors' pull, `0..=1`.
    pub resist: f32,
    /// Where the character's wrist sits relative to the controller's grip pose.
    pub grip: GripOffset,
    /// How long a hand takes to blend from its controller to the animation when a state yields
    /// it, seconds.
    pub yield_blend_s: f32,
    /// How long a hand takes to blend from the animation back to its controller, seconds.
    pub return_blend_s: f32,
    /// The hand tier of each scripted state.
    pub tiers: HandTierTable,
    /// The anchored tier of the grapple states, and the hang movement it drives.
    pub anchor: AnchorConfig,
}

impl HandsConfig {
    pub const fn new() -> Self {
        Self {
            ik: true,
            late_update: true,
            suppress_secondary_ik: true,
            reach_t: 1.0,
            reach_r: 1.0,
            pull: 0.0,
            resist: 0.0,
            grip: GripOffset::INDEX_BEST_GUESS,
            yield_blend_s: 0.25,
            return_blend_s: 0.35,
            tiers: HandTierTable::PLAN,
            anchor: AnchorConfig::new(),
        }
    }
}

/// The offset from a controller's OpenXR grip pose to the character's wrist, given for the right
/// hand in the grip frame and mirrored for the left (see
/// [`grip_to_wrist`](crate::controllers::hands::pose::grip_to_wrist)).
///
/// The wrist frame is the hand-tracking wrist convention: the origin at the wrist joint, -Z toward
/// the fingers, and +Y out of the back of the hand. With no adjustment the fingers point along the
/// grip's +Y, square to the handle; the adjustment rotates the hand about the grip axes.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GripOffset {
    /// The wrist joint's position in the right grip frame, metres.
    pub position_m: glam::Vec3,
    /// The hand's rotation in the right grip frame, applied about the grip's X, then Y, then Z
    /// axes, degrees.
    pub rotation_deg: glam::Vec3,
}

impl GripOffset {
    /// A best guess for the Valve Index controller, not yet fitted in the headset: the wrist a few
    /// centimetres behind and to the back-of-hand side of the grip centre, and the handle tilted
    /// toward the fingers as a power grip holds it.
    pub const INDEX_BEST_GUESS: GripOffset = GripOffset {
        position_m: glam::Vec3::new(-0.02, -0.075, 0.0),
        rotation_deg: glam::Vec3::new(-15.0, 0.0, 0.0),
    };
}

/// Per-hand aiming (see [`crate::controllers::aim`]). Acts only under the controller scheme.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct AimConfig {
    /// Aim the held guns along the dominant hand's sight line (and, dual-wielding, each pistol
    /// along its own hand's).
    pub gun: bool,
    /// Aim the grapple along the off-hand controller's ray.
    pub grapple: bool,
    /// How strongly the written aim point is pulled toward the best candidate target along each
    /// hand ray, `0..=1`. Zero disables aim assist.
    pub assist_strength: f32,
    /// The farthest a candidate may be from the hand for aim assist, metres.
    pub assist_max_distance_m: f32,
    /// How far short of an aim-assist candidate's body position the line of sight to it may be
    /// blocked and still count as clear, metres: the line ends inside the target's own body.
    pub assist_occlusion_tolerance_m: f32,
    /// How far the hand rays are cast, metres. A gun ray that hits nothing aims at this distance
    /// along the ray.
    pub max_distance_m: f32,
    /// The nearest a gun's aim point may be to the sight's origin, metres. A nearer hit is pushed
    /// out along the ray, because the game fires along the barrel instead of at an aim point near
    /// the muzzle, and skips the weapon's update entirely for one almost at the muzzle.
    pub min_target_distance_m: f32,
    /// Where the grapple ray starts relative to the off-hand's wrist, in the wrist frame (-Z toward
    /// the fingers, +Y out of the back of the hand), metres. The gauntlet sits on the forearm just
    /// behind the wrist.
    pub grapple_origin_m: glam::Vec3,
    /// Per-weapon corrections to the sight line derived from the fire point. An entry here takes
    /// precedence over the embedded sight table
    /// ([`crate::controllers::aim::sights`]); a weapon in neither aims from the fire point.
    pub sights: SightTable,
    /// The in-headset sight tuning tool.
    pub sight_tuning: SightTuningConfig,
}

impl AimConfig {
    pub const fn new() -> Self {
        Self {
            gun: true,
            grapple: true,
            assist_strength: 0.0,
            assist_max_distance_m: 150.0,
            assist_occlusion_tolerance_m: 0.75,
            max_distance_m: 500.0,
            min_target_distance_m: 2.0,
            grapple_origin_m: glam::Vec3::ZERO,
            sights: SightTable::EMPTY,
            sight_tuning: SightTuningConfig::new(),
        }
    }
}

/// The in-headset sight tuning tool (see [`crate::controllers::aim::sights::tuning`]): a marker
/// placed ahead of the head, and a capture that stores the held weapon's sight offset that makes
/// its sight line pass through the marker.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct SightTuningConfig {
    /// Offer the tuning tool: the line-up marker, the capture, and the capture chord (both grips
    /// held and the dominant A pressed, on foot, while the marker is placed).
    pub enabled: bool,
    /// How far ahead of the head the marker is placed, metres.
    pub marker_distance_m: f32,
}

impl SightTuningConfig {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            marker_distance_m: 10.0,
        }
    }
}

/// A correction to one weapon type's sight line, keyed by the weapon's name hash. The derived
/// sight line runs from the gun's current fire point along the fire point's barrel axis; the
/// correction moves its origin and turns its direction, both in the fire point's frame.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SightOverride {
    /// The weapon type's name hash
    /// ([`WeaponBase::m_WeaponNameHash`](jc3gi::equipment::weapon::WeaponBase::m_WeaponNameHash)).
    pub weapon_hash: u32,
    /// The sight's origin relative to the fire point, in the fire point's frame (+Y up, -Z along
    /// the barrel), metres.
    pub offset_m: glam::Vec3,
    /// The sight line's pitch (positive up) and yaw (positive left) relative to the barrel axis,
    /// degrees.
    pub angles_deg: glam::Vec2,
}

impl SightOverride {
    /// No correction for `weapon_hash`: the derived sight line as it is.
    pub const fn identity(weapon_hash: u32) -> Self {
        Self {
            weapon_hash,
            offset_m: glam::Vec3::ZERO,
            angles_deg: glam::Vec2::ZERO,
        }
    }
}

/// The per-weapon sight corrections: a small fixed table, so that the configuration stays `Copy`.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SightTable {
    pub entries: [Option<SightOverride>; SightTable::CAPACITY],
}

impl SightTable {
    /// How many weapons can carry a correction. The embedded sight table covers the weapons it
    /// trusts, so this holds only the weapons tuned or corrected by hand: room for every weapon the
    /// embedded table does not trust, in one session. Serde derives arrays up to this length.
    pub const CAPACITY: usize = 32;

    pub const EMPTY: SightTable = SightTable {
        entries: [None; SightTable::CAPACITY],
    };

    /// The correction for `weapon_hash`, if any.
    pub fn get(&self, weapon_hash: u32) -> Option<&SightOverride> {
        self.entries
            .iter()
            .flatten()
            .find(|entry| entry.weapon_hash == weapon_hash)
    }

    /// Set the correction for its weapon, replacing any it has. Returns whether the table holds it
    /// afterwards; `false` means the table is full.
    pub fn set(&mut self, sight: SightOverride) -> bool {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .flatten()
            .find(|entry| entry.weapon_hash == sight.weapon_hash)
        {
            *entry = sight;
            return true;
        }
        match self.entries.iter_mut().find(|entry| entry.is_none()) {
            Some(slot) => {
                *slot = Some(sight);
                true
            }
            None => false,
        }
    }

    /// Remove the correction for `weapon_hash`.
    pub fn remove(&mut self, weapon_hash: u32) {
        for entry in &mut self.entries {
            if entry.is_some_and(|e| e.weapon_hash == weapon_hash) {
                *entry = None;
            }
        }
    }
}

/// The world-space overlay layer: its readouts and the renderer's tuning. Acts only under the
/// controller scheme.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct OverlayConfig {
    /// Master switch for the layer. Off, nothing is drawn, whoever submits.
    pub enabled: bool,
    /// What the overlay draws for the guns' aim.
    pub gun_readout: GunReadout,
    /// Highlight the surface or object the grapple would attach to, while the target is valid and
    /// in range.
    pub grapple_highlight: bool,
    /// Draw a beam from the grapple wrist to the grapple's target point.
    pub grapple_beam: bool,
    /// Draw a translucent ghost at the controller of a hand the animation owns.
    pub ghost_hands: bool,
    /// Highlight the grab volume each hand is in (a holster, the foregrip, the grenade pouch).
    pub grab_volumes: bool,
    /// Draw axes at the controllers' grip and aim poses, for debugging.
    pub debug_gizmos: bool,
    /// How far behind the scene's depth, in metres, a primitive still counts as visible, so that a
    /// marker on a surface is not hidden by that surface.
    pub depth_tolerance_m: f32,
    /// The same tolerance as a fraction of the view distance, where it exceeds the absolute one: the
    /// depth buffer's precision falls with distance.
    pub depth_tolerance_rel: f32,
    /// The thinnest a line is drawn, in pixels. A line thinner than this fades instead.
    pub min_line_px: f32,
    /// The alpha scale of a ghost hand where the scene is in front of it.
    pub xray_alpha: f32,
}

impl OverlayConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            gun_readout: GunReadout::None,
            grapple_highlight: true,
            grapple_beam: true,
            ghost_hands: true,
            grab_volumes: true,
            debug_gizmos: false,
            depth_tolerance_m: 0.05,
            depth_tolerance_rel: 0.01,
            min_line_px: 1.5,
            xray_alpha: 0.25,
        }
    }
}

/// What the overlay draws for a gun's aim.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GunReadout {
    /// Nothing: the gun's own sights are the only aid.
    #[default]
    None,
    /// A dot at the hit point, at its true depth.
    Dot,
    /// A line from the muzzle to the hit point.
    Laser,
}

/// Physical weapon handling. Acts only under the controller scheme.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct WeaponsConfig {
    /// Draw and holster weapons with a grip press at the body holsters.
    pub holsters: HolstersConfig,
    /// Let the off-hand grab a two-handed weapon's foregrip, which then sets the weapon's
    /// direction.
    pub foregrip: ForegripConfig,
    /// Hold the fire inputs back while a reload, a draw, or a holster plays (the action-runs hand
    /// tier), so the weapon cannot fire for the action's duration.
    pub block_fire_during_actions: bool,
    /// The recoil offset between the controller and the rendered weapon.
    pub recoil: RecoilConfig,
    /// Controller vibration.
    pub haptics: HapticsConfig,
    /// Dual-wielded pistols split onto both hands, each fired by its own trigger and aimed by its
    /// own hand.
    pub dual_wield: DualWieldConfig,
    /// Offer the magazine gesture: the off-hand takes a magazine from its belt pouch and puts it
    /// in the held weapon's magazine well, which starts the reload. The reload button still
    /// reloads.
    pub reload_gesture: bool,
    /// The magazine gesture's pouch, well, and timing.
    pub magazine: MagazineConfig,
    /// Throw grenades physically: grab from the pouch, release the grip to throw with the hand's
    /// velocity. The grenade pouch exists only while this is on.
    pub grenade_throw: bool,
    /// The physical throw's release, warning, and fallback settings.
    pub grenade: GrenadeConfig,
    /// Trigger melee attacks with physical strikes.
    pub melee: bool,
    /// The strike detector and the melee target's hold.
    pub melee_strike: MeleeStrikeConfig,
    /// Draw a magnified image on a scoped weapon's lens while the scope is at an eye, in place of
    /// the game's precision-aim zoom, which is suppressed separately
    /// ([`ScopeConfig::suppress_precision_aim`]).
    pub scopes: bool,
    /// The scopes' precision-aim suppression and the lens's tuning.
    pub scope: ScopeConfig,
}

impl WeaponsConfig {
    pub const fn new() -> Self {
        Self {
            holsters: HolstersConfig::new(),
            foregrip: ForegripConfig::new(),
            block_fire_during_actions: true,
            recoil: RecoilConfig::new(),
            haptics: HapticsConfig::new(),
            dual_wield: DualWieldConfig::new(),
            reload_gesture: false,
            magazine: MagazineConfig::new(),
            grenade_throw: true,
            grenade: GrenadeConfig::new(),
            melee: true,
            melee_strike: MeleeStrikeConfig::new(),
            scopes: true,
            scope: ScopeConfig::new(),
        }
    }
}

/// Body scale remapping between the player's body and the character's (see
/// [`crate::controllers::body`]).
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct BodyRemapConfig {
    /// Place the eyes at the character's eye height and scale vertical head travel by the ratio of
    /// the heights. Off by default: it moves the view for every player, controllers or not.
    pub remap_head: bool,
    /// Scale the hands' offsets from the shoulders by the ratio of the arm spans. Acts only under
    /// the controller scheme.
    pub remap_hands: bool,
}

impl BodyRemapConfig {
    pub const fn new() -> Self {
        Self {
            remap_head: false,
            remap_hands: true,
        }
    }
}

/// Posture and roomscale locomotion (see [`crate::controllers::roomscale`]). These act on the
/// headset alone, controllers or not, so the behaviour-changing parts default off. The head height
/// itself is remapped only under [`BodyRemapConfig::remap_head`].
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct RoomscaleConfig {
    /// The player's posture.
    pub posture: Posture,
    /// In the automatic posture, the smallest play area (both sides, metres) that counts as a
    /// room-scale setup. A runtime reports a standing-only setup as no bounds or a very small
    /// rectangle.
    pub roomscale_min_extent_m: f32,
    /// The body chasing the physical head in the roomscale posture.
    pub chase: ChaseConfig,
    /// Crouching.
    pub crouch: CrouchConfig,
    /// Fading the view when the head goes deep into geometry the body cannot follow it through.
    pub head_fade: HeadFadeConfig,
    /// How fast the comfort fade darkens and clears.
    pub fade: FadeConfig,
    /// How the view behaves while the character enters or exits a vehicle seat.
    pub seat_transition: SeatTransition,
    /// Under [`SeatTransition::Fade`], how long the view stays dark after the seat locks, so that
    /// the re-based view settles before it clears, seconds.
    pub seat_fade_hold_s: f32,
}

impl RoomscaleConfig {
    pub const fn new() -> Self {
        Self {
            posture: Posture::Auto,
            roomscale_min_extent_m: 1.0,
            chase: ChaseConfig::new(),
            crouch: CrouchConfig::new(),
            head_fade: HeadFadeConfig::new(),
            fade: FadeConfig::new(),
            seat_transition: SeatTransition::Ride,
            seat_fade_hold_s: 0.3,
        }
    }
}

/// The player's posture.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Posture {
    /// Standing or roomscale, chosen by whether the runtime reports a room-scale boundary. The
    /// seated posture is never chosen automatically.
    #[default]
    Auto,
    /// LOCAL space: the seated eye height maps to the character's standing eye height, roomscale
    /// translation is off, and a button toggles crouching.
    Seated,
    /// STAGE space: the true floor height, roomscale translation off, physical crouching.
    Standing,
    /// STAGE space with the roomscale chase loop on.
    Roomscale,
}

/// The roomscale chase loop: in the roomscale posture, the body walks toward the physical head.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct ChaseConfig {
    /// Off by default: it moves the character from head motion, which the headset alone produces.
    pub enabled: bool,
    /// Horizontal head offsets from the body below this many metres are left alone. A chase that
    /// has started runs until the offset is below half of it.
    pub deadzone_m: f32,
    /// The fastest the chase moves the body, metres per second.
    pub max_speed_mps: f32,
    /// How quickly the chase closes the offset: the chase speed is the offset divided by this,
    /// seconds, before the speed limit. A few simulation ticks, so that the body does not overshoot
    /// the head.
    pub time_constant_s: f32,
    /// The shortest single-tick jump of the body, metres, that counts as a teleport the game made
    /// without announcing it. The threshold also grows with the body's speed.
    pub warp_min_m: f32,
}

impl ChaseConfig {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            deadzone_m: 0.10,
            max_speed_mps: 3.0,
            time_constant_s: 0.15,
            warp_min_m: 8.0,
        }
    }
}

/// Crouching.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct CrouchConfig {
    /// Fold the body with the lowered head through the body IK: the feet stay planted and the hips
    /// drop. Off by default: it changes the body for every player who ducks.
    pub visual: bool,
    /// Shorten the character's collision capsule while crouched, with the game's own crouching
    /// proxy state. Requires [`visual`](Self::visual). The game hands the proxy state to its own
    /// states (swimming, vehicles, the parachute, ragdoll) whenever they want it.
    pub capsule: bool,
    /// How much of the head's drop the hips take, `0..=1`. A squat lowers the hips nearly as far as
    /// the head; the spine takes the rest.
    pub hips_share: f32,
    /// How far (metres, at the character's scale) the head must drop below the standing eye height
    /// before the character counts as crouched, for the capsule and the readouts.
    pub enter_drop_m: f32,
    /// How far the head must come back up (metres below the standing eye height) before the
    /// character counts as standing again.
    pub exit_drop_m: f32,
    /// In the seated posture, how far (metres, at the character's scale) the crouch toggle lowers
    /// the eyes.
    pub seated_drop_m: f32,
}

impl CrouchConfig {
    pub const fn new() -> Self {
        Self {
            visual: false,
            capsule: false,
            hips_share: 0.8,
            enter_drop_m: 0.45,
            exit_drop_m: 0.30,
            seated_drop_m: 0.6,
        }
    }
}

/// The head-through-geometry fade.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct HeadFadeConfig {
    /// Off by default: it darkens the view for every player whose head enters a wall.
    pub enabled: bool,
    /// How deep (metres) past the obstruction the head may go before the view starts to darken.
    pub start_depth_m: f32,
    /// How deep (metres) past the obstruction the view is fully dark.
    pub full_depth_m: f32,
}

impl HeadFadeConfig {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            start_depth_m: 0.05,
            full_depth_m: 0.25,
        }
    }
}

/// The comfort fade's timing, shared by every source (the head in geometry, the seat transitions).
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct FadeConfig {
    /// How long a fade from clear to fully dark takes, seconds.
    pub out_s: f32,
    /// How long a fade from fully dark to clear takes, seconds.
    pub in_s: f32,
}

impl FadeConfig {
    pub const fn new() -> Self {
        Self {
            out_s: 0.15,
            in_s: 0.3,
        }
    }
}

/// How the view behaves while the character enters or exits a vehicle seat.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SeatTransition {
    /// Re-base the cockpit the moment the seat locks. The entry animation plays behind a fade that
    /// clears at the lock; the exit plays live, and the view re-bases its yaw and height once the
    /// character is out.
    Instant,
    /// Fade out when the entry animation takes the camera, and fade in seated once the re-based
    /// view has settled. The exit also plays behind a fade.
    Fade,
    /// Keep the head anchored through the whole animation with nothing hidden. The default until
    /// the other modes are proven, because it is the mod's existing behaviour.
    #[default]
    Ride,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sight_table_keys_by_weapon_hash() {
        let mut table = SightTable::EMPTY;
        assert!(table.set(SightOverride::identity(7)));
        let raised = SightOverride {
            offset_m: glam::Vec3::Y * 0.05,
            ..SightOverride::identity(7)
        };
        assert!(table.set(raised));
        assert_eq!(table.entries.iter().flatten().count(), 1);
        assert_eq!(table.get(7), Some(&raised));
        table.remove(7);
        assert!(table.get(7).is_none());
    }

    #[test]
    fn a_full_sight_table_refuses_a_new_weapon() {
        let mut table = SightTable::EMPTY;
        for hash in 0..SightTable::CAPACITY as u32 {
            assert!(table.set(SightOverride::identity(hash)));
        }
        assert!(!table.set(SightOverride::identity(1000)));
        assert!(table.set(SightOverride::identity(3)));
    }
}
