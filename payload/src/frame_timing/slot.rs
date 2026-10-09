//! The fixed set of timed regions and how each is classified in the breakdown.

/// A timed region of the frame. The set is closed and small so the counters can live in a static
/// array indexed by the slot, with no registration and no lookup on the hot path.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Slot {
    /// `crate::update`: the directing tools, the eye-capture upkeep, and the egui debug UI pass.
    DebugUi,
    /// The engine's `Game::Update`, excluding the mod scopes nested in it and the sim ticks: the
    /// tick scheduler, the update contexts, and the parts of the frame the mod does not hook.
    EngineUpdate,
    /// One sim tick (`Game::UpdateGame`), excluding the scopes nested in it: the run state's
    /// managers, its waits on the AI and light fragments, and everything in the tick this
    /// breakdown does not time on its own.
    SimTick,
    /// The game world's pre-simulation queue (`GameWorld::UpdatePreSim`): the registered objects'
    /// pre-simulation updates, the characters' state-machine and pre-physics passes among them.
    SimWorldPreSim,
    /// The run state's physics phase (`GameState::UpdatePhysics`): the physics step and the
    /// collision, damage, and shape-sensor dispatch after it, excluding the hand bodies' callback.
    SimPhysics,
    /// The game world's post-simulation queue (`GameWorld::UpdatePostSim`): the objects'
    /// post-simulation updates, the teleport, and the removal queue.
    SimWorldPostSim,
    /// The tick's time-budgeted streaming block: the resource streamer, the location system, and
    /// the deletion of queued objects.
    SimStreaming,
    /// The first AI thread's body (`RunAIThread1_Proxy`), on the fragment worker that runs it.
    AiThread1,
    /// The second AI thread's body (`RunAIThread2_Proxy`), on the fragment worker that runs it. It
    /// is kicked at the end of a tick and runs alongside the render update.
    AiThread2,
    /// The mod's work after the engine's input-device poll, excluding the bindings and the
    /// physical hands: the gamepad-setting hold and the headpose look handling.
    InputTick,
    /// The controller bindings' input tick (`controllers::bindings::input_tick`), which drives the
    /// controller subsystems.
    Bindings,
    /// The physical hands' input tick and post-aim update (`controllers::physical_hands`): the palm
    /// probes, pushes, and pickups.
    PhysicalHands,
    /// The local player's aim-control post-simulation update with the controllers active
    /// (`controllers::aim`), excluding the physical hands: the hand composition, the engine's own aim
    /// update it wraps (with the grapple visibility checks the mod makes immediate), and the hand
    /// rays.
    Aim,
    /// The mod's work in the character pose-pass hooks, around the HumanIK solve: the head, hand,
    /// stance, and crouch targets queued at the pose finalization's entry, the secondary hand pass's
    /// switches, and the head override and post-solve fix-ups after the prop effects. Excludes the
    /// solve gate.
    CharacterPose,
    /// The local player's HumanIK solve gate: the procedural body's pose and the controllers' wrist
    /// targets queued on it.
    BodyGate,
    /// The physical hands' pre-simulation callback, which drives the hand bodies before each physics
    /// step.
    HandBodies,
    /// The render-rate presentation layer (`controllers::presentation`): the frame's snapshot, and
    /// the upper body's re-solve for the draws of the player's body.
    Presentation,
    /// The mod's work in the UI capture thread's hooks: at the main UI movie's capture seam, the HUD
    /// partition, the overlay and reticle hiding, and the atlas's and the world markers' moves into
    /// their cells and back; and the pause menu's VR pages' per-frame update.
    UiCapture,
    /// The world markers' fallback raycasts in the simulation update, and their placement and depth
    /// sampling on eye 0's post-draw.
    WorldMarkers,
    /// The render-update hook's own glue, excluding everything timed separately inside it.
    UpdateRenderHook,
    /// `vr::update`: the OpenXR event pump.
    VrUpdate,
    /// `xrBeginFrame`, the view location, and the head location in the frame begin.
    VrBeginFrame,
    /// The controllers' OpenXR input update in the frame begin: the action sync, the action-state
    /// reads, and the space locations.
    VrInput,
    /// The engine's own render update.
    EngineUpdateRender,
    /// The engine's game-thread draw for each dispatch (`Game::Draw`).
    Dispatch,
    /// The blit and composition work of the frame submit, excluding the swapchain wait and
    /// `xrEndFrame`.
    Submit,
    /// The desktop mirror.
    Mirror,
    /// The deferred frame tail's own glue, excluding its drain, mirror, and submit.
    Tail,
    /// The mod's work in `RenderEngine::PostDraw` on the draw task: the world-space overlays, the
    /// HUD quad, the egui panel, and the eye captures, excluding the world markers' placement.
    PostDraw,
    /// `xrEndFrame`.
    VrEndFrame,
    /// Blocking on the VR runtime lock, which the deferred tail holds until it submits the previous
    /// frame.
    VrLock,
    /// `xrWaitFrame`: the compositor's pacing.
    VrWaitFrame,
    /// Acquiring and waiting on the swapchain image.
    VrSwapchainWait,
    /// The game thread waiting on the draw task between and after the dispatches.
    Drain,
    /// The deferred tail waiting on the draw task's completion signals.
    TailDrain,
}

/// What kind of time a slot measures, for the per-thread totals.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The mod's own CPU work.
    Mod,
    /// OpenXR calls that do work rather than pace: the begin, the view and input reads, the end.
    Runtime,
    /// The engine's own CPU work, with the mod's nested hooks taken out.
    Engine,
    /// Waiting on the compositor, the GPU, the draw task, or the frame tail.
    Wait,
}

/// Which thread a span of time was spent on, for the per-thread totals.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum ThreadClass {
    /// The game thread, which is what bounds the frame rate.
    Game,
    /// The deferred frame tail's worker.
    Tail,
    /// Every other thread: the draw task and the CPU fragment workers.
    Other,
}

impl Slot {
    /// Every slot, in index order.
    pub(crate) const ALL: [Slot; Self::COUNT] = [
        Slot::DebugUi,
        Slot::EngineUpdate,
        Slot::SimTick,
        Slot::SimWorldPreSim,
        Slot::SimPhysics,
        Slot::SimWorldPostSim,
        Slot::SimStreaming,
        Slot::AiThread1,
        Slot::AiThread2,
        Slot::InputTick,
        Slot::Bindings,
        Slot::PhysicalHands,
        Slot::Aim,
        Slot::CharacterPose,
        Slot::BodyGate,
        Slot::HandBodies,
        Slot::Presentation,
        Slot::UiCapture,
        Slot::WorldMarkers,
        Slot::UpdateRenderHook,
        Slot::VrUpdate,
        Slot::VrBeginFrame,
        Slot::VrInput,
        Slot::EngineUpdateRender,
        Slot::Dispatch,
        Slot::Submit,
        Slot::Mirror,
        Slot::Tail,
        Slot::PostDraw,
        Slot::VrEndFrame,
        Slot::VrLock,
        Slot::VrWaitFrame,
        Slot::VrSwapchainWait,
        Slot::Drain,
        Slot::TailDrain,
    ];

    /// The number of slots.
    pub(crate) const COUNT: usize = 35;

    /// The slot's position in [`Self::ALL`] and in the counter arrays.
    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    /// The slot's name in the log line and the debug UI.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Slot::DebugUi => "debug_ui",
            Slot::EngineUpdate => "engine_update",
            Slot::SimTick => "sim_tick",
            Slot::SimWorldPreSim => "sim_world_pre",
            Slot::SimPhysics => "sim_physics",
            Slot::SimWorldPostSim => "sim_world_post",
            Slot::SimStreaming => "sim_streaming",
            Slot::AiThread1 => "ai_thread_1",
            Slot::AiThread2 => "ai_thread_2",
            Slot::InputTick => "input_tick",
            Slot::Bindings => "bindings",
            Slot::PhysicalHands => "physical_hands",
            Slot::Aim => "aim",
            Slot::CharacterPose => "character_pose",
            Slot::BodyGate => "body_gate",
            Slot::HandBodies => "hand_bodies",
            Slot::Presentation => "presentation",
            Slot::UiCapture => "ui_capture",
            Slot::WorldMarkers => "world_markers",
            Slot::UpdateRenderHook => "update_render_hook",
            Slot::VrUpdate => "vr_update",
            Slot::VrBeginFrame => "vr_begin_frame",
            Slot::VrInput => "vr_input",
            Slot::EngineUpdateRender => "engine_update_render",
            Slot::Dispatch => "dispatch",
            Slot::Submit => "submit",
            Slot::Mirror => "mirror",
            Slot::Tail => "tail",
            Slot::PostDraw => "post_draw",
            Slot::VrEndFrame => "vr_end_frame",
            Slot::VrLock => "vr_lock",
            Slot::VrWaitFrame => "vr_wait_frame",
            Slot::VrSwapchainWait => "vr_swapchain_wait",
            Slot::Drain => "drain",
            Slot::TailDrain => "tail_drain",
        }
    }

    /// What kind of time the slot measures.
    pub(crate) const fn kind(self) -> Kind {
        match self {
            Slot::DebugUi
            | Slot::InputTick
            | Slot::Bindings
            | Slot::PhysicalHands
            | Slot::Aim
            | Slot::CharacterPose
            | Slot::BodyGate
            | Slot::HandBodies
            | Slot::Presentation
            | Slot::UiCapture
            | Slot::WorldMarkers
            | Slot::UpdateRenderHook
            | Slot::VrUpdate
            | Slot::Submit
            | Slot::Mirror
            | Slot::Tail
            | Slot::PostDraw => Kind::Mod,
            Slot::VrBeginFrame | Slot::VrInput | Slot::VrEndFrame => Kind::Runtime,
            Slot::EngineUpdate
            | Slot::SimTick
            | Slot::SimWorldPreSim
            | Slot::SimPhysics
            | Slot::SimWorldPostSim
            | Slot::SimStreaming
            | Slot::AiThread1
            | Slot::AiThread2
            | Slot::EngineUpdateRender
            | Slot::Dispatch => Kind::Engine,
            Slot::VrLock
            | Slot::VrWaitFrame
            | Slot::VrSwapchainWait
            | Slot::Drain
            | Slot::TailDrain => Kind::Wait,
        }
    }
}

impl Kind {
    /// Every kind, in index order.
    pub(crate) const ALL: [Kind; Self::COUNT] =
        [Kind::Mod, Kind::Runtime, Kind::Engine, Kind::Wait];

    /// The number of kinds.
    pub(crate) const COUNT: usize = 4;

    /// The kind's position in [`Self::ALL`] and in the bucket arrays.
    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    /// The kind's name in the log line and the debug UI.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Kind::Mod => "mod",
            Kind::Runtime => "openxr",
            Kind::Engine => "engine",
            Kind::Wait => "wait",
        }
    }
}

impl ThreadClass {
    /// Every thread class, in index order.
    pub(crate) const ALL: [ThreadClass; Self::COUNT] =
        [ThreadClass::Game, ThreadClass::Tail, ThreadClass::Other];

    /// The number of thread classes.
    pub(crate) const COUNT: usize = 3;

    /// The class's position in [`Self::ALL`] and in the bucket arrays.
    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    /// The class's name in the log line and the debug UI.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            ThreadClass::Game => "game",
            ThreadClass::Tail => "tail",
            ThreadClass::Other => "other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_are_in_index_order() {
        for (i, slot) in Slot::ALL.iter().enumerate() {
            assert_eq!(slot.index(), i, "{}", slot.name());
        }
        for (i, kind) in Kind::ALL.iter().enumerate() {
            assert_eq!(kind.index(), i);
        }
        for (i, class) in ThreadClass::ALL.iter().enumerate() {
            assert_eq!(class.index(), i);
        }
    }

    #[test]
    fn slot_names_are_unique() {
        let mut names: Vec<_> = Slot::ALL.iter().map(|s| s.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Slot::COUNT);
    }
}
