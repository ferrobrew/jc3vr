//! The fixed set of timed regions and how each is classified in the breakdown.

/// A timed region of the frame. The set is closed and small so the counters can live in a static
/// array indexed by the slot, with no registration and no lookup on the hot path.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Slot {
    /// `crate::update`: the directing tools, the eye-capture upkeep, and the egui debug UI pass.
    DebugUi,
    /// The engine's `Game::Update`, excluding the mod scopes nested in it: the sim tick and the
    /// parts of the frame the mod does not hook.
    EngineUpdate,
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
    /// HUD quad, the egui panel, and the eye captures.
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
        Slot::InputTick,
        Slot::Bindings,
        Slot::PhysicalHands,
        Slot::Aim,
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
    pub(crate) const COUNT: usize = 22;

    /// The slot's position in [`Self::ALL`] and in the counter arrays.
    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    /// The slot's name in the log line and the debug UI.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Slot::DebugUi => "debug_ui",
            Slot::EngineUpdate => "engine_update",
            Slot::InputTick => "input_tick",
            Slot::Bindings => "bindings",
            Slot::PhysicalHands => "physical_hands",
            Slot::Aim => "aim",
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
            | Slot::UpdateRenderHook
            | Slot::VrUpdate
            | Slot::Submit
            | Slot::Mirror
            | Slot::Tail
            | Slot::PostDraw => Kind::Mod,
            Slot::VrBeginFrame | Slot::VrInput | Slot::VrEndFrame => Kind::Runtime,
            Slot::EngineUpdate | Slot::EngineUpdateRender | Slot::Dispatch => Kind::Engine,
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
