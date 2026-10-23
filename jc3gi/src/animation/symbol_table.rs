#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The animation-system event-id (act) symbol table: maps act names (`ACT_*`) to the sequential
/// runtime ids that the animation state machines and [`Character::QueueAct`](crate::character::character::Character::QueueAct)
/// use. Ids are assigned in registration order as names are first encountered (by loaded animation
/// data or the `NCharacter` id globals), so an act's id is only resolvable at runtime.
pub struct EventIdSymbolTable {}
impl EventIdSymbolTable {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418083080usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl EventIdSymbolTable {
    pub const string_to_id_ADDRESS: usize = 0x140413020;
    /// Resolves an act name to its runtime id. Registers the name with a fresh id when it is not
    /// yet present (asserting if the table has been locked), so resolving a name that no loaded
    /// animation data uses yields an id that no queued act will ever carry.
    pub unsafe fn string_to_id(&mut self, name: *const u8) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, name: *const u8) -> i32 = ::std::mem::transmute(
                Self::string_to_id_ADDRESS,
            );
            f(self as *mut Self as _, name)
        }
    }
}
impl EventIdSymbolTable {
    /// The upper-body actions' abort: every `S_UB_*` state of `rico_partial_global` (the draws,
    /// holsters, switches, reloads, pumps, bolts, pickups, throws, and explosives' actions) has an
    /// unconditional transition on it back to the layer's idle, `S_GLOBAL_PARTIAL_NULL`, so a clip
    /// still playing fires none of its remaining track messages. The parachute arm states, the
    /// detonations, and the explosive reloads (`S_JC_UB_*`) do not take it. Read in the shipped
    /// graph data.
    pub const ACT_ABORT_WEAPON_PARTIAL: &::std::ffi::CStr = c"ACT_ABORT_WEAPON_PARTIAL";
    /// The heavy weapon's standing turn a quarter to the left: one of the on-spot acts
    /// [`QueueAimRelativeOnSpotActions`](crate::input::locomotion::NStateTask_LocoUtil::QueueAimRelativeOnSpotActions)
    /// queues for a player wielding a heavy weapon when the aim is more than a sixth of a turn from
    /// the body's forward.
    pub const ACT_AIM_FOCUS_ROTATE_90L: &::std::ffi::CStr = c"ACT_AIM_FOCUS_ROTATE_90L";
    /// The heavy weapon's standing turn a quarter to the right; see
    /// [`ACT_AIM_FOCUS_ROTATE_90L`](crate::animation::symbol_table::EventIdSymbolTable::ACT_AIM_FOCUS_ROTATE_90L).
    pub const ACT_AIM_FOCUS_ROTATE_90R: &::std::ffi::CStr = c"ACT_AIM_FOCUS_ROTATE_90R";
    /// The wingsuit's evade, a barrel roll. The air physics
    /// ([`NAirMovement_UpdateAirPhysics`](crate::input::parachute::NAirMovement_UpdateAirPhysics)) queues
    /// it straight on the character's [`AnimatedModel`](crate::character::character::AnimatedModel), not
    /// through [`Character::QueueAct`](crate::character::character::Character::QueueAct), when the
    /// `WINGSUIT_EVADE` action fires with the move stick out of its rest. It is the only evade act:
    /// the roll's directions (`SEG_IN_BARREL_ROLL_EVADE_MOVEMENT` and its `_LEFT`, `_RIGHT`, `_UP`,
    /// and `_DOWN` variants) and its recovery are segments of the clip it plays, not acts of their
    /// own.
    pub const ACT_BOOST_WINGSUIT_BARREL_ROLL: &::std::ffi::CStr = c"ACT_BOOST_WINGSUIT_BARREL_ROLL";
    /// The forward flick: one of the flick acts that
    /// [`QueueFlickStick`](crate::input::locomotion::NStateTask_LocoUtil::QueueFlickStick) chooses by the
    /// angle from the body's forward to the move direction, for a move direction within a sixth of
    /// a turn of the body's forward. The starts task queues the flicks in place of the stops when a
    /// stop is called for inside the start clip's `FLICK_STICK_TURN_VALID_SEG` segment. Like the
    /// starts, the flick writes the residual angle correction that turns the root toward the move
    /// direction.
    pub const ACT_FLICK_0: &::std::ffi::CStr = c"ACT_FLICK_0";
    /// The forward aiming flick: one of the flick acts that
    /// [`QueueAimFlickStick`](crate::input::locomotion::NStateTask_LocoUtil::QueueAimFlickStick) chooses
    /// by the angle from the body's forward to the move direction, for a move direction within an
    /// eighth of a turn of the body's forward. Unlike the running flicks, the aiming flicks write
    /// no angle correction.
    pub const ACT_FLICK_0_AIM: &::std::ffi::CStr = c"ACT_FLICK_0_AIM";
    /// The flick that turns a third to the left; see [`ACT_FLICK_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_FLICK_0).
    pub const ACT_FLICK_120L: &::std::ffi::CStr = c"ACT_FLICK_120L";
    /// The flick that turns a third to the right; see [`ACT_FLICK_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_FLICK_0).
    pub const ACT_FLICK_120R: &::std::ffi::CStr = c"ACT_FLICK_120R";
    /// The aiming flick that turns around; see [`ACT_FLICK_0_AIM`](crate::animation::symbol_table::EventIdSymbolTable::ACT_FLICK_0_AIM).
    pub const ACT_FLICK_180_AIM: &::std::ffi::CStr = c"ACT_FLICK_180_AIM";
    /// The aiming flick that turns a quarter to the left; see
    /// [`ACT_FLICK_0_AIM`](crate::animation::symbol_table::EventIdSymbolTable::ACT_FLICK_0_AIM).
    pub const ACT_FLICK_90L_AIM: &::std::ffi::CStr = c"ACT_FLICK_90L_AIM";
    /// The aiming flick that turns a quarter to the right; see
    /// [`ACT_FLICK_0_AIM`](crate::animation::symbol_table::EventIdSymbolTable::ACT_FLICK_0_AIM).
    pub const ACT_FLICK_90R_AIM: &::std::ffi::CStr = c"ACT_FLICK_90R_AIM";
    /// The knock-back a player takes from an explosion's impulse:
    /// [`Character::HitReaction`](crate::character::character::Character::HitReaction) queues it when
    /// the state machine accepts it, and the full ragdoll otherwise. In `rico_base.afsmb` it leads
    /// to the `S_JC_HITREACT_FLY_*` states.
    pub const ACT_HITREACT_FLY: &::std::ffi::CStr = c"ACT_HITREACT_FLY";
    /// The left melee stumble, for a character struck from the front. The
    /// player's melee input task
    /// ([`NStateTask_InputMeleeTask::Update`](crate::input::locomotion::NStateTask_InputMeleeTask::Update))
    /// queues one of the four melee stumbles (or its `_DEATH` variant, for a blow that will kill)
    /// on the target character, after testing it with
    /// [`AnimatedModel::TryAct`](crate::character::character::AnimatedModel::TryAct). The `_0` acts are
    /// for a target that faces the attacker (its yaw within a quarter turn of the yaw that points
    /// back along the attack direction), the `_180` acts for one struck from behind, and the side
    /// (`LEFT` or `RIGHT`) follows the angle of the attacker's swing.
    pub const ACT_HITREACT_MELEE_STUMBLE_LEFT_0: &::std::ffi::CStr = c"ACT_HITREACT_MELEE_STUMBLE_LEFT_0";
    /// The left melee stumble, for a character struck from behind; see
    /// [`ACT_HITREACT_MELEE_STUMBLE_LEFT_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_HITREACT_MELEE_STUMBLE_LEFT_0).
    pub const ACT_HITREACT_MELEE_STUMBLE_LEFT_180: &::std::ffi::CStr = c"ACT_HITREACT_MELEE_STUMBLE_LEFT_180";
    /// The right melee stumble, for a character struck from the front; see
    /// [`ACT_HITREACT_MELEE_STUMBLE_LEFT_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_HITREACT_MELEE_STUMBLE_LEFT_0).
    pub const ACT_HITREACT_MELEE_STUMBLE_RIGHT_0: &::std::ffi::CStr = c"ACT_HITREACT_MELEE_STUMBLE_RIGHT_0";
    /// The right melee stumble, for a character struck from behind; see
    /// [`ACT_HITREACT_MELEE_STUMBLE_LEFT_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_HITREACT_MELEE_STUMBLE_LEFT_0).
    pub const ACT_HITREACT_MELEE_STUMBLE_RIGHT_180: &::std::ffi::CStr = c"ACT_HITREACT_MELEE_STUMBLE_RIGHT_180";
    /// The stumble a hit's impulse knocks a player into, past the character's stumble threshold
    /// and short of its ragdoll threshold. [`Character::HitReaction`](crate::character::character::Character::HitReaction)
    /// queues it, or [`ACT_HITREACT_STUMBLE_BULLET`](crate::animation::symbol_table::EventIdSymbolTable::ACT_HITREACT_STUMBLE_BULLET) for a bullet
    /// or [`ACT_HITREACT_STUMBLE_MELEE`](crate::animation::symbol_table::EventIdSymbolTable::ACT_HITREACT_STUMBLE_MELEE) for a melee blow, on the
    /// character's [`AnimatedModel`](crate::character::character::AnimatedModel) when the state machine
    /// accepts it, and the flinch otherwise. In `rico_base.afsmb` the stumbles lead to the
    /// `S_JC_HITREACT_STUMBLE_*` states.
    pub const ACT_HITREACT_STUMBLE: &::std::ffi::CStr = c"ACT_HITREACT_STUMBLE";
    /// The bullet variant of [`ACT_HITREACT_STUMBLE`](crate::animation::symbol_table::EventIdSymbolTable::ACT_HITREACT_STUMBLE).
    pub const ACT_HITREACT_STUMBLE_BULLET: &::std::ffi::CStr = c"ACT_HITREACT_STUMBLE_BULLET";
    /// The melee variant of [`ACT_HITREACT_STUMBLE`](crate::animation::symbol_table::EventIdSymbolTable::ACT_HITREACT_STUMBLE).
    pub const ACT_HITREACT_STUMBLE_MELEE: &::std::ffi::CStr = c"ACT_HITREACT_STUMBLE_MELEE";
    /// The forward run start, out of the idle: one of the directional run-start (wind-up) acts that
    /// [`QueueStarts`](crate::input::locomotion::NStateTask_LocoUtil::QueueStarts) chooses by the angle from
    /// the body's forward to the move direction. Its clip starts straight ahead.
    pub const ACT_IDLE_TO_START_0: &::std::ffi::CStr = c"ACT_IDLE_TO_START_0";
    /// The run start that turns a third to the left; see
    /// [`ACT_IDLE_TO_START_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_IDLE_TO_START_0).
    pub const ACT_IDLE_TO_START_120L: &::std::ffi::CStr = c"ACT_IDLE_TO_START_120L";
    /// The run start that turns a third to the right; see
    /// [`ACT_IDLE_TO_START_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_IDLE_TO_START_0).
    pub const ACT_IDLE_TO_START_120R: &::std::ffi::CStr = c"ACT_IDLE_TO_START_120R";
    /// The run start that turns around to the left; see
    /// [`ACT_IDLE_TO_START_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_IDLE_TO_START_0).
    pub const ACT_IDLE_TO_START_180L: &::std::ffi::CStr = c"ACT_IDLE_TO_START_180L";
    /// The run start that turns around to the right; see
    /// [`ACT_IDLE_TO_START_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_IDLE_TO_START_0).
    pub const ACT_IDLE_TO_START_180R: &::std::ffi::CStr = c"ACT_IDLE_TO_START_180R";
    /// The run start that turns a quarter to the left; see
    /// [`ACT_IDLE_TO_START_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_IDLE_TO_START_0).
    pub const ACT_IDLE_TO_START_90L: &::std::ffi::CStr = c"ACT_IDLE_TO_START_90L";
    /// The run start that turns a quarter to the right on the way out of the idle; see
    /// [`ACT_IDLE_TO_START_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_IDLE_TO_START_0).
    pub const ACT_IDLE_TO_START_90R: &::std::ffi::CStr = c"ACT_IDLE_TO_START_90R";
    /// The forward walk start: the walk flavor of
    /// [`ACT_IDLE_TO_START_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_IDLE_TO_START_0), which
    /// [`QueueStarts`](crate::input::locomotion::NStateTask_LocoUtil::QueueStarts) picks when the speed is
    /// below the movement settings' threshold.
    pub const ACT_IDLE_TO_WALK_START_0: &::std::ffi::CStr = c"ACT_IDLE_TO_WALK_START_0";
    /// The walk start that turns a third to the left; see
    /// [`ACT_IDLE_TO_WALK_START_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_IDLE_TO_WALK_START_0).
    pub const ACT_IDLE_TO_WALK_START_120L: &::std::ffi::CStr = c"ACT_IDLE_TO_WALK_START_120L";
    /// The walk start that turns a third to the right; see
    /// [`ACT_IDLE_TO_WALK_START_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_IDLE_TO_WALK_START_0).
    pub const ACT_IDLE_TO_WALK_START_120R: &::std::ffi::CStr = c"ACT_IDLE_TO_WALK_START_120R";
    /// The walk start that turns around to the left; see
    /// [`ACT_IDLE_TO_WALK_START_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_IDLE_TO_WALK_START_0).
    pub const ACT_IDLE_TO_WALK_START_180L: &::std::ffi::CStr = c"ACT_IDLE_TO_WALK_START_180L";
    /// The walk start that turns around to the right; see
    /// [`ACT_IDLE_TO_WALK_START_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_IDLE_TO_WALK_START_0).
    pub const ACT_IDLE_TO_WALK_START_180R: &::std::ffi::CStr = c"ACT_IDLE_TO_WALK_START_180R";
    /// The walk start that turns a quarter to the left; see
    /// [`ACT_IDLE_TO_WALK_START_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_IDLE_TO_WALK_START_0).
    pub const ACT_IDLE_TO_WALK_START_90L: &::std::ffi::CStr = c"ACT_IDLE_TO_WALK_START_90L";
    /// The walk start that turns a quarter to the right; see
    /// [`ACT_IDLE_TO_WALK_START_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_IDLE_TO_WALK_START_0).
    pub const ACT_IDLE_TO_WALK_START_90R: &::std::ffi::CStr = c"ACT_IDLE_TO_WALK_START_90R";
    /// The automatic holster queued by `CPlayer::UpdateMentalState`
    /// ([`Player_UpdateMentalState`](crate::character::player::Player_UpdateMentalState)) when the
    /// player's mental state decays to relaxed with a weapon wielded. It drives a state chain in
    /// `rico_base.afsmb` whose `NWeaponUnequipTask` queues [`UB_ACT_HOLSTER`](crate::animation::symbol_table::EventIdSymbolTable::UB_ACT_HOLSTER).
    pub const ACT_ON_UNEQUIP_WEAPON_SLOW: &::std::ffi::CStr = c"ACT_ON_UNEQUIP_WEAPON_SLOW";
    /// The on-foot / land-vehicle reversing act: drives the `S_REVERSE_*` states in
    /// `rico_base.afsmb`, where Rico turns to look over his shoulder while backing up.
    pub const ACT_REVERSE: &::std::ffi::CStr = c"ACT_REVERSE";
    /// The motorbike variant of [`ACT_REVERSE`](crate::animation::symbol_table::EventIdSymbolTable::ACT_REVERSE).
    pub const ACT_REVERSE_MOTORBIKE: &::std::ffi::CStr = c"ACT_REVERSE_MOTORBIKE";
    /// See [`ACT_ROTATE_45L`](crate::animation::symbol_table::EventIdSymbolTable::ACT_ROTATE_45L).
    pub const ACT_ROTATE_180L: &::std::ffi::CStr = c"ACT_ROTATE_180L";
    /// See [`ACT_ROTATE_90L_WARP`](crate::animation::symbol_table::EventIdSymbolTable::ACT_ROTATE_90L_WARP).
    pub const ACT_ROTATE_180L_WARP: &::std::ffi::CStr = c"ACT_ROTATE_180L_WARP";
    /// See [`ACT_ROTATE_45L`](crate::animation::symbol_table::EventIdSymbolTable::ACT_ROTATE_45L).
    pub const ACT_ROTATE_180R: &::std::ffi::CStr = c"ACT_ROTATE_180R";
    /// See [`ACT_ROTATE_90L_WARP`](crate::animation::symbol_table::EventIdSymbolTable::ACT_ROTATE_90L_WARP).
    pub const ACT_ROTATE_180R_WARP: &::std::ffi::CStr = c"ACT_ROTATE_180R_WARP";
    /// The rotate-on-spot acts: a standing turn in place by an eighth, a quarter, or half a turn,
    /// left or right, queued by the on-foot input tasks' on-spot turn queuers when the body's
    /// facing and the wanted facing differ while the character stands still.
    pub const ACT_ROTATE_45L: &::std::ffi::CStr = c"ACT_ROTATE_45L";
    /// See [`ACT_ROTATE_45L`](crate::animation::symbol_table::EventIdSymbolTable::ACT_ROTATE_45L).
    pub const ACT_ROTATE_45R: &::std::ffi::CStr = c"ACT_ROTATE_45R";
    /// See [`ACT_ROTATE_45L`](crate::animation::symbol_table::EventIdSymbolTable::ACT_ROTATE_45L).
    pub const ACT_ROTATE_90L: &::std::ffi::CStr = c"ACT_ROTATE_90L";
    /// The warp flavors of the rotate-on-spot acts; see [`ACT_ROTATE_45L`](crate::animation::symbol_table::EventIdSymbolTable::ACT_ROTATE_45L).
    pub const ACT_ROTATE_90L_WARP: &::std::ffi::CStr = c"ACT_ROTATE_90L_WARP";
    /// See [`ACT_ROTATE_45L`](crate::animation::symbol_table::EventIdSymbolTable::ACT_ROTATE_45L).
    pub const ACT_ROTATE_90R: &::std::ffi::CStr = c"ACT_ROTATE_90R";
    /// See [`ACT_ROTATE_90L_WARP`](crate::animation::symbol_table::EventIdSymbolTable::ACT_ROTATE_90L_WARP).
    pub const ACT_ROTATE_90R_WARP: &::std::ffi::CStr = c"ACT_ROTATE_90R_WARP";
    /// Ends a standing rotation that [`ACT_ROTATE_LEFT_START`](crate::animation::symbol_table::EventIdSymbolTable::ACT_ROTATE_LEFT_START) or
    /// [`ACT_ROTATE_RIGHT_START`](crate::animation::symbol_table::EventIdSymbolTable::ACT_ROTATE_RIGHT_START) started.
    pub const ACT_ROTATE_END: &::std::ffi::CStr = c"ACT_ROTATE_END";
    /// Starts the standing rotation to the left; see [`ACT_ROTATE_END`](crate::animation::symbol_table::EventIdSymbolTable::ACT_ROTATE_END).
    pub const ACT_ROTATE_LEFT_START: &::std::ffi::CStr = c"ACT_ROTATE_LEFT_START";
    /// Starts the standing rotation to the right; see [`ACT_ROTATE_END`](crate::animation::symbol_table::EventIdSymbolTable::ACT_ROTATE_END).
    pub const ACT_ROTATE_RIGHT_START: &::std::ffi::CStr = c"ACT_ROTATE_RIGHT_START";
    /// The gun-side pump act: the pump shotgun's (`w151`) own state machine plays the pump on
    /// the gun's model for it. The shotgun's gun-facing do-act component
    /// ([`DoActWeaponComponent`](crate::equipment::weapon::DoActWeaponComponent)) sends it to the gun
    /// ([`WeaponData::DoAct`](crate::equipment::weapon::WeaponData::DoAct)) at each shot, alongside the
    /// holder's `UB_ACT_SHOTGUN_PUMP`.
    pub const ACT_SHOTGUN_PUMP: &::std::ffi::CStr = c"ACT_SHOTGUN_PUMP";
    /// The gun-side bolt act, handled by the state machines of the bolt-action rifles that cycle a
    /// bolt on the gun's own model (`w162` and `w163`). The holder's counterpart is
    /// `UB_ACT_SNIPER_BOLT_ACTION`, which the rifle's per-tick update queues
    /// ([`SniperWeapon::UpdateFromUserPostSim_Parallel`](crate::equipment::weapon::SniperWeapon::UpdateFromUserPostSim_Parallel)).
    pub const ACT_SNIPER_BOLT_ACTION: &::std::ffi::CStr = c"ACT_SNIPER_BOLT_ACTION";
    /// The straight stop: one of the stop-and-turn acts that
    /// [`QueueStopTurns`](crate::input::locomotion::NStateTask_LocoUtil::QueueStopTurns) chooses by the angle
    /// from the body's forward to the aim, here none.
    pub const ACT_STOP_TURN_0: &::std::ffi::CStr = c"ACT_STOP_TURN_0";
    /// The warp flavor of the straight stop; the `_WARP` stop turns below are its turning
    /// variants.
    pub const ACT_STOP_TURN_0_WARP: &::std::ffi::CStr = c"ACT_STOP_TURN_0_WARP";
    /// The stop that turns a third to the left; see [`ACT_STOP_TURN_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_STOP_TURN_0).
    pub const ACT_STOP_TURN_120L: &::std::ffi::CStr = c"ACT_STOP_TURN_120L";
    /// The stop that turns a third to the right; see [`ACT_STOP_TURN_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_STOP_TURN_0).
    pub const ACT_STOP_TURN_120R: &::std::ffi::CStr = c"ACT_STOP_TURN_120R";
    /// The stop that turns around to the left; see [`ACT_STOP_TURN_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_STOP_TURN_0).
    pub const ACT_STOP_TURN_180L: &::std::ffi::CStr = c"ACT_STOP_TURN_180L";
    /// See [`ACT_STOP_TURN_0_WARP`](crate::animation::symbol_table::EventIdSymbolTable::ACT_STOP_TURN_0_WARP).
    pub const ACT_STOP_TURN_180L_WARP: &::std::ffi::CStr = c"ACT_STOP_TURN_180L_WARP";
    /// The stop that turns around to the right; see [`ACT_STOP_TURN_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_STOP_TURN_0).
    pub const ACT_STOP_TURN_180R: &::std::ffi::CStr = c"ACT_STOP_TURN_180R";
    /// See [`ACT_STOP_TURN_0_WARP`](crate::animation::symbol_table::EventIdSymbolTable::ACT_STOP_TURN_0_WARP).
    pub const ACT_STOP_TURN_180R_WARP: &::std::ffi::CStr = c"ACT_STOP_TURN_180R_WARP";
    /// The stop that turns a quarter to the left; see [`ACT_STOP_TURN_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_STOP_TURN_0).
    pub const ACT_STOP_TURN_90L: &::std::ffi::CStr = c"ACT_STOP_TURN_90L";
    /// See [`ACT_STOP_TURN_0_WARP`](crate::animation::symbol_table::EventIdSymbolTable::ACT_STOP_TURN_0_WARP).
    pub const ACT_STOP_TURN_90L_WARP: &::std::ffi::CStr = c"ACT_STOP_TURN_90L_WARP";
    /// The stop that turns a quarter to the right; see
    /// [`ACT_STOP_TURN_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_STOP_TURN_0).
    pub const ACT_STOP_TURN_90R: &::std::ffi::CStr = c"ACT_STOP_TURN_90R";
    /// See [`ACT_STOP_TURN_0_WARP`](crate::animation::symbol_table::EventIdSymbolTable::ACT_STOP_TURN_0_WARP).
    pub const ACT_STOP_TURN_90R_WARP: &::std::ffi::CStr = c"ACT_STOP_TURN_90R_WARP";
    /// The stunt turn toward a move direction a little off the body's forward: one of the stunt
    /// turns that [`QueueStuntTurns`](crate::input::locomotion::NStateTask_LocoUtil::QueueStuntTurns)
    /// chooses by the angle from the body's forward to the move direction, for a character standing
    /// in a vehicle's stunt area. Each writes the residual angle correction that turns the root.
    pub const ACT_STUNT_TURN_0: &::std::ffi::CStr = c"ACT_STUNT_TURN_0";
    /// The stunt turn a third to the left; see [`ACT_STUNT_TURN_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_STUNT_TURN_0).
    pub const ACT_STUNT_TURN_120L: &::std::ffi::CStr = c"ACT_STUNT_TURN_120L";
    /// The stunt turn a third to the right; see [`ACT_STUNT_TURN_0`](crate::animation::symbol_table::EventIdSymbolTable::ACT_STUNT_TURN_0).
    pub const ACT_STUNT_TURN_120R: &::std::ffi::CStr = c"ACT_STUNT_TURN_120R";
    /// The return to the idle. The locomotion helpers queue it where no turn is needed:
    /// [`QueueStuntTurns`](crate::input::locomotion::NStateTask_LocoUtil::QueueStuntTurns) when the move
    /// direction is close to the body's forward, and
    /// [`QueueAimRelativeOnSpotActions`](crate::input::locomotion::NStateTask_LocoUtil::QueueAimRelativeOnSpotActions)
    /// when the aim is. The starts, move, and path-follow input tasks also queue it.
    pub const ACT_TO_IDLE: &::std::ffi::CStr = c"ACT_TO_IDLE";
    /// The one-off idle-variation act: transitions the on-foot idle from `S_IDLE` to
    /// `S_IDLE_ONE_OFF` in `rico_base.afsmb`, playing a single idle-break animation (a weight shift,
    /// a look-around, a fidget) before returning to `S_IDLE`. `NStateTask_InputLocoIdleTask::Update`
    /// queues it via [`Character::QueueAct`](crate::character::character::Character::QueueAct) once a
    /// per-character idle countdown (reset to a random ~8.5-12 s each time the character moves)
    /// elapses while the character is standing still, calm, and not attached to a vehicle.
    pub const ACT_TO_IDLE_ONE_OFF: &::std::ffi::CStr = c"ACT_TO_IDLE_ONE_OFF";
    /// See [`ACT_TURN_90L`](crate::animation::symbol_table::EventIdSymbolTable::ACT_TURN_90L).
    pub const ACT_TURN_180L: &::std::ffi::CStr = c"ACT_TURN_180L";
    /// See [`ACT_TURN_90L`](crate::animation::symbol_table::EventIdSymbolTable::ACT_TURN_90L).
    pub const ACT_TURN_180R: &::std::ffi::CStr = c"ACT_TURN_180R";
    /// The quarter and half turns to either side. The locomotion utilities and the grapple-fire move
    /// task also queue the half turns.
    pub const ACT_TURN_90L: &::std::ffi::CStr = c"ACT_TURN_90L";
    /// See [`ACT_TURN_90L`](crate::animation::symbol_table::EventIdSymbolTable::ACT_TURN_90L).
    pub const ACT_TURN_90R: &::std::ffi::CStr = c"ACT_TURN_90R";
    /// The upper-body holster act. Queued by
    /// [`Inventory::UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling)'s
    /// relaxed branch for the player, by
    /// [`Character::HolsterWeapon`](crate::character::character::Character::HolsterWeapon) for other
    /// characters, and by `NWeaponUnequipTask` on entering its state.
    pub const UB_ACT_HOLSTER: &::std::ffi::CStr = c"UB_ACT_HOLSTER";
}
impl std::convert::AsRef<EventIdSymbolTable> for EventIdSymbolTable {
    fn as_ref(&self) -> &EventIdSymbolTable {
        self
    }
}
impl std::convert::AsMut<EventIdSymbolTable> for EventIdSymbolTable {
    fn as_mut(&mut self) -> &mut EventIdSymbolTable {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// A ragdoll rigid-body group id (`ava::idstring<NAnimationSystem::SRagdollRBGroupIdTag>`), which
/// names the part of a character's body a hit struck. Every live value holds a use of the group
/// symbol table: [`init_ptr`](crate::animation::symbol_table::RagdollRBGroupId::init_ptr) takes one, creating the table on first use, and the
/// value's destructor gives it back, destroying the table when the last use goes. A function that
/// takes an id by value destroys it, so its caller copies the id and takes a use for the copy
/// first.
pub struct RagdollRBGroupId {
    /// The id.
    pub m_id: u32,
}
fn _RagdollRBGroupId_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], RagdollRBGroupId>([0u8; 0x4]);
    }
    unreachable!()
}
impl RagdollRBGroupId {
    pub const init_ptr_ADDRESS: usize = 0x140416A70;
    /// Takes a use of the group symbol table for this value.
    pub unsafe fn init_ptr(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::init_ptr_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl RagdollRBGroupId {
    /// The id a character's damage handler gives a hit it finds no group for, as it gives every
    /// melee hit.
    pub const NONE: u32 = 0;
}
impl std::convert::AsRef<RagdollRBGroupId> for RagdollRBGroupId {
    fn as_ref(&self) -> &RagdollRBGroupId {
        self
    }
}
impl std::convert::AsMut<RagdollRBGroupId> for RagdollRBGroupId {
    fn as_mut(&mut self) -> &mut RagdollRBGroupId {
        self
    }
}
