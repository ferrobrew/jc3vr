#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct Game {
    _field_0: [u8; 16],
    pub m_CountAccumulator: u64,
    /// The sim tick rate, ticks per second. The constructor writes 30 here and into
    /// [`m_DefaultUpdateFrequency`](crate::game::Game::m_DefaultUpdateFrequency). [`Update`](crate::game::Game::Update)
    /// reads it every frame (raising it to 1 when it is not positive): the step it derives,
    /// one over this rate, is each tick's `m_Dt` and `m_OriginalDt` in the update context, and the
    /// rate the clock is forced to for the duration of the sim. Writing it changes the tick from
    /// the next frame on. [`PhysicsSystem::Update`](crate::physics::PhysicsSystem::Update) does not
    /// follow it: it steps the world by its own fixed step.
    pub m_UpdateFrequency: u32,
    /// The tick rate the constructor sets, kept beside [`m_UpdateFrequency`](crate::game::Game::m_UpdateFrequency).
    pub m_DefaultUpdateFrequency: u32,
    _field_20: [u8; 20],
    pub m_Exit: u8,
    _field_35: [u8; 75],
    pub m_FilteredDrawTime: u64,
    _field_88: [u8; 12],
    pub m_RenderCount: u32,
    /// The frame-pacing method [`Update`](crate::game::Game::Update) uses this frame, rechosen every frame:
    /// [`INTERPOLATION_METHOD_ACCUMULATED`](crate::game::Game::INTERPOLATION_METHOD_ACCUMULATED) while a cutscene
    /// plays, [`INTERPOLATION_METHOD_SINGLE_STEP`](crate::game::Game::INTERPOLATION_METHOD_SINGLE_STEP) otherwise,
    /// or [`m_InterpolationOverride`](crate::game::Game::m_InterpolationOverride) when that is non-negative.
    pub m_InterpolationMethod: u32,
    /// The method used on the previous frame; a change resets the accumulators. Set to
    /// [`INTERPOLATION_METHOD_PAUSED`](crate::game::Game::INTERPOLATION_METHOD_PAUSED) when the frame takes the
    /// paused path.
    pub m_PrevInterpolationMethod: u32,
    /// A console override for [`m_InterpolationMethod`](crate::game::Game::m_InterpolationMethod);
    /// [`INTERPOLATION_OVERRIDE_NONE`](crate::game::Game::INTERPOLATION_OVERRIDE_NONE) when unset.
    pub m_InterpolationOverride: i32,
    _field_a4: [u8; 444],
    pub m_DecoupleEnabled: bool,
    _field_261: [u8; 87],
    /// The [`GameUpdateFlags`](crate::game::GameUpdateFlags) bit set gating the phases of [`Update`](crate::game::Game::Update): the sim
    /// step, the render-bookkeeping step, and the tools step.
    pub m_UpdateFlags: u32,
    _field_2bc: [u8; 4],
}
fn _Game_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x2C0], Game>([0u8; 0x2C0]);
    }
    unreachable!()
}
impl Game {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417086568usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl Game {
    pub const Destructor_ADDRESS: usize = 0x140967600;
    /// `CGame::~CGame`: tears down the message-box manager, the game resource cache and handler,
    /// the profiler utility, and the version display.
    pub unsafe fn Destructor(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Destructor_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const Draw_ADDRESS: usize = 0x140952390;
    pub unsafe fn Draw(&self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self, dt: f32) = ::std::mem::transmute(
                Self::Draw_ADDRESS,
            );
            f(self as *const Self as _, dt)
        }
    }
    pub const Update_ADDRESS: usize = 0x1409604E0;
    /// The top-level per-frame update: advances the sim and drives rendering. Returns false on exit.
    ///
    /// Each frame it derives the sim step as one over [`m_UpdateFrequency`](crate::game::Game::m_UpdateFrequency),
    /// builds the update contexts with that step as their `m_Dt`, and, while
    /// [`E_GUF_UPDATE`](crate::game::GameUpdateFlags::E_GUF_UPDATE) is set, forces the clock to that rate
    /// (`CClock::EnableForceToFPS`), so that [`Clock::GetSPF`](crate::clock::Clock::GetSPF) returns the
    /// step during the sim; it undoes the forcing before [`UpdateRender`](crate::game::Game::UpdateRender).
    /// The frame's seconds per frame, capped at half a second, are converted into ticks by
    /// the chosen [`m_InterpolationMethod`](crate::game::Game::m_InterpolationMethod), each tick one
    /// [`UpdateGame`](crate::game::Game::UpdateGame) call, and the fraction left over is the render context's
    /// `m_Dtf`. While the clock is paused, or [`m_DecoupleEnabled`](crate::game::Game::m_DecoupleEnabled) is
    /// clear, it instead runs one [`UpdateGame`](crate::game::Game::UpdateGame) with the frame's time as the
    /// step.
    pub unsafe fn Update(&mut self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) -> bool = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const UpdateRender_ADDRESS: usize = 0x14095A4E0;
    /// Builds the per-frame update contexts and drives rendering.
    pub unsafe fn UpdateRender(
        &mut self,
        update_contexts: *mut crate::game::UpdateContexts,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                update_contexts: *mut crate::game::UpdateContexts,
            ) = ::std::mem::transmute(Self::UpdateRender_ADDRESS);
            f(self as *mut Self as _, update_contexts)
        }
    }
    pub const UpdateGame_ADDRESS: usize = 0x1409521F0;
    /// One sim tick: advances the game clock, updates the FPS counter, and dispatches to the
    /// active game state's update (install, init, frontend, load, or run). Called zero or more
    /// times per [`Update`](crate::game::Game::Update) according to the frame-pacing mode.
    pub unsafe fn UpdateGame(
        &mut self,
        update_contexts: *mut crate::game::UpdateContexts,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                update_contexts: *mut crate::game::UpdateContexts,
            ) = ::std::mem::transmute(Self::UpdateGame_ADDRESS);
            f(self as *mut Self as _, update_contexts)
        }
    }
}
impl Game {
    /// The [`m_InterpolationMethod`](crate::game::Game::m_InterpolationMethod) chosen while a cutscene plays:
    /// the same accumulator, but [`UpdateGame`](crate::game::Game::UpdateGame) runs once per whole step
    /// accumulated, subtracting one per update, up to [`Game_MaxGameUpdatesPerRender`](get_Game_MaxGameUpdatesPerRender) a frame;
    /// the frame's contribution to the accumulator is capped at the same count, so a frame that
    /// would need more ticks loses the time. A filtered-timer guard drops the frame to one tick
    /// when the recent ticks took longer than a step.
    pub const INTERPOLATION_METHOD_ACCUMULATED: u32 = 2;
    /// The [`m_PrevInterpolationMethod`](crate::game::Game::m_PrevInterpolationMethod) value written when the
    /// frame takes the paused path; it matches no method, so the next unpaused frame resets the
    /// accumulators.
    pub const INTERPOLATION_METHOD_PAUSED: u32 = 4294967295;
    /// The [`m_InterpolationMethod`](crate::game::Game::m_InterpolationMethod) chosen during gameplay: the
    /// fractional sim step accumulates in `m_RunningFrameFraction` and [`UpdateGame`](crate::game::Game::UpdateGame)
    /// runs at most once per render frame. The method names are descriptive; the engine uses bare
    /// integers.
    pub const INTERPOLATION_METHOD_SINGLE_STEP: u32 = 1;
    /// The [`m_InterpolationMethod`](crate::game::Game::m_InterpolationMethod) that derives the number of sim
    /// steps from `QueryPerformanceCounter` wall-clock time since the method was entered, running
    /// several [`UpdateGame`](crate::game::Game::UpdateGame) calls per frame when rendering falls behind. Only
    /// reachable through [`m_InterpolationOverride`](crate::game::Game::m_InterpolationOverride).
    pub const INTERPOLATION_METHOD_TIME_BASED: u32 = 3;
    /// The [`m_InterpolationOverride`](crate::game::Game::m_InterpolationOverride) value meaning no override:
    /// the method chosen each frame stands.
    pub const INTERPOLATION_OVERRIDE_NONE: i32 = -1;
    /// The intra-`Update` site at which the game's per-frame render bookkeeping starts: the
    /// [`E_GUF_RENDER`](crate::game::GameUpdateFlags::E_GUF_RENDER) check (the `jz`) that normally gates `UpdateRender` (and everything
    /// after it) on the frame-pacing mode sits here.
    pub const UPDATE_RENDER_GATE_OFFSET: u64 = 1927;
    /// The intra-`Update` site at which the render bookkeeping ends: `++m_RenderCount`, the last
    /// step of the `UpdateRender` tail.
    pub const UPDATE_RENDER_LENGTH_OFFSET: u64 = 1954;
}
impl std::convert::AsRef<Game> for Game {
    fn as_ref(&self) -> &Game {
        self
    }
}
impl std::convert::AsMut<Game> for Game {
    fn as_mut(&mut self) -> &mut Game {
        self
    }
}
#[repr(C, align(8))]
/// A game object's `CGameObject` subobject, through which the engine's run-time type tests go. A
/// [`PhysicsGameObject`](crate::physics::PhysicsGameObject) carries it at
/// [`GAME_OBJECT_BASE_OFFSET`](crate::physics::PhysicsGameObject::GAME_OBJECT_BASE_OFFSET). Only some
/// virtuals are bound.
pub struct GameObject {
    vftable: *const crate::game::GameObjectVftable,
}
fn _GameObject_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], GameObject>([0u8; 0x8]);
    }
    unreachable!()
}
impl GameObject {
    pub fn vftable(&self) -> *const crate::game::GameObjectVftable {
        self.vftable as *const crate::game::GameObjectVftable
    }
    pub const GetUniqueSpawnTagIndex_ADDRESS: usize = 0x1404CCF60;
    /// The object's spawn tag index (`CGameObject::GetUniqueSpawnTagIndex`): the index of the
    /// spawn tag the spawn system spawned it from, which identifies its type. The spawn system
    /// converts it to the tag's hash, the persistent form, with
    /// [`GetTagHashFromTagIndex`](crate::spawn_system::SpawnSystem::GetTagHashFromTagIndex). The player
    /// save data stores each inventory weapon's type this way, and the inventory's respawn table
    /// ([`Inventory::m_WeaponToSpawn`](crate::character::inventory::Inventory::m_WeaponToSpawn)) holds it.
    pub unsafe fn GetUniqueSpawnTagIndex(&self) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> i32 = ::std::mem::transmute(
                Self::GetUniqueSpawnTagIndex_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    /// `CRtti::IsType`: whether the object is of the class `type_id` names or derives from it.
    /// `type_id` points at the class's run-time type id, as a class's `TYPE_ID` function
    /// returns it (such as [`Character::TYPE_ID`](crate::character::character::Character::TYPE_ID)).
    pub unsafe fn IsType(&self, type_id: *const u32) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).IsType).read();
            f(self as *const Self as _, type_id)
        }
    }
    /// Enables or disables the object. A rigid object's override
    /// ([`RigidObject`](crate::physics::RigidObject)) records the state and, when it changes, adds its
    /// body to the physics world on enabling and removes it on disabling. A gun's spare
    /// magazine prop ([`WeaponData::m_MagazineObject`](crate::equipment::weapon::WeaponData::m_MagazineObject))
    /// is disabled from its spawn until
    /// [`ThrowAwayMagazineObject`](crate::equipment::weapon::WeaponData::ThrowAwayMagazineObject)
    /// enables it.
    pub unsafe fn Enable(&mut self, enable: bool) {
        unsafe {
            let f = (&raw const (*self.vftable()).Enable).read();
            f(self as *mut Self as _, enable)
        }
    }
}
impl std::convert::AsRef<GameObject> for GameObject {
    fn as_ref(&self) -> &GameObject {
        self
    }
}
impl std::convert::AsMut<GameObject> for GameObject {
    fn as_mut(&mut self) -> &mut GameObject {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
pub struct GameObjectInitContext {
    pub m_Dt: f32,
    pub m_DtIgnorePause: f32,
    pub m_RealDt: f32,
    _field_c: [u8; 4],
    pub m_ResourceCache: *const crate::game::ResourceCache,
    pub m_ProjectContext: *const crate::game::ProjectContext,
}
fn _GameObjectInitContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], GameObjectInitContext>([0u8; 0x20]);
    }
    unreachable!()
}
impl GameObjectInitContext {}
impl std::convert::AsRef<GameObjectInitContext> for GameObjectInitContext {
    fn as_ref(&self) -> &GameObjectInitContext {
        self
    }
}
impl std::convert::AsMut<GameObjectInitContext> for GameObjectInitContext {
    fn as_mut(&mut self) -> &mut GameObjectInitContext {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
pub struct GameObjectRenderContext {
    pub m_Dt: f32,
    pub m_Dtf: f32,
    pub m_OriginalDt: f32,
    pub m_DtIgnorePause: f32,
    pub m_RealDt: f32,
    _field_14: [u8; 4],
    pub m_ProjectContext: *const crate::game::ProjectContext,
}
fn _GameObjectRenderContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], GameObjectRenderContext>([0u8; 0x20]);
    }
    unreachable!()
}
impl GameObjectRenderContext {}
impl std::convert::AsRef<GameObjectRenderContext> for GameObjectRenderContext {
    fn as_ref(&self) -> &GameObjectRenderContext {
        self
    }
}
impl std::convert::AsMut<GameObjectRenderContext> for GameObjectRenderContext {
    fn as_mut(&mut self) -> &mut GameObjectRenderContext {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
pub struct GameObjectUpdateContext {
    pub m_Dt: f32,
    pub m_OriginalDt: f32,
    pub m_DtIgnorePause: f32,
    pub m_RealDt: f32,
    pub m_SkippedDt: f32,
    pub m_Paused: bool,
    _field_15: [u8; 3],
    pub m_ProjectContext: *const crate::game::ProjectContext,
}
fn _GameObjectUpdateContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], GameObjectUpdateContext>([0u8; 0x20]);
    }
    unreachable!()
}
impl GameObjectUpdateContext {}
impl std::convert::AsRef<GameObjectUpdateContext> for GameObjectUpdateContext {
    fn as_ref(&self) -> &GameObjectUpdateContext {
        self
    }
}
impl std::convert::AsMut<GameObjectUpdateContext> for GameObjectUpdateContext {
    fn as_mut(&mut self) -> &mut GameObjectUpdateContext {
        self
    }
}
#[repr(C, align(8))]
pub struct GameObjectVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    /// `CRtti::IsType`: whether the object is of the class `type_id` names or derives from it.
    /// `type_id` points at the class's run-time type id, as a class's `TYPE_ID` function
    /// returns it (such as [`Character::TYPE_ID`](crate::character::character::Character::TYPE_ID)).
    pub IsType: unsafe extern "system" fn(
        this: *const crate::game::GameObject,
        type_id: *const u32,
    ) -> bool,
    _vfunc_3: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_11: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_12: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_13: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_14: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_15: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_16: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_17: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_18: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_19: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_20: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_21: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_22: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_23: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_24: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_25: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_26: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_27: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_28: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_29: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_30: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_31: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_32: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_33: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_34: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_35: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_36: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    _vfunc_37: unsafe extern "system" fn(this: *mut crate::game::GameObject),
    /// Enables or disables the object. A rigid object's override
    /// ([`RigidObject`](crate::physics::RigidObject)) records the state and, when it changes, adds its
    /// body to the physics world on enabling and removes it on disabling. A gun's spare
    /// magazine prop ([`WeaponData::m_MagazineObject`](crate::equipment::weapon::WeaponData::m_MagazineObject))
    /// is disabled from its spawn until
    /// [`ThrowAwayMagazineObject`](crate::equipment::weapon::WeaponData::ThrowAwayMagazineObject)
    /// enables it.
    pub Enable: unsafe extern "system" fn(
        this: *mut crate::game::GameObject,
        enable: bool,
    ),
}
fn _GameObjectVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x138], GameObjectVftable>([0u8; 0x138]);
    }
    unreachable!()
}
impl GameObjectVftable {}
impl std::convert::AsRef<GameObjectVftable> for GameObjectVftable {
    fn as_ref(&self) -> &GameObjectVftable {
        self
    }
}
impl std::convert::AsMut<GameObjectVftable> for GameObjectVftable {
    fn as_mut(&mut self) -> &mut GameObjectVftable {
        self
    }
}
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
pub enum GameState {
    E_GAME_INSTALL = 0isize as _,
    E_GAME_INIT = 1isize as _,
    E_GAME_FRONTEND = 2isize as _,
    E_GAME_LOAD = 3isize as _,
    E_GAME_RUN = 4isize as _,
    E_GAME_STARTUP = 5isize as _,
    NOF_GAME_STATES = 6isize as _,
}
fn _GameState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], GameState>([0u8; 0x4]);
    }
    unreachable!()
}
impl GameState {
    pub unsafe fn get() -> Self {
        unsafe { *(0x142F3404C as *const Self) }
    }
}
impl GameState {
    pub const PostUpdateRender_ADDRESS: usize = 0x140A00E90;
    pub unsafe fn PostUpdateRender(update_contexts: *const crate::game::UpdateContexts) {
        unsafe {
            let f: unsafe extern "system" fn(
                update_contexts: *const crate::game::UpdateContexts,
            ) = ::std::mem::transmute(Self::PostUpdateRender_ADDRESS);
            f(update_contexts)
        }
    }
    pub const UpdatePhysics_ADDRESS: usize = 0x1409AC2A0;
    /// `CGameState::UpdatePhysics`: the run state's physics phase, between the game world's
    /// pre-simulation and post-simulation queues ([`GameWorld::UpdatePreSim`](crate::game_world::GameWorld::UpdatePreSim)
    /// and [`GameWorld::UpdatePostSim`](crate::game_world::GameWorld::UpdatePostSim)). Unless the clock's
    /// update speed is below a tenth (deep slow motion, which skips the step), it calls
    /// [`PhysicsSystem::Update`](crate::physics::PhysicsSystem::Update) with `dt`, the update context's
    /// `m_Dt`. It then dispatches the queued collisions, clears the collision array, runs the
    /// vehicles' post-collision update, processes the damage listeners' events, and updates the
    /// shape sensors.
    pub unsafe fn UpdatePhysics(dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(dt: f32) = ::std::mem::transmute(
                Self::UpdatePhysics_ADDRESS,
            );
            f(dt)
        }
    }
    pub const UpdateDeletionOfObjects_ADDRESS: usize = 0x1409BA410;
    /// `CGameState::UpdateDeletionOfObjects`: deletes the queued background objects and the
    /// locator's queued resources within `budget_us` microseconds. The run state's update calls
    /// it once per tick, after [`StreamerUpdate`](crate::game::StreamerUpdate) and [`GameLocationSystemUpdate`](crate::game::GameLocationSystemUpdate), with what is
    /// left of the tick's streaming budget.
    pub unsafe fn UpdateDeletionOfObjects(budget_us: u32) {
        unsafe {
            let f: unsafe extern "system" fn(budget_us: u32) = ::std::mem::transmute(
                Self::UpdateDeletionOfObjects_ADDRESS,
            );
            f(budget_us)
        }
    }
}
#[repr(C, align(8))]
/// `CGameStateRun`: the in-game state's static update functions.
pub struct GameStateRun {}
impl GameStateRun {
    pub const UpdateRenderPaused_ADDRESS: usize = 0x1409AE200;
    /// The render-side update the run state performs while the game is paused: drives the
    /// landscape, UI prerender, video, water simulation, and resource streaming, and waits for the
    /// CPU draw, without running the camera update.
    pub unsafe fn UpdateRenderPaused(update_contexts: *mut crate::game::UpdateContexts) {
        unsafe {
            let f: unsafe extern "system" fn(
                update_contexts: *mut crate::game::UpdateContexts,
            ) = ::std::mem::transmute(Self::UpdateRenderPaused_ADDRESS);
            f(update_contexts)
        }
    }
}
impl std::convert::AsRef<GameStateRun> for GameStateRun {
    fn as_ref(&self) -> &GameStateRun {
        self
    }
}
impl std::convert::AsMut<GameStateRun> for GameStateRun {
    fn as_mut(&mut self) -> &mut GameStateRun {
        self
    }
}
crate::__bitflags! {
    #[doc =
    " `CGame::EGameUpdateFlags`: the phases of [`Game::Update`](crate::game::Game::Update) enabled by"]
    #[doc = " [`Game::m_UpdateFlags`](crate::game::Game::m_UpdateFlags)."] pub struct
    GameUpdateFlags : u32 { const E_GUF_TOOLS = 1usize as _; const E_GUF_UPDATE = 2usize
    as _; const E_GUF_RENDER = 4usize as _; }
}
fn _GameUpdateFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], GameUpdateFlags>([0u8; 0x4]);
    }
    unreachable!()
}
/// `g_MaxGameUpdatesPerRender`: the most [`Game::UpdateGame`](crate::game::Game::UpdateGame) calls one [`Game::Update`](crate::game::Game::Update) makes
/// under [`INTERPOLATION_METHOD_ACCUMULATED`](crate::game::Game::INTERPOLATION_METHOD_ACCUMULATED) and
/// [`INTERPOLATION_METHOD_TIME_BASED`](crate::game::Game::INTERPOLATION_METHOD_TIME_BASED). The image
/// initializes it to 2.
pub unsafe fn get_Game_MaxGameUpdatesPerRender() -> &'static mut u32 {
    unsafe { &mut *(0x142D6EA50 as *mut u32) }
}
#[repr(C, align(8))]
pub struct ProjectContext {}
impl ProjectContext {}
impl std::convert::AsRef<ProjectContext> for ProjectContext {
    fn as_ref(&self) -> &ProjectContext {
        self
    }
}
impl std::convert::AsMut<ProjectContext> for ProjectContext {
    fn as_mut(&mut self) -> &mut ProjectContext {
        self
    }
}
#[repr(C, align(8))]
pub struct ResourceCache {}
impl ResourceCache {}
impl std::convert::AsRef<ResourceCache> for ResourceCache {
    fn as_ref(&self) -> &ResourceCache {
        self
    }
}
impl std::convert::AsMut<ResourceCache> for ResourceCache {
    fn as_mut(&mut self) -> &mut ResourceCache {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
pub struct UpdateContexts {
    pub m_InitContext: crate::game::GameObjectInitContext,
    pub m_UpdateContext: crate::game::GameObjectUpdateContext,
    pub m_RenderContext: crate::game::GameObjectRenderContext,
}
fn _UpdateContexts_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x60], UpdateContexts>([0u8; 0x60]);
    }
    unreachable!()
}
impl UpdateContexts {}
impl std::convert::AsRef<UpdateContexts> for UpdateContexts {
    fn as_ref(&self) -> &UpdateContexts {
        self
    }
}
impl std::convert::AsMut<UpdateContexts> for UpdateContexts {
    fn as_mut(&mut self) -> &mut UpdateContexts {
        self
    }
}
pub const StreamerUpdate_ADDRESS: usize = 0x14118A180;
/// The resource streamer's update: polls the file system and the decompressors, creates the
/// loaded resources, and starts the pending requests, within `budget_us` microseconds. The run
/// state's update calls it once per tick after the physics phase, with a budget of at least
/// a fixed minimum taken from a reservoir it refills every tick, so the streaming time per second
/// grows with the tick rate when there is work. Returns the streamer's result code.
pub unsafe fn StreamerUpdate(budget_us: u32) -> u32 {
    unsafe {
        let f: unsafe extern "system" fn(budget_us: u32) -> u32 = ::std::mem::transmute(
            StreamerUpdate_ADDRESS,
        );
        f(budget_us)
    }
}
pub const GameLocationSystemUpdate_ADDRESS: usize = 0x1404BC110;
/// The game location system's update: checks which locations to load and advances their loading
/// and the landmarks' visibility, within `budget_us` microseconds. The run state's update calls it
/// once per tick, after [`StreamerUpdate`](crate::game::StreamerUpdate).
pub unsafe fn GameLocationSystemUpdate(budget_us: u32) {
    unsafe {
        let f: unsafe extern "system" fn(budget_us: u32) = ::std::mem::transmute(
            GameLocationSystemUpdate_ADDRESS,
        );
        f(budget_us)
    }
}
pub const RunLightThread_ADDRESS: usize = 0x1409AE070;
/// `RunLightThread`: queues the light manager's update on a CPU fragment, with `dt` the tick's
/// step, and clears the signal the run state's update later waits on. The run state's update calls
/// it once per tick after the effects' post-simulation update.
pub unsafe fn RunLightThread(dt: f32) {
    unsafe {
        let f: unsafe extern "system" fn(dt: f32) = ::std::mem::transmute(
            RunLightThread_ADDRESS,
        );
        f(dt)
    }
}
pub const RunAIThread1_ADDRESS: usize = 0x1409F0D40;
/// `RunAIThread1`: queues [`RunAIThread1_Proxy`](crate::game::RunAIThread1_Proxy) on a CPU fragment with `dt` the tick's step, and
/// clears its completion signal. The run state's update calls it once per tick after the awareness
/// events' dispatch, and waits for the signal near the end of the same tick.
pub unsafe fn RunAIThread1(dt: f32) {
    unsafe {
        let f: unsafe extern "system" fn(dt: f32) = ::std::mem::transmute(
            RunAIThread1_ADDRESS,
        );
        f(dt)
    }
}
pub const RunAIThread1_Proxy_ADDRESS: usize = 0x1409F0B30;
/// The first AI thread's body, run on a CPU fragment worker (or by a thread that runs queued
/// fragments while it waits): the per-character AI update, the road graph, traffic, and action
/// token managers, and the pathfinder's instant-rotation switch. `dt` points at the tick's step.
/// Returns 1.
pub unsafe fn RunAIThread1_Proxy(dt: *mut f32) -> i32 {
    unsafe {
        let f: unsafe extern "system" fn(dt: *mut f32) -> i32 = ::std::mem::transmute(
            RunAIThread1_Proxy_ADDRESS,
        );
        f(dt)
    }
}
pub const RunAIThread2_ADDRESS: usize = 0x1409F0DE0;
/// `RunAIThread2`: caches the game camera's position for the AI pathfinder, then queues
/// [`RunAIThread2_Proxy`](crate::game::RunAIThread2_Proxy) on a CPU fragment with `dt` the tick's step, and clears its completion
/// signal. The run state's update calls it last in the tick, so the thread runs alongside the rest
/// of the frame: the run state's render update waits for the signal before its raycast threads,
/// and the next tick's update waits for it before its input.
pub unsafe fn RunAIThread2(dt: f32) {
    unsafe {
        let f: unsafe extern "system" fn(dt: f32) = ::std::mem::transmute(
            RunAIThread2_ADDRESS,
        );
        f(dt)
    }
}
pub const RunAIThread2_Proxy_ADDRESS: usize = 0x1409F0BC0;
/// The second AI thread's body, run on a CPU fragment worker: the pathfinders' work, the navmesh
/// pathfinder, the behavior trees, the AI system's update, and the terrain pathfinder; it queues a
/// further fragment of its own. `dt` points at the tick's step. Returns 1.
pub unsafe fn RunAIThread2_Proxy(dt: *mut f32) -> i32 {
    unsafe {
        let f: unsafe extern "system" fn(dt: *mut f32) -> i32 = ::std::mem::transmute(
            RunAIThread2_Proxy_ADDRESS,
        );
        f(dt)
    }
}
