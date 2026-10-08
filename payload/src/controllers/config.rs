//! The motion-controller configuration: the master switch, the scheme, and one sub-struct per
//! subsystem of the plan of record (`docs/mod/input/controllers-and-roomscale.md`), each holding its
//! toggle and the tuning beside it.
//!
//! `ControllersConfig::new()` ships as the full gameplay preset
//! ([`presets::Preset::VehiclesAndAdvanced`](crate::controllers::presets::Preset::VehiclesAndAdvanced)),
//! so that hand aiming, roomscale, weapon handling, vehicle steering (including the wingsuit's arm
//! steering and the parachute's handles), and the debug gizmos all work, and show, out of the box.
//! `presets::preset_modified` checked against that preset stays false for the defaults.
//!
//! Stick turning is not configured here: the controller scheme's right stick drives the same body
//! yaw as the gamepad's look input, so it shares
//! [`VrTurnConfig`](crate::headpose::config::VrTurnConfig) (`headpose.vr_turn`) rather than keeping
//! a second copy of the smooth and snap settings.

use serde::{Deserialize, Serialize};

pub use crate::controllers::aim::sights::config::{SightOverride, SightTable, SightTuningConfig};
use crate::{
    controllers::{
        bindings::{panel_chord, yank::YankConfig},
        body::{alignment::AlignmentConfig, procedural::config::ProceduralBodyConfig},
        fingers::config::FingersConfig,
        frame::{GripPress, Hand, InteractionProfile, PressRules, PressThresholds},
        hands::{anchor::AnchorConfig, reach::ReachConfig, tiers::HandTierTable},
        locomotion::config::{ActFilterConfig, StableViewConfig, StickConfig},
        physical_hands::PhysicalHandsConfig,
        roomscale::stance::StanceConfig,
        summary::SummaryConfig,
        vehicles::config::VehiclesConfig,
        weapons::{
            ads::config::AdsConfig,
            collide::config::CollideConfig,
            config::{
                C4Config, DualWieldConfig, ForegripConfig, GrenadeConfig, HapticsConfig,
                HeftConfig, HoldConfig, HolstersConfig, PouchesConfig, RecoilConfig, ScopeConfig,
            },
            reload::config::ReloadConfig,
            wheel::config::WheelConfig,
        },
    },
    grapple::physical::PhysicalGrappleConfig,
    vr::input::layout::{FloatInput, Input, provides},
};

/// All motion-controller settings, added to [`crate::config::Config`] as `controllers`.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ControllersConfig {
    /// Master switch. Off, the mod reads no controller input and publishes no controller frame, so
    /// every controller subsystem is inert and the gamepad scheme is in effect. The OpenXR actions
    /// are still created and attached with the session, because attachment is possible only once
    /// per session and turning this on later must not require a new one.
    pub enabled: bool,
    /// Which control scheme is in effect (see [`SchemeSetting`]).
    pub scheme: SchemeSetting,
    /// The hand that holds and fires the guns. The other hand fires the grapple. Mirroring swaps the
    /// bindings and the dual-wield pistols' attach bones; a single gun stays on the model's right
    /// hand, and the grapple gauntlet on the model's left arm. The hand roles follow
    /// [`ControllersConfig::acting_hand`], which differs from this until a restart after a change.
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
    /// The palms' collision with the world.
    pub physical_hands: PhysicalHandsConfig,
    /// The controls summary, and the game's own button hints.
    pub summary: SummaryConfig,
    /// The physical grapple: the reel, the tether retract, and the hang swing worked by the grapple
    /// hand.
    pub grapple: PhysicalGrappleConfig,
}

impl ControllersConfig {
    /// The dominant hand the mod's hand roles follow: the gun hand, the grapple's ray, the
    /// readouts on the wrists, the mirrored sticks and levers, and the bindings' reading of the
    /// buttons. While controller bindings are attached it is the hand they were suggested for,
    /// since OpenXR keeps those until the game restarts and the buttons stay laid out for it; a
    /// change of [`ControllersConfig::dominant_hand`] takes effect for the roles at the same
    /// restart as for the buttons, so the two never disagree.
    pub fn acting_hand(&self) -> Hand {
        acting_hand(self.dominant_hand, crate::vr::input::bound_hand())
    }

    pub const fn new() -> Self {
        Self {
            enabled: true,
            scheme: SchemeSetting::Controllers,
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
            physical_hands: PhysicalHandsConfig::new(),
            summary: SummaryConfig::new(),
            grapple: PhysicalGrappleConfig::new(),
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
#[serde(default)]
pub struct InputConfig {
    /// The trigger values that press and release
    /// [`Button::TriggerPress`](crate::controllers::frame::Button::TriggerPress).
    pub trigger_press: PressThresholds,
    /// The squeeze values that press and release
    /// [`Button::SqueezePress`](crate::controllers::frame::Button::SqueezePress) on a controller
    /// without a force-sensing grip (Touch, or a profile the runtime emulates). The grip is used
    /// only as an edge, so the press threshold sits high enough that resting a hand on the grip
    /// does not press it.
    pub squeeze_press: PressThresholds,
    /// The squeeze forces that press and release
    /// [`Button::SqueezePress`](crate::controllers::frame::Button::SqueezePress) on a controller
    /// whose profile provides the grip force (the Index). The Index grip's value is capacitive and
    /// reads a full squeeze with the hand merely closed round the handle, so the press waits for a
    /// deliberate squeeze, and the release sits low enough that a held grab does not need a hard
    /// squeeze to keep.
    pub squeeze_force_press: PressThresholds,
    /// The trackpad forces that press and release
    /// [`Button::TrackpadPressLeft`](crate::controllers::frame::Button::TrackpadPressLeft) and
    /// [`Button::TrackpadPressRight`](crate::controllers::frame::Button::TrackpadPressRight), on
    /// a controller with a trackpad. A resting thumb reads no force, so a light press is enough.
    pub trackpad_press: PressThresholds,
    /// Thumbstick and trackpad magnitudes below this count as centred when deciding whether the
    /// controllers are in use (see [`crate::controllers::scheme`]).
    pub stick_deadzone: f32,
    /// Locate the hand skeleton through `XR_EXT_hand_tracking` when the runtime offers it. The
    /// joints feed the finger curls; with this off, or without runtime support, the curls are
    /// estimated from the inputs.
    pub hand_tracking: bool,
    /// Run the input self-check once per session, logging how the runtime treats the action
    /// layout (see `crate::vr::input::self_check`).
    pub self_check: bool,
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
            squeeze_force_press: PressThresholds {
                press: 0.3,
                release: 0.1,
            },
            trackpad_press: PressThresholds {
                press: 0.4,
                release: 0.2,
            },
            stick_deadzone: 0.3,
            hand_tracking: true,
            self_check: true,
        }
    }

    /// What derives `hand`'s grip press under `profile`: the squeeze force where the profile
    /// provides it, and the squeeze value elsewhere.
    pub fn grip_press(&self, profile: InteractionProfile, hand: Hand) -> GripPress {
        if provides(profile, hand, Input::Float(FloatInput::SqueezeForce)) {
            GripPress::Force(self.squeeze_force_press)
        } else {
            GripPress::Value(self.squeeze_press)
        }
    }

    /// How `hand`'s derived presses are read under `profile`.
    pub fn press_rules(&self, profile: InteractionProfile, hand: Hand) -> PressRules {
        PressRules {
            trigger: self.trigger_press,
            grip: self.grip_press(profile, hand),
            trackpad: self.trackpad_press,
        }
    }
}

impl Default for InputConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// On-foot stick locomotion. Turning is configured in `headpose.vr_turn` (see the module docs).
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LocomotionConfig {
    /// The frame the off-hand stick's direction is relative to.
    pub direction: LocomotionDirection,
    /// The stick's velocity, with fixed acceleration.
    pub stick: StickConfig,
    /// The filter that keeps the turning animations off the body.
    pub acts: ActFilterConfig,
    /// The on-foot view anchored to Rico's root rather than his animated head.
    pub view: StableViewConfig,
}

impl LocomotionConfig {
    pub const fn new() -> Self {
        Self {
            direction: LocomotionDirection::Head,
            stick: StickConfig::new(),
            acts: ActFilterConfig::new(),
            view: StableViewConfig::new(),
        }
    }
}

impl Default for LocomotionConfig {
    fn default() -> Self {
        Self::new()
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
#[serde(default)]
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

impl Default for ModeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The controller-to-game bindings (see [`crate::controllers::bindings`]). They inject only under
/// the controller scheme while a controller is active, so they default on.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    /// evade rather than open the parachute, `0..1`. The stick rests below half of it.
    pub evade_flick: f32,
    /// How recently the move stick must have left its rest for the wingsuit's A to evade, seconds:
    /// a flick, not a stick held over to steer.
    pub evade_window_s: f32,
    /// How far down the dominant stick must be flicked on foot to toggle the seated crouch, `0..1`.
    /// The stick must come back above half of it before the next flick counts.
    pub crouch_flick: f32,
    /// How far either stick must be pushed along one axis to step a menu, `0..1`. The step holds,
    /// and the game repeats it, until the push falls back below half of it.
    pub menu_stick_push: f32,
    /// How long a lone stick click waits for the other before it acts, seconds: both clicks pressed
    /// within it are the recenter chord, and neither acts on its own.
    pub stick_chord_window_s: f32,
    /// How long both stick clicks must be held together to recenter the VR view, seconds.
    pub recenter_hold_s: f32,
    /// How soon after the left stick click the right B must be pressed to show or hide the debug
    /// panel, seconds ([`crate::controllers::bindings::panel_chord`]). A lone left stick click
    /// waits this long, or the stick click chord window if that is longer, before it acts.
    pub panel_chord_window_s: f32,
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
            evade_window_s: 0.25,
            crouch_flick: 0.8,
            menu_stick_push: 0.5,
            stick_chord_window_s: 0.1,
            recenter_hold_s: 1.0,
            panel_chord_window_s: 0.35,
            yank: YankConfig::new(),
        }
    }

    /// How long a lone stick click waits before it acts, seconds, by [`Hand::index`]: the stick
    /// click chord window, and on the left also the debug panel chord's window.
    pub fn stick_click_windows(&self) -> [f32; 2] {
        Hand::BOTH.map(|hand| {
            if hand == panel_chord::CLICK_HAND {
                self.stick_chord_window_s.max(self.panel_chord_window_s)
            } else {
                self.stick_chord_window_s
            }
        })
    }
}

impl Default for BindingsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The aim-state pin and the mental-state cuts (see [`crate::controllers::aim_state`]). Each part
/// has its own toggle so that it can be tested alone, and all of them act only under the
/// controller scheme unless [`without_controllers`](Self::without_controllers) is set.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
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

impl Default for AimStateConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Controller-owned hands (see [`crate::controllers::hands`]). Every part acts only under the
/// controller scheme and only for a hand whose controller is tracked, so the parts default on.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    /// Hold a gun in a controller-driven hand fixed to the wrist at the hand's bind-pose grip,
    /// rather than where the animation puts the attach bone.
    pub rigid_grip: bool,
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
    /// How the character's arm reaches for the controllers: the remap about his measured shoulders,
    /// the pull near full extension, and the render-time detach.
    pub reach: ReachConfig,
}

impl HandsConfig {
    pub const fn new() -> Self {
        Self {
            ik: true,
            late_update: true,
            suppress_secondary_ik: true,
            rigid_grip: true,
            reach_t: 1.0,
            reach_r: 1.0,
            pull: 0.0,
            resist: 0.0,
            grip: GripOffset::INDEX_BEST_GUESS,
            yield_blend_s: 0.25,
            return_blend_s: 0.35,
            tiers: HandTierTable::PLAN,
            anchor: AnchorConfig::new(),
            reach: ReachConfig::new(),
        }
    }
}

impl Default for HandsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The offset from a controller's OpenXR grip pose to the character's wrist, given for the right
/// hand in the grip frame and mirrored for the left (see
/// [`grip_to_wrist`](crate::controllers::hands::pose::grip_to_wrist)).
///
/// The wrist frame is the hand-tracking wrist convention: the origin at the wrist joint, -Z toward
/// the fingers, and +Y out of the back of the hand. With no adjustment the fingers point along the
/// grip's -Y, square to the handle, with the back of the right hand along the grip's +X; the
/// adjustment rotates the hand about the grip axes. The grip frame is OpenXR's: -Z up the handle
/// toward the thumb, +X into the right palm, and +Y back toward the wrist.
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
        position_m: glam::Vec3::new(0.02, 0.075, 0.0),
        rotation_deg: glam::Vec3::new(15.0, 0.0, 0.0),
    };
}

/// Per-hand aiming (see [`crate::controllers::aim`]). Acts only under the controller scheme.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
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

impl Default for AimConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The world-space overlay layer: its readouts and the renderer's tuning. Acts only under the
/// controller scheme.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    /// Hide the game's own weapon reticle wherever the guns aim by hand (on foot whether or not a
    /// gun is out, and in a vehicle while its weapons aim by hand), and its grapple reticle
    /// everywhere under the controller scheme. Both mark the gaze's aim point (see
    /// [`crate::controllers::overlay::game_reticle`]). Independent of [`Self::enabled`].
    pub hide_game_reticle: bool,
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
            hide_game_reticle: true,
            ghost_hands: false,
            grab_volumes: false,
            debug_gizmos: false,
            depth_tolerance_m: 0.05,
            depth_tolerance_rel: 0.01,
            min_line_px: 1.5,
            xray_alpha: 0.25,
        }
    }
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self::new()
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
#[serde(default)]
pub struct WeaponsConfig {
    /// Draw and holster weapons with a grip press at the body holsters.
    pub holsters: HolstersConfig,
    /// The pouches on the front of the body: the magazines, the grenades, and the C4.
    pub pouches: PouchesConfig,
    /// Let the off-hand grab a two-handed weapon's foregrip, which then sets the weapon's
    /// direction.
    pub foregrip: ForegripConfig,
    /// Hold the fire inputs back while a reload, a draw, or a holster plays (the action-runs hand
    /// tier), so the weapon cannot fire for the action's duration.
    pub block_fire_during_actions: bool,
    /// The recoil offset between the controller and the rendered weapon.
    pub recoil: RecoilConfig,
    /// The one-handed weight of long guns: sag, lag, sway, a stronger kick, and a wider spread,
    /// all gone while the off-hand braces the weapon on its foregrip.
    pub heft: HeftConfig,
    /// Controller vibration.
    pub haptics: HapticsConfig,
    /// Dual-wielded pistols split onto both hands, each fired by its own trigger and aimed by its
    /// own hand.
    pub dual_wield: DualWieldConfig,
    /// Which hand holds the weapon: the hand that drew it, picked it up, or was handed it.
    pub hold: HoldConfig,
    /// The true reload: eject, load from the belt pouch, and chamber by hand, in place of the
    /// game's timed reload.
    pub reload: ReloadConfig,
    /// Throw grenades physically: grab from the chest pouch, release the grip to throw with the
    /// hand's velocity. The grenade pouch exists only while this is on.
    pub grenade_throw: bool,
    /// The physical throw's release, warning, and fallback settings.
    pub grenade: GrenadeConfig,
    /// Throw C4 physically: grab a charge from the chest pouch, release the grip to throw it with
    /// the hand's velocity, and it sticks where it lands. The C4 pouch exists only while this is
    /// on.
    pub c4_throw: bool,
    /// Detonate the placed charges by pushing the dominant index fingertip into the button on the
    /// inside of the off-hand's wrist.
    pub wrist_detonator: bool,
    /// The C4 pouch, the throw's release, and the wrist detonator's placement and press tuning.
    pub c4: C4Config,
    /// Draw a magnified image on a scoped weapon's lens while the scope is at an eye, in place of
    /// the game's precision-aim zoom, which is suppressed separately
    /// ([`ScopeConfig::suppress_precision_aim`]).
    pub scopes: bool,
    /// The scopes' precision-aim suppression and the lens's tuning.
    pub scope: ScopeConfig,
    /// Let the game switch to another weapon when the trigger of an empty weapon that cannot
    /// reload is pulled. Off, the empty weapon stays in the hand until it is holstered or another
    /// is drawn, and the pull only dry-fires.
    pub auto_switch_when_empty: bool,
    /// Let the fire input draw the selected weapon with nothing in the hands, as the game does.
    /// Off, a weapon comes out only from a holster, the weapon wheel, or a slot select, and the
    /// precision-aim input does not draw either.
    pub fire_draws_weapon: bool,
    /// The weapon wheel: the game's selection cross, opened with a held input and steered by the
    /// hand.
    pub wheel: WheelConfig,
    /// Aiming a long gun down its sights: the sights onto the eye, the stock into the shoulder,
    /// and a scope's cheek weld.
    pub ads: AdsConfig,
    /// Held long guns stop at walls: turned aside and pulled back into the hands.
    pub collide: CollideConfig,
}

impl WeaponsConfig {
    pub const fn new() -> Self {
        Self {
            holsters: HolstersConfig::new(),
            pouches: PouchesConfig::new(),
            foregrip: ForegripConfig::new(),
            block_fire_during_actions: true,
            recoil: RecoilConfig::new(),
            heft: HeftConfig::new(),
            haptics: HapticsConfig::new(),
            dual_wield: DualWieldConfig::new(),
            hold: HoldConfig::new(),
            reload: ReloadConfig::new(),
            grenade_throw: true,
            grenade: GrenadeConfig::new(),
            c4_throw: true,
            wrist_detonator: true,
            c4: C4Config::new(),
            scopes: true,
            scope: ScopeConfig::new(),
            auto_switch_when_empty: false,
            fire_draws_weapon: false,
            wheel: WheelConfig::new(),
            ads: AdsConfig::new(),
            collide: CollideConfig::new(),
        }
    }
}

impl Default for WeaponsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Body scale remapping between the player's body and the character's (see
/// [`crate::controllers::body`]).
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BodyRemapConfig {
    /// Place the eyes at the character's eye height and scale vertical head travel by the ratio of
    /// the heights. On by default, as part of the full gameplay preset; it moves the view for
    /// every player, controllers or not.
    pub remap_head: bool,
    /// Scale the hands' offsets from the shoulders by the ratio of the arm spans. Acts only under
    /// the controller scheme.
    pub remap_hands: bool,
    /// Keeping the character's body aligned with the player's: the torso estimate, the body
    /// turning under the view, and the chest turn.
    pub alignment: AlignmentConfig,
    /// The procedural body: Rico's pose built from the player's head and body.
    pub procedural: ProceduralBodyConfig,
}

impl BodyRemapConfig {
    pub const fn new() -> Self {
        Self {
            remap_head: true,
            remap_hands: true,
            alignment: AlignmentConfig::new(),
            procedural: ProceduralBodyConfig::new(),
        }
    }
}

impl Default for BodyRemapConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Posture and roomscale locomotion (see [`crate::controllers::roomscale`]). These act on the
/// headset alone, controllers or not, and default on as part of the full gameplay preset. The head
/// height itself is remapped only under [`BodyRemapConfig::remap_head`].
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RoomscaleConfig {
    /// The player's posture.
    pub posture: Posture,
    /// In the automatic posture, the smallest play area (both sides, metres) that counts as a
    /// room-scale setup. A runtime reports a standing-only setup as no bounds or a very small
    /// rectangle.
    pub roomscale_min_extent_m: f32,
    /// In the automatic posture, how the head's height above the floor tells a seated player from
    /// a standing one.
    pub auto_posture: AutoPostureConfig,
    /// In the seated posture, how the head height that maps to the character's eyes follows the
    /// player.
    pub seated_reference: SeatedReferenceConfig,
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
    /// Keeping the standing stance square with the player's.
    pub stance: StanceConfig,
}

impl RoomscaleConfig {
    pub const fn new() -> Self {
        Self {
            posture: Posture::Auto,
            roomscale_min_extent_m: 1.0,
            auto_posture: AutoPostureConfig::new(),
            seated_reference: SeatedReferenceConfig::new(),
            chase: ChaseConfig::new(),
            crouch: CrouchConfig::new(),
            head_fade: HeadFadeConfig::new(),
            fade: FadeConfig::new(),
            seat_transition: SeatTransition::Ride,
            seat_fade_hold_s: 0.3,
            stance: StanceConfig::new(),
        }
    }
}

impl Default for RoomscaleConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The player's posture.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Posture {
    /// Seated or standing by the head's height above the floor ([`AutoPostureConfig`]), seated
    /// until the head has been located; standing becomes roomscale when the runtime reports a
    /// room-scale boundary.
    #[default]
    Auto,
    /// The seated eye reference ([`SeatedReferenceConfig`]) maps to the character's standing eye
    /// height, roomscale translation is off, and a button toggles crouching.
    Seated,
    /// STAGE space: the true floor height, roomscale translation off, physical crouching.
    Standing,
    /// STAGE space with the roomscale chase loop on.
    Roomscale,
}

/// The automatic posture's seated and standing detection
/// ([`crate::controllers::roomscale::posture::PostureClassifier`]). The thresholds sit between the
/// calibrated seated eye height (or an estimate from the standing eye height) and the standing eye
/// height; the dwell times decide how long the head must stay past them.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoPostureConfig {
    /// How long a seated player's head must stay at standing height before the posture becomes
    /// standing, seconds. Short: a seated player cannot raise the head that far without standing
    /// up.
    pub stand_after_s: f32,
    /// How long a standing player's head must stay at seated height before the posture becomes
    /// seated, seconds. Long, so that a standing player crouching behind cover keeps the physical
    /// crouch.
    pub sit_after_s: f32,
    /// Count the sit wait only while a standing player's head is steady at a chair's height: a
    /// head at floor level (sitting or kneeling on the floor of the play area) or one that bobs
    /// (a crouch peeking out of cover) stays standing.
    pub chair_height_only: bool,
}

impl AutoPostureConfig {
    pub const fn new() -> Self {
        Self {
            stand_after_s: 1.0,
            sit_after_s: 20.0,
            chair_height_only: true,
        }
    }
}

impl Default for AutoPostureConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The seated eye reference ([`crate::controllers::roomscale::seated`]): the head height above the
/// floor that the seated posture puts at the character's eyes. It starts at the calibrated seated
/// eye height, a manual recenter while seated captures the head's height, and the adaptive
/// baseline moves it toward how the player actually sits.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SeatedReferenceConfig {
    /// Move the reference toward the median head height over the window. On by default: without it
    /// a reference that does not match how the player sits holds the view above or below the
    /// character's eyes until the next manual recenter.
    pub adaptive: bool,
    /// The window the median is taken over, seconds. Head movement held for less than half of it
    /// (a lean, a duck, a look around) does not move the median.
    pub window_s: f32,
    /// The fastest the adaptive baseline moves the reference, metres per second.
    pub rate_mps: f32,
    /// How far the median must be from the reference before the adaptive baseline moves it,
    /// metres.
    pub deadband_m: f32,
}

impl SeatedReferenceConfig {
    pub const fn new() -> Self {
        Self {
            adaptive: true,
            window_s: 120.0,
            rate_mps: 0.01,
            deadband_m: 0.02,
        }
    }
}

impl Default for SeatedReferenceConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The roomscale chase loop: in the roomscale posture, the body walks toward the physical head.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChaseConfig {
    /// On by default, as part of the full gameplay preset: it moves the character from head
    /// motion, which the headset alone produces.
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
            enabled: true,
            deadzone_m: 0.10,
            max_speed_mps: 3.0,
            time_constant_s: 0.15,
            warp_min_m: 8.0,
        }
    }
}

impl Default for ChaseConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Crouching.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CrouchConfig {
    /// Fold the body with the lowered head through the body IK: the feet stay planted and the hips
    /// drop. On by default, as part of the full gameplay preset: it changes the body for every
    /// player who ducks.
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
            visual: true,
            capsule: true,
            hips_share: 0.8,
            enter_drop_m: 0.45,
            exit_drop_m: 0.30,
            seated_drop_m: 0.6,
        }
    }
}

impl Default for CrouchConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The head-through-geometry fade.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HeadFadeConfig {
    /// On by default, as part of the full gameplay preset: it darkens the view for every player
    /// whose head enters a wall.
    pub enabled: bool,
    /// How deep (metres) past the obstruction the head may go before the view starts to darken.
    pub start_depth_m: f32,
    /// How deep (metres) past the obstruction the view is fully dark.
    pub full_depth_m: f32,
}

impl HeadFadeConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            start_depth_m: 0.05,
            full_depth_m: 0.25,
        }
    }
}

impl Default for HeadFadeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The comfort fade's timing, shared by every source (the head in geometry, the seat transitions).
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
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

impl Default for FadeConfig {
    fn default() -> Self {
        Self::new()
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

/// The hand the roles follow for a configured `dominant` hand and the hand the attached bindings
/// were suggested for, `bound`, if any.
fn acting_hand(dominant: Hand, bound: Option<Hand>) -> Hand {
    bound.unwrap_or(dominant)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_roles_follow_the_bound_hand_until_a_restart() {
        assert_eq!(acting_hand(Hand::Left, None), Hand::Left);
        assert_eq!(acting_hand(Hand::Left, Some(Hand::Right)), Hand::Right);
        assert_eq!(acting_hand(Hand::Right, Some(Hand::Right)), Hand::Right);
    }

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
