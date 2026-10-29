//! The game's shooting permission as the held-gun fire paths apply it.
//!
//! The game withholds its shooting permission (`Inventory::GetAnimationAllowsShooting`) through
//! its animation graph's permission tasks and a few code rules (see
//! `docs/engine/gameplay/weapon-firing.md`, "The shooting permission"). Several of those tasks sit on
//! airborne locomotion states, most of which also put the game's own weapon away: free fall, fast fall, the
//! wingsuit's rolls, evades, flinches, and exits, and the parachute's opening. A gun the player
//! holds in a hand is not posed by those animations, and the mod updates it directly, which the
//! permission does not reach, so the held-gun paths waive the permission in those states. The
//! waiver is scoped to them: it never covers a grapple wind-up, reel-in, or live wire (the code rules
//! that withhold every permission, or shooting without the reel upgrade), and it never covers the
//! ground, swimming, vehicle, ragdoll, or death states, whose withholding still applies. A trigger
//! the permission withholds stays reserved by the gun, so a press neither fires nor falls through to
//! the game's own action for the trigger.

use jc3gi::character::character::Character;

/// Whether a gun the character holds may fire: the game's shooting permission, or the airborne
/// locomotion waiver ([`waived`]).
///
/// # Safety
///
/// `character` must be live for the call; the reads are the same ones the game's own predicates
/// make, and take no locks.
pub(crate) unsafe fn shooting_allowed(character: &Character) -> bool {
    let allowed = unsafe { character.m_Inventory.GetAnimationAllowsShooting() };
    allowed || waived(unsafe { airborne_state(character) })
}

/// The character's airborne locomotion and grapple activity, as the waiver reads them.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
struct AirborneState {
    /// Flying the wingsuit, under a deployed or vehicle-attached parachute, or in free or fast fall.
    airborne: bool,
    /// In the grapple's reel-in wind-up or a reel-in traversal.
    reeling: bool,
    /// The grappling hook has a live wire.
    active_wire: bool,
}

/// Whether the shooting permission is waived: in airborne locomotion, with no grapple wind-up,
/// reel-in, or live wire.
fn waived(state: AirborneState) -> bool {
    state.airborne && !state.reeling && !state.active_wire
}

unsafe fn airborne_state(character: &Character) -> AirborneState {
    let hook = unsafe { character.m_Inventory.m_GrapplingHook.px.as_ref() };
    unsafe {
        AirborneState {
            airborne: character.IsUsingWingsuit()
                || character.IsUsingParachute()
                || character.IsInAttachedToParachute()
                || character.IsFreefalling()
                || character.IsFastfalling(),
            reeling: character.IsPreReelingIn() || character.IsReelingIn(),
            active_wire: hook.is_some_and(|hook| !hook.m_ActiveWire.px.is_null()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_airborne_locomotion_without_the_grapple_waives_the_permission() {
        let airborne = AirborneState {
            airborne: true,
            ..AirborneState::default()
        };
        assert!(waived(airborne));
        assert!(!waived(AirborneState::default()));
        assert!(!waived(AirborneState {
            reeling: true,
            ..airborne
        }));
        assert!(!waived(AirborneState {
            active_wire: true,
            ..airborne
        }));
    }
}
