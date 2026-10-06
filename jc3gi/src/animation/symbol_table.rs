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
