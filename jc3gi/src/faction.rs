#![cfg_attr(any(), rustfmt::skip)]
pub const GetRelation_ADDRESS: usize = 0x1404CF0B0;
/// How a character of faction `from` stands toward one of faction `to`, from the game's faction
/// table: [`E_RELATION_ALLIED`](crate::aim::target::FactionRelation::E_RELATION_ALLIED),
/// [`E_RELATION_HOSTILE`](crate::aim::target::FactionRelation::E_RELATION_HOSTILE), or
/// [`E_RELATION_NONE`](crate::aim::target::FactionRelation::E_RELATION_NONE) when the table is not loaded or either
/// faction is not in it. Factions are those
/// [`Character::GetFaction`](crate::character::character::Character::GetFaction) returns. The player aim's
/// target relation ([`Target::m_PlayerFactionRelation`](crate::aim::target::Target::m_PlayerFactionRelation))
/// is this from the player's faction to the target's.
pub unsafe fn GetRelation(from: i32, to: i32) -> crate::aim::target::FactionRelation {
    unsafe {
        let f: unsafe extern "system" fn(
            from: i32,
            to: i32,
        ) -> crate::aim::target::FactionRelation = ::std::mem::transmute(
            GetRelation_ADDRESS,
        );
        f(from, to)
    }
}
