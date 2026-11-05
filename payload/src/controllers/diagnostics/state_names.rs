//! The names behind the body rule states' hashes that the mod knows, so that a recorded state reads
//! as its name rather than as a hash.
//!
//! The game identifies a rule state by the hash of its name; the release binary holds only the
//! hashes. The names here are the ones the mod's own code and the engine documents cite, from the
//! shipped animation graphs, so a state outside them is recorded by its hash alone.
//! `tools/anim_graph` decodes the graphs for every state's name.

use std::sync::LazyLock;

use crate::{
    controllers::{roomscale::seat::PHASE_STATES, vehicles::take_off::TAKE_OFF_STATES},
    hash::StateTable,
};

/// The name of the rule state whose name hash is `hash`, when the mod knows it.
pub(super) fn name(hash: u32) -> Option<&'static str> {
    static TABLE: LazyLock<StateTable<&'static str>> =
        LazyLock::new(|| StateTable::new(known_names().map(|name| (name, name))));
    TABLE.get(hash)
}

/// Every known name: the seat phases' and the take-off's, and the others the mod cites.
fn known_names() -> impl Iterator<Item = &'static str> {
    PHASE_STATES
        .iter()
        .flat_map(|(_, names)| names.iter().copied())
        .chain(TAKE_OFF_STATES)
        .chain(NAMES.iter().copied())
}

/// The other known rule state names, sorted.
const NAMES: &[&str] = &[
    "S_AIM_FOCUS_ROTATE_90L",
    "S_COMBAT_IDLE_TO_UNARMED_IDLE",
    "S_CRASH_LAND_RAGDOLL",
    "S_ENTER_DRIVER_VEHICLE",
    "S_ENTER_PASSENGER_VEHICLE",
    "S_FALLING",
    "S_FALL_TO_FREEFALL",
    "S_FAST_FALLING",
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
    "S_GUITAR_CASE_EXIT",
    "S_HEAVY_WEAPON_PICKUP",
    "S_IDLE",
    "S_IDLE_AIM_FOCUS",
    "S_IDLE_AIM_FOCUS_ADDITIVE",
    "S_IDLE_HANG_STUNT",
    "S_IDLE_HANG_STUNT2",
    "S_IDLE_HANG_STUNT_120L",
    "S_IDLE_ONE_OFF",
    "S_IDLE_PICKUP",
    "S_IDLE_STUNT",
    "S_IDLE_TRANS_TO_AIM",
    "S_IDLE_TRANS_TO_NO_AIM",
    "S_IDLE_TRANS_TO_NO_AIM_SLOW",
    "S_JC_BOOST_CRASH_RECOVERY",
    "S_JC_BOOST_HIGH_VELOCITY_CRASH_RECOVERY",
    "S_JC_BOOST_WINGSUIT_BREAK_TO_LAND",
    "S_JC_PARACHUTE_IDLE",
    "S_JC_PARACHUTE_RUN_LAND",
    "S_JC_PARACHUTE_SLIDE_LAND",
    "S_JC_SEQUENCE_TO_FREEFALL",
    "S_JC_WINGSUIT_BOOST_BREAK",
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
    "S_RAGDOLL_TO_FREEFALL",
    "S_RUN",
    "S_RUN_STOP",
    "S_RUN_TO_STOP_TURN_0",
    "S_STUNT_BACKFLIP_JUMP_EXIT_VEHICLE_DETACHED",
    "S_STUNT_BWD",
    "S_STUNT_FWD",
    "S_STUNT_LEFT",
    "S_STUNT_RIGHT",
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
    use crate::hash::hashlittle;

    #[test]
    fn the_names_are_sorted_unique_state_names() {
        assert!(NAMES.windows(2).all(|pair| pair[0] < pair[1]));
        let names: Vec<&str> = known_names().collect();
        assert!(names.iter().all(|name| name.starts_with("S_")));
        assert_eq!(names.iter().collect::<HashSet<_>>().len(), names.len());
        let hashes: HashSet<u32> = names
            .iter()
            .map(|name| hashlittle(name.as_bytes()))
            .collect();
        assert_eq!(hashes.len(), names.len());
    }
}
