//! One recorded frame: the difference between two readings of the never-reset counters, taken a
//! frame apart on the game thread.

use jc3vr_core::frame_timing::{Cumulative, Kind, Slot, ThreadClass};

use crate::{hooks::draw_count::EngineCallTotals, testing::benchmark::detour_timing::DetourTotals};

/// A reading of every counter a frame row is made from.
#[derive(Clone, Debug)]
pub(crate) struct Reading {
    pub(crate) timing: Cumulative,
    pub(crate) engine: EngineCallTotals,
    pub(crate) submissions: u64,
    pub(crate) detours: DetourTotals,
    /// The submitted frames' summed head-pose age at `xrEndFrame`, and their count
    /// ([`crate::vr::pose_age_totals`]).
    pub(crate) pose_age: (u64, u64),
}

/// The names of a frame row's values, in order. Times are microseconds, counts are counts.
pub(crate) const COLUMNS: [&str; COLUMN_COUNT] = [
    "frame_us",
    "sim_ticks",
    "game_mod_us",
    "game_openxr_us",
    "game_engine_us",
    "game_wait_us",
    "tail_mod_us",
    "tail_openxr_us",
    "tail_wait_us",
    "other_mod_us",
    "other_engine_us",
    "other_wait_us",
    "draw_task_us",
    "draw_tasks",
    "post_draw_us",
    "dispatch_us",
    "drain_us",
    "tail_drain_us",
    "vr_wait_frame_us",
    "vr_begin_frame_us",
    "vr_end_frame_us",
    "vr_swapchain_wait_us",
    "vr_lock_us",
    "submit_us",
    "sim_us",
    "engine_update_us",
    "engine_update_render_us",
    "debug_ui_us",
    "engine_draws",
    "engine_dispatches",
    "d3d_submissions",
    "detour_us",
    "detour_calls",
    "forward_us",
    "forward_calls",
    "pose_age_us",
    "pose_ages",
];

pub(crate) const COLUMN_COUNT: usize = 37;

/// The slots that make up the simulation's time: the sim tick, its phases, and the managers timed
/// inside it.
const SIM_SLOTS: [Slot; 23] = [
    Slot::SimTick,
    Slot::SimWorldPreSim,
    Slot::SimPhysics,
    Slot::SimWorldPostSim,
    Slot::SimStreaming,
    Slot::SimHead,
    Slot::SimTail,
    Slot::SimOnline,
    Slot::SimStats,
    Slot::SimSteam,
    Slot::SimOperations,
    Slot::SimLogin,
    Slot::SimLoggedOn,
    Slot::OsuiteAccounts,
    Slot::OsuiteAuth,
    Slot::OsuiteTicket,
    Slot::OsuiteServlet,
    Slot::OsuiteFriends,
    Slot::OsuiteSem,
    Slot::OsuiteConsumables,
    Slot::OsuiteInfo,
    Slot::OsuiteCallbacks,
    Slot::InputJob,
];

/// The row for the frame between `earlier` and `later`, which lasted `frame_ns` and ran
/// `sim_ticks` simulation ticks.
pub(crate) fn frame_row(
    earlier: &Reading,
    later: &Reading,
    frame_ns: u64,
    sim_ticks: u32,
) -> [u64; COLUMN_COUNT] {
    let (a, b) = (&earlier.timing, &later.timing);
    let bucket = |class, kind| us(b.bucket_since(a, class, kind));
    let slot = |slot| us(b.slot_since(a, slot));
    let sim: u64 = SIM_SLOTS.iter().map(|&s| b.slot_since(a, s)).sum();
    [
        us(frame_ns),
        u64::from(sim_ticks),
        bucket(ThreadClass::Game, Kind::Mod),
        bucket(ThreadClass::Game, Kind::Runtime),
        bucket(ThreadClass::Game, Kind::Engine),
        bucket(ThreadClass::Game, Kind::Wait),
        bucket(ThreadClass::Tail, Kind::Mod),
        bucket(ThreadClass::Tail, Kind::Runtime),
        bucket(ThreadClass::Tail, Kind::Wait),
        bucket(ThreadClass::Other, Kind::Mod),
        bucket(ThreadClass::Other, Kind::Engine),
        bucket(ThreadClass::Other, Kind::Wait),
        slot(Slot::DrawTask),
        b.calls_since(a, Slot::DrawTask),
        slot(Slot::PostDraw),
        slot(Slot::Dispatch),
        slot(Slot::Drain),
        slot(Slot::TailDrain),
        slot(Slot::VrWaitFrame),
        slot(Slot::VrBeginFrame),
        slot(Slot::VrEndFrame),
        slot(Slot::VrSwapchainWait),
        slot(Slot::VrLock),
        slot(Slot::Submit),
        us(sim),
        slot(Slot::EngineUpdate),
        slot(Slot::EngineUpdateRender),
        slot(Slot::DebugUi),
        later.engine.draws.saturating_sub(earlier.engine.draws),
        later
            .engine
            .dispatches
            .saturating_sub(earlier.engine.dispatches),
        later.submissions.saturating_sub(earlier.submissions),
        us(later
            .detours
            .detour_ns
            .saturating_sub(earlier.detours.detour_ns)),
        later
            .detours
            .detour_calls
            .saturating_sub(earlier.detours.detour_calls),
        us(later
            .detours
            .forward_ns
            .saturating_sub(earlier.detours.forward_ns)),
        later
            .detours
            .forward_calls
            .saturating_sub(earlier.detours.forward_calls),
        us(later.pose_age.0.saturating_sub(earlier.pose_age.0)),
        later.pose_age.1.saturating_sub(earlier.pose_age.1),
    ]
}

/// Nanoseconds to whole microseconds, rounded to nearest.
fn us(ns: u64) -> u64 {
    ns.saturating_add(500) / 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Reading {
        Reading {
            timing: Cumulative {
                slot_self_ns: [0; Slot::COUNT],
                slot_calls: [0; Slot::COUNT],
                buckets: [[0; Kind::COUNT]; ThreadClass::COUNT],
            },
            engine: EngineCallTotals::default(),
            submissions: 0,
            detours: DetourTotals::default(),
            pose_age: (0, 0),
        }
    }

    fn column(row: &[u64; COLUMN_COUNT], name: &str) -> u64 {
        row[COLUMNS.iter().position(|c| *c == name).unwrap()]
    }

    #[test]
    fn the_columns_are_unique() {
        let mut names = COLUMNS.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COLUMN_COUNT);
    }

    #[test]
    fn a_row_is_the_difference_between_two_readings() {
        let mut earlier = empty();
        earlier.timing.slot_self_ns[Slot::DrawTask.index()] = 1_000_000;
        earlier.timing.slot_calls[Slot::DrawTask.index()] = 4;
        earlier.engine.draws = 100;
        earlier.submissions = 50;
        let mut later = earlier.clone();
        later.timing.slot_self_ns[Slot::DrawTask.index()] = 7_400_000;
        later.timing.slot_calls[Slot::DrawTask.index()] = 6;
        later.timing.slot_self_ns[Slot::SimPhysics.index()] = 1_250_000;
        later.timing.slot_self_ns[Slot::SimTick.index()] = 250_000;
        later.timing.buckets[ThreadClass::Game.index()][Kind::Wait.index()] = 9_000_400;
        later.engine.draws = 5_100;
        later.submissions = 4_050;
        later.detours.detour_ns = 2_000_000;
        later.detours.detour_calls = 3_000;
        let row = frame_row(&earlier, &later, 13_888_888, 1);
        assert_eq!(column(&row, "frame_us"), 13_889);
        assert_eq!(column(&row, "sim_ticks"), 1);
        assert_eq!(column(&row, "draw_task_us"), 6_400);
        assert_eq!(column(&row, "draw_tasks"), 2);
        assert_eq!(column(&row, "sim_us"), 1_500);
        assert_eq!(column(&row, "game_wait_us"), 9_000);
        assert_eq!(column(&row, "engine_draws"), 5_000);
        assert_eq!(column(&row, "d3d_submissions"), 4_000);
        assert_eq!(column(&row, "detour_us"), 2_000);
        assert_eq!(column(&row, "detour_calls"), 3_000);
        assert_eq!(column(&row, "forward_calls"), 0);
    }
}
