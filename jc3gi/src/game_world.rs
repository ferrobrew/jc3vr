#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The game world interface (`IGameWorld`), reached through the `Base::CSingle<IGameWorld>`
/// instance pointer. The concrete object is `CGameWorld`, which owns the object manager, the
/// world file, and the player-teleport state machine; the interface subobject is what every
/// caller (bookmarks, teleport triggers, mission starts) goes through. Only the vtable is mapped.
pub struct GameWorld {
    vftable: *const crate::game_world::GameWorldVftable,
}
fn _GameWorld_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], GameWorld>([0u8; 0x8]);
    }
    unreachable!()
}
impl GameWorld {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418089680usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl GameWorld {
    pub fn vftable(&self) -> *const crate::game_world::GameWorldVftable {
        self.vftable as *const crate::game_world::GameWorldVftable
    }
    pub const TeleportPlayerImpl_ADDRESS: usize = 0x1409AF820;
    /// `CGameWorld::TeleportPlayer`, the implementation behind the
    /// [`TeleportPlayer`](crate::game_world::GameWorld::TeleportPlayer) virtual: copies the request into
    /// the world's teleport state and arms the `TELEPORT_*` state machine
    /// [`UpdateTeleport`](crate::game_world::UpdateTeleport) services; starts the overlay fade-to-black when `use_fade` is set and
    /// [`TELEPORT_FLAG_NO_FADE`](crate::game_world::GameWorld::TELEPORT_FLAG_NO_FADE) is clear.
    pub unsafe fn TeleportPlayerImpl(
        &mut self,
        world: *const crate::types::math::Matrix4,
        state: *const crate::hash::HashString,
        use_load_state: bool,
        use_fade: bool,
        fade_in_time: f32,
        fade_out_time: f32,
        flags: u32,
        teleport_done_event: *mut u8,
        use_load_screen: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                world: *const crate::types::math::Matrix4,
                state: *const crate::hash::HashString,
                use_load_state: bool,
                use_fade: bool,
                fade_in_time: f32,
                fade_out_time: f32,
                flags: u32,
                teleport_done_event: *mut u8,
                use_load_screen: bool,
            ) = ::std::mem::transmute(Self::TeleportPlayerImpl_ADDRESS);
            f(
                self as *mut Self as _,
                world,
                state,
                use_load_state,
                use_fade,
                fade_in_time,
                fade_out_time,
                flags,
                teleport_done_event,
                use_load_screen,
            )
        }
    }
    pub const TeleportPlayerInstantImpl_ADDRESS: usize = 0x140A126C0;
    /// `CGameWorld::TeleportPlayerInstant`, the implementation behind the
    /// [`TeleportPlayerInstant`](crate::game_world::GameWorld::TeleportPlayerInstant) virtual. Writes the
    /// character's transform through its world-transform virtual, calls
    /// `CCharacter::ForceNeutralState`, sets the character's teleported flag bit, and resets the
    /// camera through [`GameCameraManager::ResetCamera`](crate::camera::game_camera_manager::GameCameraManager::ResetCamera),
    /// bracketed by the `game_teleporting_initiated` and `game_teleporting_completed` posted
    /// events.
    pub unsafe fn TeleportPlayerInstantImpl(
        &mut self,
        world: *const crate::types::math::Matrix4,
        state: *const crate::hash::HashString,
        flags: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                world: *const crate::types::math::Matrix4,
                state: *const crate::hash::HashString,
                flags: u32,
            ) = ::std::mem::transmute(Self::TeleportPlayerInstantImpl_ADDRESS);
            f(self as *mut Self as _, world, state, flags)
        }
    }
    pub const UpdatePreSimImpl_ADDRESS: usize = 0x1409C9FD0;
    /// `CGameWorld::UpdatePreSim`, the implementation behind the
    /// [`UpdatePreSim`](crate::game_world::GameWorld::UpdatePreSim) virtual. It takes the interface subobject (the
    /// singleton) as `self`.
    pub unsafe fn UpdatePreSimImpl(
        &mut self,
        context: *mut crate::game::GameObjectUpdateContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *mut crate::game::GameObjectUpdateContext,
            ) = ::std::mem::transmute(Self::UpdatePreSimImpl_ADDRESS);
            f(self as *mut Self as _, context)
        }
    }
    pub const UpdatePostSimImpl_ADDRESS: usize = 0x140A14B40;
    /// `CGameWorld::UpdatePostSim`, the implementation behind the
    /// [`UpdatePostSim`](crate::game_world::GameWorld::UpdatePostSim) virtual. It takes the interface subobject (the
    /// singleton) as `self`.
    pub unsafe fn UpdatePostSimImpl(
        &mut self,
        context: *mut crate::game::GameObjectUpdateContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *mut crate::game::GameObjectUpdateContext,
            ) = ::std::mem::transmute(Self::UpdatePostSimImpl_ADDRESS);
            f(self as *mut Self as _, context)
        }
    }
    pub const IsTeleportingImpl_ADDRESS: usize = 0x1409AF810;
    /// `CGameWorld::IsTeleporting`, the implementation behind the
    /// [`IsTeleporting`](crate::game_world::GameWorld::IsTeleporting) virtual: whether the teleport state
    /// machine is not idle.
    pub unsafe fn IsTeleportingImpl(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsTeleportingImpl_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    /// Runs the pre-simulation half of the world's update queue (the priorities below 1024)
    /// with `context`, unless the context is paused. The registered objects' pre-simulation
    /// updates run here, the characters' state-machine and pre-physics passes among them.
    /// The run state's update calls it once per tick, before
    /// [`GameState::UpdatePhysics`](crate::game::GameState::UpdatePhysics).
    pub unsafe fn UpdatePreSim(
        &mut self,
        context: *mut crate::game::GameObjectUpdateContext,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).UpdatePreSim).read();
            f(self as *mut Self as _, context)
        }
    }
    /// Advances the deferred teleport ([`UpdateTeleport`](crate::game_world::UpdateTeleport)), runs the post-simulation half of
    /// the world's update queue (the priorities from 1024 to 2047) with `context` unless the
    /// context is paused, services the object removal queue, and, when the world's water
    /// extensions are enabled, updates their splashes, wind, and collisions. The run state's
    /// update calls it once per tick, after
    /// [`GameState::UpdatePhysics`](crate::game::GameState::UpdatePhysics).
    pub unsafe fn UpdatePostSim(
        &mut self,
        context: *mut crate::game::GameObjectUpdateContext,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).UpdatePostSim).read();
            f(self as *mut Self as _, context)
        }
    }
    /// Queues a deferred player teleport that `CGameWorld::UpdateTeleport` services over the
    /// following frames through its `TELEPORT_*` state machine. `world` is the destination
    /// transform (the translation row places the character; the rotation sets the facing and
    /// resets the camera behind it). `state` is an optional character state to enter on
    /// arrival (null keeps the neutral state). With `use_load_state` the game enters the load
    /// game state so the destination streams in first, with a load screen if
    /// `use_load_screen`; without it the world must already be resident (the engine logs a
    /// warning past 500 m from the active camera). `use_fade` fades the overlay UI to black for
    /// `fade_out_time` seconds and back in over `fade_in_time`. `flags` is a bit set of
    /// [`TELEPORT_FLAG_KEEP_MISSION`](crate::game_world::GameWorld::TELEPORT_FLAG_KEEP_MISSION) and
    /// [`TELEPORT_FLAG_NO_FADE`](crate::game_world::GameWorld::TELEPORT_FLAG_NO_FADE). `teleport_done_event`, when
    /// non-null, is sent once the arrival completes. Asserts when a teleport is already in
    /// flight ([`IsTeleporting`](crate::game_world::GameWorld::IsTeleporting)). Main thread only.
    pub unsafe fn TeleportPlayer(
        &mut self,
        world: *const crate::types::math::Matrix4,
        state: *const crate::hash::HashString,
        use_load_state: bool,
        use_fade: bool,
        fade_in_time: f32,
        fade_out_time: f32,
        flags: u32,
        teleport_done_event: *mut u8,
        use_load_screen: bool,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).TeleportPlayer).read();
            f(
                self as *mut Self as _,
                world,
                state,
                use_load_state,
                use_fade,
                fade_in_time,
                fade_out_time,
                flags,
                teleport_done_event,
                use_load_screen,
            )
        }
    }
    /// Teleports the local player's character synchronously: initializes the character's
    /// transform to `world`, forces the neutral animation state, optionally enters `state`,
    /// resets the game camera behind the new transform, and re-enables the player and
    /// character. Fires `game_teleporting_initiated` and `game_teleporting_completed`, resets
    /// the current settlement, province, and region, and notifies the HUD. Aborts the active
    /// mission unless `flags` has
    /// [`TELEPORT_FLAG_KEEP_MISSION`](crate::game_world::GameWorld::TELEPORT_FLAG_KEEP_MISSION). No streaming: the
    /// destination must already be resident. Asserts when a deferred teleport is in flight.
    /// Main thread only.
    pub unsafe fn TeleportPlayerInstant(
        &mut self,
        world: *const crate::types::math::Matrix4,
        state: *const crate::hash::HashString,
        flags: u32,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).TeleportPlayerInstant).read();
            f(self as *mut Self as _, world, state, flags)
        }
    }
    /// Whether a deferred [`TeleportPlayer`](crate::game_world::GameWorld::TeleportPlayer) is still being serviced.
    pub unsafe fn IsTeleporting(&mut self) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).IsTeleporting).read();
            f(self as *mut Self as _)
        }
    }
}
impl GameWorld {
    /// A teleport `flags` bit: keep the active mission running across the teleport.
    /// Without it the mission manager aborts the active mission as part of the teleport.
    pub const TELEPORT_FLAG_KEEP_MISSION: u32 = 1;
    /// A teleport `flags` bit that [`Teleport::Teleport`](crate::game_world::Teleport::Teleport) adds when the teleport object's
    /// [`m_LeavePlayerDisabled`](crate::game_world::TeleportFlags::m_LeavePlayerDisabled) flag is set. Its effect in
    /// the teleport has not been traced.
    pub const TELEPORT_FLAG_LEAVE_PLAYER_DISABLED: u32 = 4;
    /// A teleport `flags` bit (deferred teleports only): skip the fade-to-black even
    /// when `use_fade` is set.
    pub const TELEPORT_FLAG_NO_FADE: u32 = 2;
}
impl std::convert::AsRef<GameWorld> for GameWorld {
    fn as_ref(&self) -> &GameWorld {
        self
    }
}
impl std::convert::AsMut<GameWorld> for GameWorld {
    fn as_mut(&mut self) -> &mut GameWorld {
        self
    }
}
#[repr(C, align(8))]
pub struct GameWorldVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_3: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_11: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_12: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    /// Runs the pre-simulation half of the world's update queue (the priorities below 1024)
    /// with `context`, unless the context is paused. The registered objects' pre-simulation
    /// updates run here, the characters' state-machine and pre-physics passes among them.
    /// The run state's update calls it once per tick, before
    /// [`GameState::UpdatePhysics`](crate::game::GameState::UpdatePhysics).
    pub UpdatePreSim: unsafe extern "system" fn(
        this: *mut crate::game_world::GameWorld,
        context: *mut crate::game::GameObjectUpdateContext,
    ),
    /// Advances the deferred teleport ([`UpdateTeleport`](crate::game_world::UpdateTeleport)), runs the post-simulation half of
    /// the world's update queue (the priorities from 1024 to 2047) with `context` unless the
    /// context is paused, services the object removal queue, and, when the world's water
    /// extensions are enabled, updates their splashes, wind, and collisions. The run state's
    /// update calls it once per tick, after
    /// [`GameState::UpdatePhysics`](crate::game::GameState::UpdatePhysics).
    pub UpdatePostSim: unsafe extern "system" fn(
        this: *mut crate::game_world::GameWorld,
        context: *mut crate::game::GameObjectUpdateContext,
    ),
    _vfunc_15: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_16: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_17: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    _vfunc_18: unsafe extern "system" fn(this: *mut crate::game_world::GameWorld),
    /// Queues a deferred player teleport that `CGameWorld::UpdateTeleport` services over the
    /// following frames through its `TELEPORT_*` state machine. `world` is the destination
    /// transform (the translation row places the character; the rotation sets the facing and
    /// resets the camera behind it). `state` is an optional character state to enter on
    /// arrival (null keeps the neutral state). With `use_load_state` the game enters the load
    /// game state so the destination streams in first, with a load screen if
    /// `use_load_screen`; without it the world must already be resident (the engine logs a
    /// warning past 500 m from the active camera). `use_fade` fades the overlay UI to black for
    /// `fade_out_time` seconds and back in over `fade_in_time`. `flags` is a bit set of
    /// [`TELEPORT_FLAG_KEEP_MISSION`](crate::game_world::GameWorld::TELEPORT_FLAG_KEEP_MISSION) and
    /// [`TELEPORT_FLAG_NO_FADE`](crate::game_world::GameWorld::TELEPORT_FLAG_NO_FADE). `teleport_done_event`, when
    /// non-null, is sent once the arrival completes. Asserts when a teleport is already in
    /// flight ([`IsTeleporting`](crate::game_world::GameWorld::IsTeleporting)). Main thread only.
    pub TeleportPlayer: unsafe extern "system" fn(
        this: *mut crate::game_world::GameWorld,
        world: *const crate::types::math::Matrix4,
        state: *const crate::hash::HashString,
        use_load_state: bool,
        use_fade: bool,
        fade_in_time: f32,
        fade_out_time: f32,
        flags: u32,
        teleport_done_event: *mut u8,
        use_load_screen: bool,
    ),
    /// Teleports the local player's character synchronously: initializes the character's
    /// transform to `world`, forces the neutral animation state, optionally enters `state`,
    /// resets the game camera behind the new transform, and re-enables the player and
    /// character. Fires `game_teleporting_initiated` and `game_teleporting_completed`, resets
    /// the current settlement, province, and region, and notifies the HUD. Aborts the active
    /// mission unless `flags` has
    /// [`TELEPORT_FLAG_KEEP_MISSION`](crate::game_world::GameWorld::TELEPORT_FLAG_KEEP_MISSION). No streaming: the
    /// destination must already be resident. Asserts when a deferred teleport is in flight.
    /// Main thread only.
    pub TeleportPlayerInstant: unsafe extern "system" fn(
        this: *mut crate::game_world::GameWorld,
        world: *const crate::types::math::Matrix4,
        state: *const crate::hash::HashString,
        flags: u32,
    ),
    /// Whether a deferred [`TeleportPlayer`](crate::game_world::GameWorld::TeleportPlayer) is still being serviced.
    pub IsTeleporting: unsafe extern "system" fn(
        this: *mut crate::game_world::GameWorld,
    ) -> bool,
}
fn _GameWorldVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xB0], GameWorldVftable>([0u8; 0xB0]);
    }
    unreachable!()
}
impl GameWorldVftable {}
impl std::convert::AsRef<GameWorldVftable> for GameWorldVftable {
    fn as_ref(&self) -> &GameWorldVftable {
        self
    }
}
impl std::convert::AsMut<GameWorldVftable> for GameWorldVftable {
    fn as_mut(&mut self) -> &mut GameWorldVftable {
        self
    }
}
#[repr(C, align(8))]
/// `CTeleport`: a placed game object that teleports the player to its own transform when
/// triggered by its teleport event, through [`GameWorld::TeleportPlayer`](crate::game_world::GameWorld::TeleportPlayer). Only the fields
/// [`Teleport`](crate::game_world::Teleport::Teleport) reads are mapped.
pub struct Teleport {
    _field_0: [u8; 200],
    /// The object's local transform.
    pub m_LocalTransform: crate::types::math::Matrix4,
    /// The object's world transform: the teleport destination.
    pub m_WorldMatrix: crate::types::math::Matrix4,
    /// The optional object whose arrival state is checked before teleporting.
    pub m_TeleportObjectId: u64,
    pub m_FadeInTime: f32,
    pub m_FadeOutTime: f32,
    /// The character state to enter on arrival.
    pub m_State: crate::hash::HashString,
    /// The packed flags.
    pub m_Flags: crate::game_world::TeleportFlags,
    _field_15d: [u8; 3],
}
fn _Teleport_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x160], Teleport>([0u8; 0x160]);
    }
    unreachable!()
}
impl Teleport {
    pub const SetTransform_ADDRESS: usize = 0x1404CECE0;
    /// `CTeleport::SetTransform`: copies `world_matrix` into
    /// [`m_WorldMatrix`](crate::game_world::Teleport::m_WorldMatrix).
    pub unsafe fn SetTransform(
        &mut self,
        world_matrix: *const crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                world_matrix: *const crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::SetTransform_ADDRESS);
            f(self as *mut Self as _, world_matrix)
        }
    }
    pub const Teleport_ADDRESS: usize = 0x14050E360;
    /// `CTeleport::Teleport`: issues [`GameWorld::TeleportPlayer`](crate::game_world::GameWorld::TeleportPlayer) to
    /// [`m_WorldMatrix`](crate::game_world::Teleport::m_WorldMatrix) with the object's state, fade times, and flags, posts
    /// the teleport message, and clears the pending-teleport bit
    /// ([`m_Teleport`](crate::game_world::TeleportFlags::m_Teleport)). Called from
    /// `CTeleport::UpdatePreSim` when a teleport is pending.
    pub unsafe fn Teleport(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Teleport_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<Teleport> for Teleport {
    fn as_ref(&self) -> &Teleport {
        self
    }
}
impl std::convert::AsMut<Teleport> for Teleport {
    fn as_mut(&mut self) -> &mut Teleport {
        self
    }
}
crate::__bitflags! {
    #[doc =
    " The packed flag bits of [`Teleport::m_Flags`](crate::game_world::Teleport::m_Flags) (`CTeleport`'s bitfield members, from the low bit"]
    #[doc = " upward)."] pub struct TeleportFlags : u8 { const m_Enabled = 1usize as _;
    const m_ForceInstant = 2usize as _; const m_Teleport = 4usize as _; const m_UseFade =
    8usize as _; const m_IgnoreFade = 16usize as _; const m_LeavePlayerDisabled = 32usize
    as _; const m_bUseLoadingScreen = 64usize as _; }
}
fn _TeleportFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1], TeleportFlags>([0u8; 0x1]);
    }
    unreachable!()
}
pub const UpdateTeleport_ADDRESS: usize = 0x140A128A0;
/// `CGameWorld::UpdateTeleport`: advances the deferred-teleport state machine a
/// [`GameWorld::TeleportPlayer`](crate::game_world::GameWorld::TeleportPlayer) request armed (fade, optional load state, character placement,
/// fade back in). Called from `CGameWorld::UpdatePostSim` each sim tick. `game_world` is the full
/// `CGameWorld` object, which embeds the [`GameWorld`](crate::game_world::GameWorld) interface subobject (the singleton) after
/// its own leading members, so the singleton pointer is not the start of the object.
pub unsafe fn UpdateTeleport(game_world: *mut u8, dt: f32) {
    unsafe {
        let f: unsafe extern "system" fn(game_world: *mut u8, dt: f32) = ::std::mem::transmute(
            UpdateTeleport_ADDRESS,
        );
        f(game_world, dt)
    }
}
