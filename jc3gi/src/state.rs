#![cfg_attr(any(), rustfmt::skip)]
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// `SUserVM::EContextBitFlag`: the index of a bit in the character's animation context-bit word,
/// which game code sets to steer the animation graph. The values are unconfirmed.
pub enum ContextBitFlag {
    CB_IS_PREVENTING_FALL = 0isize as _,
    CB_IS_MOTORBIKEISH = 1isize as _,
    CB_UNSTUNTIFY = 2isize as _,
    CB_FORCE_WALK = 3isize as _,
    CB_FORCE_STOP = 4isize as _,
    CB_IN_VISTA_NEAR_CAMERA_TRIGGER = 5isize as _,
    CB_IN_VISTA_FAR_CAMERA_TRIGGER = 6isize as _,
    CB_LOD_INVISIBLE = 7isize as _,
    CB_LOD_SEMI_VISIBLE = 8isize as _,
    CB_IS_FAKE_STUNTING = 9isize as _,
    CB_IS_BOOSTING_REEL = 10isize as _,
    CB_IS_ENCLOSED = 11isize as _,
    CB_IS_CONCRETE_CHARACTER = 12isize as _,
    CB_WINGSUIT_CHASE_ACTIVE = 13isize as _,
}
fn _ContextBitFlag_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ContextBitFlag>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// `SUserVM::EStateBitFlag`: the index of a bit in the character's animation state-bit words, which
/// state tasks in the animation graph set and clear and which game code and graph conditions read.
/// Bit `n` lives in 64-bit word `n / 64` at bit `n % 64` (word 0 holds bits 0-63, word 1 bits
/// 64-127, and so on); the words are stored in
/// [`UserVM`](crate::character::control_parameters::UserVM). The values are confirmed at every bit an
/// accessor tests: the one-bit predicates on
/// [`Character`](crate::character::character::Character) (wingsuit, parachute, free fall, fast fall,
/// reeling in, motorbike, swimming, attached parachute, pre-reel, pre-hang), the grapple bits of
/// `IsReeledIn` and `IsGrappleHanging`, the aim, drawing, holstering, and pause-aim-timer bits
/// `HandleAimingInputPlayer` tests, and `SB_DISABLE_SHOOT`. The other values are unconfirmed.
pub enum StateBitFlag {
    SB_IS_WINGSUIT = 0isize as _,
    SB_IS_NO_FIRING = 1isize as _,
    SB_IS_GRAPPLE_TETHER_FIRING = 2isize as _,
    SB_IS_GRAPPLE_FIRING = 3isize as _,
    SB_IS_FALLING = 4isize as _,
    SB_IS_FREE_FALLING = 5isize as _,
    SB_DISABLE_GLOBAL_PARTIALS = 6isize as _,
    SB_DISABLE_GLOBAL_ADDITIVES = 7isize as _,
    SB_IN_IDLE = 8isize as _,
    SB_IN_SLOW_START = 9isize as _,
    SB_IN_FAST_START = 10isize as _,
    SB_IN_SLOW_STOP = 11isize as _,
    SB_IN_FAST_STOP = 12isize as _,
    SB_IN_SLOW_STOP_TURN = 13isize as _,
    SB_IN_FAST_STOP_TURN = 14isize as _,
    SB_IN_SLOW_PLANT = 15isize as _,
    SB_IN_FAST_PLANT = 16isize as _,
    SB_IGNORE_MOTION_TRANSITIONS = 17isize as _,
    SB_IN_MOTION = 18isize as _,
    SB_IN_ANGLE_0 = 19isize as _,
    SB_IN_ANGLE_90R = 20isize as _,
    SB_IN_ANGLE_90L = 21isize as _,
    SB_IN_ANGLE_120R = 22isize as _,
    SB_IN_ANGLE_120L = 23isize as _,
    SB_IN_ANGLE_180R = 24isize as _,
    SB_IN_ANGLE_180L = 25isize as _,
    SB_IN_IDLE_AIMING = 26isize as _,
    SB_IN_REACTION = 27isize as _,
    SB_IN_ROTATE = 28isize as _,
    SB_IN_BACKPEDAL = 29isize as _,
    SB_USE_ENABLE_AIM_IK_SEG = 30isize as _,
    SB_IN_ANCHOR_WARPING = 31isize as _,
    SB_IN_GRAPPLE_ATTACHED = 32isize as _,
    SB_IS_FASTFALLING = 33isize as _,
    SB_IS_LANDING = 34isize as _,
    SB_IS_PARACHUTING = 35isize as _,
    SB_IS_GRAPPLE_HANG = 36isize as _,
    SB_IS_PLANTING_EXPLOSIVES = 37isize as _,
    SB_IS_THROWING_GRENADE = 38isize as _,
    SB_IN_HIT_REACTION = 39isize as _,
    SB_IS_DRAWING_WEAPON = 40isize as _,
    SB_NO_WEAPON_AIM_NOAIM_TRANSITIONS = 41isize as _,
    SB_IS_GRAPPLE_UPSIDEDOWN = 42isize as _,
    SB_IN_RAGDOLL_GRAPPLE_FIRE = 43isize as _,
    SB_NO_WEAPON_AIM = 44isize as _,
    SB_IS_LOCO_ONE_OFF = 45isize as _,
    SB_IS_LOOK_AT = 46isize as _,
    SB_IS_JUMPING = 47isize as _,
    SB_IS_LANDING_HARD = 48isize as _,
    SB_IN_DO_ACT_WAIT = 49isize as _,
    SB_IS_RELOADING = 50isize as _,
    SB_IS_AIMING_WPN_OR_GRPL = 51isize as _,
    SB_IS_SWITCHING_WEAPONS = 52isize as _,
    SB_NO_VERTICAL_ADDITIVES = 53isize as _,
    SB_IS_HOLSTERING_WEAPON = 54isize as _,
    SB_IS_DETONATING_EXPLOSIVES = 55isize as _,
    SB_DONT_APPLY_DAMAGE_IMPACT = 58isize as _,
    SB_DONT_APPLY_DAMAGE = 59isize as _,
    SB_FLINCH_ON_ZERO_DAMAGE = 60isize as _,
    SB_SKIP_HITREACTION_RAGDOLL = 61isize as _,
    SB_SKIP_HITREACTION_STUMBLE = 62isize as _,
    SB_SKIP_HITREACTION_FLINCH = 63isize as _,
    SB_DISABLE_SHOOT = 64isize as _,
    SB_DISABLE_RELOAD = 65isize as _,
    SB_DISABLE_USE_GRENADES = 66isize as _,
    SB_DISABLE_TOGGLE_PRECISION = 67isize as _,
    SB_DISABLE_PICKUP = 68isize as _,
    SB_DISABLE_SWITCH_WEAPONS = 69isize as _,
    SB_DISABLE_PLANT_EXPLOSIVES = 70isize as _,
    SB_DISABLE_TRIGGER_EXPLOSIVES = 71isize as _,
    SB_DISABLE_PICKUP_MOUNTED_GUN = 72isize as _,
    SB_DISABLE_DRAW_WEAPON = 73isize as _,
    SB_DISABLE_HOLSTER_WEAPON = 74isize as _,
    SB_DISABLE_GRAPPLE_FIRE = 75isize as _,
    SB_DISABLE_MELEE = 76isize as _,
    SB_IN_RUSH_SEARCH = 77isize as _,
    SB_IN_HIJACKLING_EXPLOSIVE_STATE = 78isize as _,
    SB_SHOULD_IGNORE_FIRING_ANGLE = 79isize as _,
    SB_IN_GRAPPLE_STRUGGLE = 80isize as _,
    SB_IN_USE_ITEM = 81isize as _,
    SB_OVERRIDE_GLOBAL_DEATH = 82isize as _,
    SB_ALLOW_STATIONARY_HITREACTION = 83isize as _,
    SB_PAUSE_AIM_TIMER = 84isize as _,
    SB_DISABLE_MENTAL_STATE_DRAW_WEAPON = 85isize as _,
    SB_DISABLE_WEAPON_OFFHAND_IK = 86isize as _,
    SB_WEAPON_OFFHAND_IK_LEFT = 87isize as _,
    SB_WEAPON_OFFHAND_IK_RIGHT = 88isize as _,
    SB_IN_TRANSITION_TO_WINGSUIT = 89isize as _,
    SB_IN_ATTACHED_TO_PARACHUTE = 90isize as _,
    SB_IN_PRE_REEL = 91isize as _,
    SB_CAN_USE_ITEM = 92isize as _,
    SB_DISABLE_USE_ITEM = 93isize as _,
    SB_IN_TRANSITION_TO_PARACHUTE_FROM_FALL = 94isize as _,
    SB_IN_LOCOMOTION = 95isize as _,
    SB_APPLY_HIP_STUNT_ADJUSTMENT = 96isize as _,
    SB_IN_LOOP_CONTEXT_ACTION = 97isize as _,
    SB_IN_LOOP_CHILD_CONTEXT_ACTION = 98isize as _,
    SB_FORCE_SEE_COMBAT_TARGET = 99isize as _,
    SB_IS_IN_DEAD_STATE = 100isize as _,
    SB_IS_RIDING_MOTORBIKE = 101isize as _,
    SB_IN_COVER_ENTRY = 102isize as _,
    SB_IN_COVER = 103isize as _,
    SB_IN_COVER_EXIT = 104isize as _,
    SB_WAIT_TO_MOVE = 105isize as _,
    SB_IN_CROUCH = 106isize as _,
    SB_IN_STOPPING = 107isize as _,
    SB_IN_REELING_IN = 108isize as _,
    SB_DISABLE_SWITCH_WEAPON_GUI = 109isize as _,
    SB_USE_ENABLE_LOOKAT_IK_SEG = 110isize as _,
    SB_USE_DISABLE_LOOKAT_IK_SEG = 111isize as _,
    SB_IN_MORTAR_MODE = 112isize as _,
    SB_IN_MORTAR_FIRE = 113isize as _,
    SB_IN_MORTAR_ENTER = 114isize as _,
    SB_DISABLE_MORTAR_MODE = 115isize as _,
    SB_INSIDE_VEHICLE = 116isize as _,
    SB_OUTSIDE_VEHICLE = 117isize as _,
    SB_IN_GRAPPLE_YANKING = 118isize as _,
    SB_IN_RAGDOLL = 119isize as _,
    SB_IS_SWIMMING = 120isize as _,
    SB_IS_UW_SWIMMING = 121isize as _,
    SB_IS_SWIM_TURNING = 122isize as _,
    SB_IN_MELEE = 123isize as _,
    SB_HELI_PASSENGER_COMBAT = 124isize as _,
    SB_IN_ROTATE_LEFT = 125isize as _,
    SB_IN_ROTATE_RIGHT = 126isize as _,
    SB_RANDOM_ENCOUNTER_IDLE = 127isize as _,
    SB_RANDOM_ENCOUNTER_ACKNOWLEDGE = 128isize as _,
    SB_RANDOM_ENCOUNTER_DIALOGUE = 129isize as _,
    SB_IS_GETTING_UP_FROM_RAGDOLL = 130isize as _,
    SB_DISABLE_SWITCH_TO_PLANTED_EXPLOSIVE = 131isize as _,
    SB_DISABLE_AIM_ASSIST = 132isize as _,
    SB_CALLING_FOR_REINFORCEMENTS = 133isize as _,
    SB_IN_FULLBODY_PICKUP = 134isize as _,
    SB_IN_PRE_HANG = 135isize as _,
    SB_DISABLE_FACIAL_EXPRESSION = 136isize as _,
    SB_DISABLE_GLOBAL_HITREACTIONS = 140isize as _,
    SB_GRAPPLE_IMMUNITY = 141isize as _,
    SB_IN_LOCOMOTION_STRAFING = 142isize as _,
    SB_IN_ANGLE_45R = 143isize as _,
    SB_IN_ANGLE_45L = 144isize as _,
    SB_IN_NOAIM_TO_AIM = 145isize as _,
    SB_UPDATE_STUCK_DETECTOR = 146isize as _,
    SB_INVERT_VERTICAL_BLENDING = 147isize as _,
    SB_IN_OBSERVING = 148isize as _,
    SB_IN_ANGLE_135L = 149isize as _,
    SB_IN_ANGLE_135R = 150isize as _,
    SB_IS_CONCRETE_CHARACTER = 151isize as _,
    SB_IS_BAVWING_AIRBRAKE = 152isize as _,
    SB_IS_BAVWING_GROUND_TAKEOFF = 153isize as _,
    SB_IS_BAVWING_EVADE = 154isize as _,
    SB_IS_BAVWING_EVADE_SPECIAL_RETICLE = 155isize as _,
    SB_IS_BAVWING_WATER_TAKEOFF = 156isize as _,
    SB_IS_BAVWING_RECOVER_FROM_FALL = 157isize as _,
    SB_VEHICLE_DISABLE_ARM_IK = 158isize as _,
    SB_IS_MECH_PUNCHING = 159isize as _,
    SB_IS_MECH_JUMPING = 160isize as _,
}
fn _StateBitFlag_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], StateBitFlag>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The opaque per-task state-machine context passed to character and task update functions. Used only
/// behind pointers; the layout is not yet mapped.
pub struct StateContext {}
impl StateContext {}
impl std::convert::AsRef<StateContext> for StateContext {
    fn as_ref(&self) -> &StateContext {
        self
    }
}
impl std::convert::AsMut<StateContext> for StateContext {
    fn as_mut(&mut self) -> &mut StateContext {
        self
    }
}
