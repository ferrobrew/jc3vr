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

use crate::controllers::frame::{Hand, PressThresholds};

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
    /// Vehicles, the wingsuit, and the parachute.
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
}

impl BindingsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            move_deadzone: 0.15,
            trigger_deadzone: 0.05,
            long_press_s: 0.5,
            evade_flick: 0.8,
        }
    }
}

/// The aim-state pin and the mental-state cuts. Both act only under the controller scheme.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct AimStateConfig {
    /// Pin the local player's aim state clear, so that the body stays in the no-aim animation
    /// family, the game's arm aim IK never runs, and the controllers own the arms.
    pub pin: bool,
    /// Remove the combat mental state's effects on the player's body: no automatic draw when
    /// combat starts and no automatic holster when it ends. The mental state itself keeps running
    /// for AI perception.
    pub mental_state_cuts: bool,
}

impl AimStateConfig {
    pub const fn new() -> Self {
        Self {
            pin: true,
            mental_state_cuts: true,
        }
    }
}

/// Controller-owned hands. Both act only under the controller scheme.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct HandsConfig {
    /// Drive the arms with HumanIK from the shoulders to the controller poses, with the wrists
    /// taking the controller rotations.
    pub ik: bool,
    /// Re-pose the hand bones and the held weapon at render time from the latest controller pose,
    /// removing the sim tick's lag behind the controllers.
    pub late_update: bool,
}

impl HandsConfig {
    pub const fn new() -> Self {
        Self {
            ik: true,
            late_update: true,
        }
    }
}

/// Per-hand aiming. Acts only under the controller scheme.
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
}

impl AimConfig {
    pub const fn new() -> Self {
        Self {
            gun: true,
            grapple: true,
            assist_strength: 0.0,
        }
    }
}

/// The world-space overlay layer's readouts. Act only under the controller scheme.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct OverlayConfig {
    /// What the overlay draws for the guns' aim.
    pub gun_readout: GunReadout,
    /// Highlight the surface or object the grapple would attach to, while the target is valid and
    /// in range.
    pub grapple_highlight: bool,
    /// Draw a beam from the grapple wrist to the grapple's target point.
    pub grapple_beam: bool,
    /// Draw axes at the controllers' grip and aim poses, for debugging.
    pub debug_gizmos: bool,
}

impl OverlayConfig {
    pub const fn new() -> Self {
        Self {
            gun_readout: GunReadout::None,
            grapple_highlight: true,
            grapple_beam: true,
            debug_gizmos: false,
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
    pub holsters: bool,
    /// Let the off-hand grab a two-handed weapon's foregrip, which then sets the weapon's
    /// direction.
    pub foregrip: bool,
    /// The recoil offset between the controller and the rendered weapon.
    pub recoil: RecoilConfig,
    /// Controller vibration.
    pub haptics: HapticsConfig,
    /// Split dual-wielded pistols onto both hands, each fired by its own trigger and aimed by its
    /// own hand.
    pub dual_wield_split: bool,
    /// Replace the reload button with the magazine gesture. Off, the reload stays on the button.
    pub reload_gesture: bool,
    /// Throw grenades physically: grab from the pouch, release the grip to throw with the hand's
    /// velocity.
    pub grenade_throw: bool,
    /// Trigger melee attacks with physical strikes.
    pub melee: bool,
    /// Render a magnified image into scoped weapons' lenses, replacing the game's precision-aim
    /// zoom.
    pub scopes: bool,
}

impl WeaponsConfig {
    pub const fn new() -> Self {
        Self {
            holsters: true,
            foregrip: true,
            recoil: RecoilConfig::new(),
            haptics: HapticsConfig::new(),
            dual_wield_split: true,
            reload_gesture: false,
            grenade_throw: true,
            melee: true,
            scopes: true,
        }
    }
}

/// The recoil offset: each shot kicks the rendered weapon away from the controller pose, and a
/// damped spring returns it.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct RecoilConfig {
    pub enabled: bool,
    /// Scales the kick derived from the game's per-weapon recoil accumulators.
    pub kick_scale: f32,
    /// The return spring's stiffness, per second squared.
    pub spring_stiffness: f32,
    /// The return spring's damping ratio: `1` is critically damped, below `1` overshoots.
    pub spring_damping_ratio: f32,
    /// Multiplies the stiffness while the off-hand holds the foregrip.
    pub two_hand_stiffness_scale: f32,
    /// Multiplies the kick while the off-hand holds the foregrip.
    pub two_hand_kick_scale: f32,
}

impl RecoilConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            kick_scale: 1.0,
            spring_stiffness: 400.0,
            spring_damping_ratio: 0.8,
            two_hand_stiffness_scale: 2.0,
            two_hand_kick_scale: 0.5,
        }
    }
}

/// Controller vibration.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct HapticsConfig {
    /// Off, every queued pulse is dropped.
    pub enabled: bool,
    /// Scales every pulse's amplitude, `0..=1`.
    pub intensity: f32,
}

impl HapticsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            intensity: 1.0,
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

/// Posture and roomscale locomotion. These act on the headset alone, controllers or not, so the
/// behaviour-changing parts default off.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct RoomscaleConfig {
    /// The player's posture.
    pub posture: Posture,
    /// The body chasing the physical head in the roomscale posture.
    pub chase: ChaseConfig,
    /// Crouching.
    pub crouch: CrouchConfig,
    /// Fade the view when the head goes deep into geometry the body cannot follow it through.
    pub head_fade: bool,
    /// How the view behaves while the character enters or exits a vehicle seat.
    pub seat_transition: SeatTransition,
}

impl RoomscaleConfig {
    pub const fn new() -> Self {
        Self {
            posture: Posture::Auto,
            chase: ChaseConfig::new(),
            crouch: CrouchConfig::new(),
            head_fade: false,
            seat_transition: SeatTransition::Ride,
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
    /// Horizontal head offsets from the body below this many metres are left alone.
    pub deadzone_m: f32,
    /// The fastest the chase moves the body, metres per second.
    pub max_speed_mps: f32,
}

impl ChaseConfig {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            deadzone_m: 0.10,
            max_speed_mps: 3.0,
        }
    }
}

/// Crouching.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct CrouchConfig {
    /// Fold the body with the lowered head through the body IK: the feet stay planted and the hips
    /// drop. Off by default: it changes the body for every player who ducks.
    pub visual: bool,
    /// Shorten the character's collision capsule while crouched. Requires [`visual`](Self::visual).
    pub capsule: bool,
    /// In the seated posture, how far (metres) the crouch toggle lowers the eyes.
    pub seated_drop_m: f32,
}

impl CrouchConfig {
    pub const fn new() -> Self {
        Self {
            visual: false,
            capsule: false,
            seated_drop_m: 0.6,
        }
    }
}

/// How the view behaves while the character enters or exits a vehicle seat.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SeatTransition {
    /// Re-base the cockpit at seat lock, hiding the entry animation behind a fade.
    Instant,
    /// Fade out when the entry animation takes the camera, and fade in seated.
    Fade,
    /// Keep the head anchored through the whole animation with nothing hidden. The default until
    /// the other modes are proven, because it is the mod's existing behaviour.
    #[default]
    Ride,
}

/// Vehicles, the wingsuit, and the parachute. The physical controls are opt-in; stick bindings are
/// the default.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct VehiclesConfig {
    /// Drive with physical controls: a wheel, handlebars, a helicopter cyclic and collective, a
    /// flight stick, and a throttle, grabbed with the grip.
    pub physical_controls: bool,
    /// Aim the driver's vehicle weapons along the dominant hand's ray instead of the gaze.
    pub driver_weapons_by_hand: bool,
    /// Steer the wingsuit with the arm pose.
    pub wingsuit_arm_steering: bool,
    /// Steer the parachute with the arm pose.
    pub parachute_arm_steering: bool,
}

impl VehiclesConfig {
    pub const fn new() -> Self {
        Self {
            physical_controls: false,
            driver_weapons_by_hand: false,
            wingsuit_arm_steering: false,
            parachute_arm_steering: false,
        }
    }
}

/// Finger curls on the rendered hands. Act only under the controller scheme.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct FingersConfig {
    /// Curl the fingers from hand tracking when available, and otherwise from the inputs the
    /// fingers rest on (the trigger, the grip, and the touch sensors).
    pub enabled: bool,
}

impl FingersConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}
