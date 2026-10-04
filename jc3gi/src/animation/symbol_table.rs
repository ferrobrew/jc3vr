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
    /// The one-off idle-variation act: transitions the on-foot idle from `S_IDLE` to
    /// `S_IDLE_ONE_OFF` in `rico_base.afsmb`, playing a single idle-break animation (a weight shift,
    /// a look-around, a fidget) before returning to `S_IDLE`. `NStateTask_InputLocoIdleTask::Update`
    /// queues it via [`Character::QueueAct`](crate::character::character::Character::QueueAct) once a
    /// per-character idle countdown (reset to a random ~8.5-12 s each time the character moves)
    /// elapses while the character is standing still, calm, and not attached to a vehicle.
    pub const ACT_TO_IDLE_ONE_OFF: &::std::ffi::CStr = c"ACT_TO_IDLE_ONE_OFF";
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
