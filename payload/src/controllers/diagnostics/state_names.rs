//! The names behind the body rule states' hashes that the mod knows, so that a recorded state reads
//! as its name rather than as a hash.
//!
//! The game identifies a rule state by the hash of its name; the release binary holds only the
//! hashes. The names here are the ones the mod's own code and the engine documents cite, from the
//! shipped animation graphs, so a state outside them is recorded by its hash alone.
//! `tools/anim_graph` decodes the graphs for every state's name.

use std::{collections::HashMap, sync::OnceLock};

use jc3gi::hash::hashlittle;

/// The name of the rule state whose name hash is `hash`, when the mod knows it.
pub(super) fn name(hash: u32) -> Option<&'static str> {
    static TABLE: OnceLock<HashMap<u32, &'static str>> = OnceLock::new();
    TABLE
        .get_or_init(|| table(|name| hashlittle(name.as_bytes()) as u32))
        .get(&hash)
        .copied()
}

/// The known names by their hash under `hash`.
fn table(hash: impl Fn(&str) -> u32) -> HashMap<u32, &'static str> {
    NAMES.iter().map(|&name| (hash(name), name)).collect()
}

/// The known rule state names, sorted.
const NAMES: &[&str] = &[
    "S_AIM_FOCUS_ROTATE_90L",
    "S_COMBAT_IDLE_TO_UNARMED_IDLE",
    "S_CRASH_LAND_RAGDOLL",
    "S_DRIVER_HIDE_IN_VEHICLE",
    "S_DRIVER_TO_PASS1_VEHICLE",
    "S_DRIVER_TO_PASSENGER",
    "S_DRIVER_TO_STUNT_VEHICLE",
    "S_EJECT_FROM_VEHICLE_ALIVE",
    "S_EJECT_FROM_VEHICLE_DEAD",
    "S_ENTERING_VEHICLE_RIGHT",
    "S_ENTER_DRIVER_VEHICLE",
    "S_ENTER_MC",
    "S_ENTER_MG",
    "S_ENTER_MG_WITH_MG",
    "S_ENTER_PASSENGER_VEHICLE",
    "S_ENTER_VEHICLE_FROM_GROUND",
    "S_EXITING_VEHICLE",
    "S_EXIT_VEHICLE_TO_GROUND",
    "S_FALLING",
    "S_FALL_TO_FREEFALL",
    "S_FAST_FALLING",
    "S_FINISHED_ENTER_LEFT",
    "S_FINISHED_EXIT_LEFT",
    "S_FINISHED_EXIT_RIGHT",
    "S_FORWARD_TO_REVERSE_DRIVER_VEHICLE",
    "S_FREE_FALLING",
    "S_FULL_RAGDOLL",
    "S_GETUP",
    "S_GETUP_POSEMATCHING",
    "S_GLOBAL_PARTIAL_NULL",
    "S_GRAPPLE_ATTACH_0",
    "S_GRAPPLE_ATTACH_UPSIDE_DOWN",
    "S_GRAPPLE_CLIMB_START",
    "S_GRAPPLE_HANG",
    "S_GRAPPLE_PRE_UPSIDEDOWN",
    "S_GRAPPLE_REEL_TO_LAND",
    "S_GROUND_TO_NEUTRAL",
    "S_GUITAR_CASE_EXIT",
    "S_HEAVY_WEAPON_PICKUP",
    "S_HIJACK_STRUGGLE_FAIL",
    "S_IDLE",
    "S_IDLE_AIM_FOCUS",
    "S_IDLE_AIM_FOCUS_ADDITIVE",
    "S_IDLE_DRIVER_VEHICLE",
    "S_IDLE_HANG_STUNT",
    "S_IDLE_HANG_STUNT2",
    "S_IDLE_HANG_STUNT_120L",
    "S_IDLE_ONE_OFF",
    "S_IDLE_PASSENGER_VEHICLE",
    "S_IDLE_PICKUP",
    "S_IDLE_STUNT",
    "S_IDLE_TRANS_TO_AIM",
    "S_IDLE_TRANS_TO_NO_AIM",
    "S_IDLE_TRANS_TO_NO_AIM_SLOW",
    "S_IDLE_VEHICLE",
    "S_IDLE_VEHICLE_MG",
    "S_INITIAL_ENTER_LEFT",
    "S_INITIAL_EXIT_RIGHT",
    "S_JC_BOOST_CRASH_RECOVERY",
    "S_JC_BOOST_HIGH_VELOCITY_CRASH_RECOVERY",
    "S_JC_BOOST_WINGSUIT_BREAK_TO_LAND",
    "S_JC_PARACHUTE_IDLE",
    "S_JC_PARACHUTE_RUN_LAND",
    "S_JC_PARACHUTE_SLIDE_LAND",
    "S_JC_SEQUENCE_TO_FREEFALL",
    "S_JC_WINGSUIT_BOOST_BREAK",
    "S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND",
    "S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO",
    "S_JC_WINGSUIT_IDLE",
    "S_JC_WINGSUIT_TO_FALL",
    "S_JC_WINGSUIT_TO_FALL_SLOW",
    "S_JC_WINGSUIT_TO_LAND_SLIDE",
    "S_JC_WINGSUIT_TO_LAND_SLIDE_ONGROUND",
    "S_JUMP",
    "S_LAND_FAST_FALL",
    "S_LAND_HARD",
    "S_LAND_IDLE_0",
    "S_LAND_RUN_0",
    "S_MOVE_AIM_FOCUS",
    "S_MOVE_AIM_STRAFE",
    "S_MOVE_AIM_STRAFE_ADDITIVE",
    "S_MOVE_BWD_PICKUP",
    "S_MOVE_FWD_PICKUP",
    "S_MOVE_NON_COMBAT",
    "S_MOVE_NO_AIM",
    "S_MOVE_STRAFE",
    "S_MOVE_STUNT_AIM_STRAFE",
    "S_NEUTRAL_TO_DRIVER",
    "S_NEUTRAL_TO_STRUGGLE",
    "S_PASS1_TO_DRIVER_VEHICLE",
    "S_PASSENGER_TO_DRIVER",
    "S_PASS_HIDE_IN_VEHICLE",
    "S_RAGDOLL_TO_FREEFALL",
    "S_REVERSE_DRIVER_VEHICLE",
    "S_REVERSE_MC",
    "S_REVERSE_TO_FORWARD_DRIVER_VEHICLE",
    "S_RIDING_MC",
    "S_RIDING_MC_AIM_0",
    "S_RIDING_MC_AIM_135L",
    "S_RIDING_MC_AIM_135R",
    "S_RUN",
    "S_RUN_STOP",
    "S_RUN_TO_STOP_TURN_0",
    "S_STUNT_BACKFLIP_JUMP_EXIT_VEHICLE_DETACHED",
    "S_STUNT_BWD",
    "S_STUNT_FWD",
    "S_STUNT_LEFT",
    "S_STUNT_RIGHT",
    "S_STUNT_TO_DRIVER_VEHICLE",
    "S_STUNT_TO_JUMP_EXIT_VEHICLE",
    "S_SWITCHING_SEAT_TO_DRIVER",
    "S_SWITCH_DRIVING_DIRECTION",
    "S_UB_RELOAD_GRENADE_LAUNCHER_UPU210_LEFT",
    "S_UB_RELOAD_MACHINE_GUN_VDOVA_89_LEFT",
    "S_UB_RELOAD_RIFLE_BACK_LEFT",
    "S_UB_RELOAD_RIFLE_LEFT",
    "S_UB_RELOAD_SHOTGUN_START_LEFT",
    "S_UB_SHOTGUN_PUMP_LEFT",
    "S_WALK_STOP",
    "S_WALK_TO_STOP_TURN_0",
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn the_names_are_sorted_unique_state_names() {
        assert!(NAMES.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(NAMES.iter().all(|name| name.starts_with("S_")));
        let table = table(|name| {
            name.bytes().fold(0u32, |hash, byte| {
                hash.wrapping_mul(31).wrapping_add(u32::from(byte))
            })
        });
        assert_eq!(table.len(), NAMES.len());
        assert_eq!(
            table.values().copied().collect::<HashSet<_>>().len(),
            NAMES.len()
        );
    }
}
