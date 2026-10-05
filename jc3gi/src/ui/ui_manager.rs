#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A Scaleform render buffer: what [`UIManager::m_RenderBuffer`](crate::ui::ui_manager::UIManager::m_RenderBuffer) points at. It holds the
/// render-target and depth-stencil views the UI HAL renders into; [`UpdateData`](crate::ui::ui_manager::RenderTargetData::UpdateData)
/// rebinds those views, and is not tied to startup.
pub struct RenderTargetData {
    _field_0: [u8; 40],
    /// The render-target buffer width. [`InitPlatformRT`](crate::ui::ui_manager::UIManager::InitPlatformRT) builds the
    /// buffer square, setting width = height = its side argument.
    pub m_BufferWidth: i32,
    /// The render-target buffer height; [`InitPlatformRT`](crate::ui::ui_manager::UIManager::InitPlatformRT) sets it equal to
    /// the width (a square buffer).
    pub m_BufferHeight: i32,
    _field_30: [u8; 8],
    /// The view rectangle's right edge (x2 = width).
    pub m_ViewRectRight: i32,
    /// The view rectangle's bottom edge (y2); [`InitPlatformRT`](crate::ui::ui_manager::UIManager::InitPlatformRT) sets it
    /// equal to the width.
    pub m_ViewRectBottom: i32,
}
fn _RenderTargetData_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x40], RenderTargetData>([0u8; 0x40]);
    }
    unreachable!()
}
impl RenderTargetData {
    pub const UpdateData_ADDRESS: usize = 0x141DE0CF0;
    /// Rebinds the buffer's views, releasing the old ones and adding a reference to the new. `self`
    /// is the render buffer itself ([`UIManager::m_RenderBuffer`](crate::ui::ui_manager::UIManager::m_RenderBuffer)); the call reaches its inner
    /// view-holder internally. Passing `depth` as null leaves the depth buffer unchanged.
    pub unsafe fn UpdateData(
        &mut self,
        rtv: *mut ::std::ffi::c_void,
        depth: *mut ::std::ffi::c_void,
        dsv: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                rtv: *mut ::std::ffi::c_void,
                depth: *mut ::std::ffi::c_void,
                dsv: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::UpdateData_ADDRESS);
            f(self as *mut Self as _, rtv, depth, dsv)
        }
    }
}
impl std::convert::AsRef<RenderTargetData> for RenderTargetData {
    fn as_ref(&self) -> &RenderTargetData {
        self
    }
}
impl std::convert::AsMut<RenderTargetData> for RenderTargetData {
    fn as_mut(&mut self) -> &mut RenderTargetData {
        self
    }
}
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The screen-position code returned by the UI world-to-screen and marker placement: on-screen, an
/// off-screen clamped edge or corner, off-screen in front (no clamp), or off-screen behind.
pub enum ScreenPos {
    SCREEN_POS_ONSCREEN = 0isize as _,
    SCREEN_POS_OFFSCREEN_LEFT = 1isize as _,
    SCREEN_POS_OFFSCREEN_RIGHT = 2isize as _,
    SCREEN_POS_OFFSCREEN_TOP = 3isize as _,
    SCREEN_POS_OFFSCREEN_BOTTOM = 4isize as _,
    SCREEN_POS_OFFSCREEN_TOP_LEFT = 5isize as _,
    SCREEN_POS_OFFSCREEN_TOP_RIGHT = 6isize as _,
    SCREEN_POS_OFFSCREEN_BOTTOM_LEFT = 7isize as _,
    SCREEN_POS_OFFSCREEN_BOTTOM_RIGHT = 8isize as _,
    SCREEN_POS_OFFSCREEN_NO_CLAMP_IN_FRONT_CAMERA = 9isize as _,
    SCREEN_POS_OFFSCREEN_NO_CLAMP_BEHIND_CAMERA = 10isize as _,
}
fn _ScreenPos_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ScreenPos>([0u8; 0x4]);
    }
    unreachable!()
}
#[derive(Copy, Clone, Default)]
#[repr(C, align(4))]
/// A rectangle in movie stage coordinates (`Scaleform::Render::Rect<float>`): left, top, right,
/// bottom.
pub struct StageRect {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}
fn _StageRect_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], StageRect>([0u8; 0x10]);
    }
    unreachable!()
}
impl StageRect {}
impl std::convert::AsRef<StageRect> for StageRect {
    fn as_ref(&self) -> &StageRect {
        self
    }
}
impl std::convert::AsMut<StageRect> for StageRect {
    fn as_mut(&mut self) -> &mut StageRect {
        self
    }
}
#[repr(C, align(8))]
/// The Scaleform-backed UI manager, the single concrete instance behind `IUIManager`. It renders the
/// HUD into the engine surface. [`InitPlatformRT`](crate::ui::ui_manager::UIManager::InitPlatformRT) builds its render
/// buffer, [`ComputeMovieSizeOnViewSize`](crate::ui::ui_manager::UIManager::ComputeMovieSizeOnViewSize) sizes the movie
/// render rectangle, [`SetMovieViewport`](crate::ui::ui_manager::UIManager::SetMovieViewport) sets the Scaleform movie
/// viewport, and [`ComputeSafeArea`](crate::ui::ui_manager::UIManager::ComputeSafeArea) derives the UI safe area.
pub struct UIManager {
    vftable: *const crate::ui::ui_manager::UIManagerVftable,
    _field_8: [u8; 32],
    /// The horizontal mouse letterbox offset from the engine's `MovieScaleInfo`
    /// (`m_MouseDeltaX`), in movie-viewport pixels.
    /// [`GetMovieSpaceMouseCursor`](crate::ui::ui_manager::UIManager::GetMovieSpaceMouseCursor) maps a movie-viewport
    /// mouse position into stage coordinates as `(pos - delta) * scale`.
    /// [`ComputeMovieSizeOnViewSize`](crate::ui::ui_manager::UIManager::ComputeMovieSizeOnViewSize) writes it: always
    /// zero today (only the non-clipping path computes a non-trivial
    /// [`m_MouseDeltaY`](crate::ui::ui_manager::UIManager::m_MouseDeltaY)).
    pub m_MouseDeltaX: i32,
    /// The vertical mouse letterbox offset from the engine's `MovieScaleInfo` (`m_MouseDeltaY`),
    /// in movie-viewport pixels. In `ComputeMovieSizeOnViewSize`'s non-clipping path it is half
    /// the vertical letterbox, `(movie height - stage height * movie width / stage width) / 2`;
    /// in the clipping path it is zero.
    pub m_MouseDeltaY: i32,
    /// The movie-viewport-pixels-to-stage-coordinates mouse scale from the engine's
    /// `MovieScaleInfo` (`m_MouseScaleFac`): `stage width / movie rectangle width` in the
    /// non-clipping path of `ComputeMovieSizeOnViewSize`, `1.0` in the clipping path.
    pub m_MouseScaleFac: f32,
    /// The movie render rectangle's width, from the engine's `MovieScaleInfo`. The movie is rendered
    /// into a `m_MovieScaleWidth` x `m_MovieScaleHeight` rectangle, centered within the viewport
    /// passed to [`SetMovieViewport`](crate::ui::ui_manager::UIManager::SetMovieViewport).
    /// [`ComputeMovieSizeOnViewSize`](crate::ui::ui_manager::UIManager::ComputeMovieSizeOnViewSize) recomputes it from the
    /// device resolution and the movie stage size.
    pub m_MovieScaleWidth: i32,
    /// The movie render rectangle's height; see [`m_MovieScaleWidth`](crate::ui::ui_manager::UIManager::m_MovieScaleWidth).
    pub m_MovieScaleHeight: i32,
    _field_3c: [u8; 8],
    /// The id of the thread that currently owns the Scaleform capture (`m_CurrentCaptureThread`);
    /// `PreRender` claims it for the update thread each frame via `CUIManager::SetCaptureThread`,
    /// which writes this field and the movie's capture thread together. `CUIBase::Invoke` runs the
    /// AVM immediately only when the calling thread matches this field, and queues into the UI's
    /// command queue otherwise. It does not consult the movie's own capture thread: if the movie's
    /// capture thread changes without this field, game-thread invokes still run immediately and
    /// mutate the display list concurrently with the new capture owner.
    pub m_CurrentCaptureThread: u32,
    _field_48: [u8; 108],
    /// The last mouse position's x, in window-client pixels. [`WndProc`](crate::window::WndProc) writes it on every
    /// `WM_MOUSEMOVE` via [`SetMousePos`](crate::ui::ui_manager::UIManager::SetMousePos) -- the only writer, so the OS
    /// message stream is the sole source of the UI mouse position (the DirectInput mouse device
    /// only contributes deltas, buttons, and the wheel).
    pub m_MouseX: i32,
    /// The last mouse position's y, in window-client pixels; see
    /// [`m_MouseX`](crate::ui::ui_manager::UIManager::m_MouseX).
    pub m_MouseY: i32,
    _field_bc: [u8; 24],
    /// The grab-screen timer (seconds); `CHUDUI::OnOmniDamage` sets it to
    /// [`OMNI_DAMAGE_GRAB_SCREEN_TIME`](crate::ui::hud_ui::HUDUI::OMNI_DAMAGE_GRAB_SCREEN_TIME).
    pub m_GrabScreenTimer: f32,
    _field_d8: [u8; 314],
    /// Whether the render system is initialized. One of the three gates `Render` checks before
    /// drawing.
    pub m_RenderReady: bool,
    /// Whether rendering is active (cleared during device resets). The second `Render` gate.
    pub m_RenderActive: bool,
    _field_214: [u8; 12],
    /// The active `CSteering`, whose action map [`SendMouseEvents`](crate::ui::ui_manager::UIManager::SendMouseEvents)
    /// polls for the mouse-button actions ([`MOUSE1`](crate::input::input_action_map::Action::MOUSE1) and
    /// [`MOUSE2`](crate::input::input_action_map::Action::MOUSE2)).
    pub m_Steering: *mut crate::input::steering::Steering,
    _field_228: [u8; 4240],
    /// The lock serializing the UI update (`PreRender`: `Advance` + `Capture`) against the UI
    /// render (`Render`, `RenderOffScreenTextures`). A Win32 critical section, so re-entrant on
    /// the owning thread.
    pub m_DeferredRenderLock: *mut crate::graphics_engine::device::CRITICAL_SECTION,
    _field_12c0: [u8; 24],
    /// The Scaleform `GFx::Loader`, created in `InitializeSystem`. Loads `.gfx` files via
    /// `Loader::CreateMovie`.
    pub m_Loader: *mut ::std::ffi::c_void,
    /// The `GFx::MovieDef` for `ui/root.gfx` -- the definition object, not the live instance.
    pub m_MovieDef: *mut ::std::ffi::c_void,
    /// The live `GFx::Movie` instance (a [`MovieImpl`](crate::ui::scaleform::MovieImpl)), created by `MovieDef::CreateInstance` in
    /// `InitializeSystem`. All `CUIBase` subclasses share this single movie. The AS3 side (the
    /// [`Movie`](crate::ui::scaleform::Movie) interface: SetVariable, Invoke, the display tree) hangs off
    /// [`MovieImpl::pASMovieRoot`](crate::ui::scaleform::MovieImpl::pASMovieRoot).
    pub m_Movie: *mut crate::ui::scaleform::MovieImpl,
    _field_12f0: [u8; 56],
    /// The Scaleform `Render::D3D1x::HAL` the UI render worker draws through.
    pub m_RenderHAL: *mut crate::ui::scaleform::RenderHAL,
    _field_1330: [u8; 16],
    /// The UI safe area in stage coordinates (`m_SafeArea`), which
    /// [`ComputeSafeArea`](crate::ui::ui_manager::UIManager::ComputeSafeArea) derives and pushes to every UI (`CUIBase::UpdateSafeArea`),
    /// whose movies anchor their edge containers to it. [`Get2DInfo`](crate::ui::ui_manager::UIManager::Get2DInfo)'s on-screen
    /// test and edge clamp use it, shrunk by the margin.
    pub m_SafeArea: crate::ui::ui_manager::StageRect,
    /// The game view area in stage coordinates (`m_GameViewArea`), from
    /// [`ComputeGameViewArea`](crate::ui::ui_manager::UIManager::ComputeGameViewArea).
    pub m_GameViewArea: crate::ui::ui_manager::StageRect,
    /// The fraction of the stage kept clear at its edges (`m_SafeAreaPercentage`), from the safe-area
    /// setting. [`ComputeSafeArea`](crate::ui::ui_manager::UIManager::ComputeSafeArea) insets the safe area by half of it on
    /// each side.
    pub m_SafeAreaPercentage: f32,
    _field_1364: [u8; 4],
    /// The UI render worker's command queue; `Render` executes it (and stamps its
    /// [`m_RenderThreadId`](crate::ui::ui_manager::UiThreadCommandQueue::m_RenderThreadId)) at the top of every call.
    pub m_ThreadCommandQueue: *mut crate::ui::ui_manager::UiThreadCommandQueue,
    /// The Scaleform `D3D1x::TextureManager`; `Render` stamps its
    /// [`RenderThreadId`](crate::ui::ui_manager::UITextureManager::RenderThreadId) at the top of every call.
    pub m_TextureManager: *mut crate::ui::ui_manager::UITextureManager,
    _field_1378: [u8; 24],
    /// The Scaleform render buffer the UI HAL renders into, set up by
    /// [`InitPlatformRT`](crate::ui::ui_manager::UIManager::InitPlatformRT). [`RenderTargetData::UpdateData`](crate::ui::ui_manager::RenderTargetData::UpdateData) rebinds which
    /// views it renders into.
    pub m_RenderBuffer: *mut crate::ui::ui_manager::RenderTargetData,
    _field_1398: [u8; 229],
    /// Whether the player is currently driving the UI with a gamepad.
    /// [`PreUpdate`](crate::ui::ui_manager::UIManager::PreUpdate) copies it from the use-gamepad setting every frame; when the
    /// value changes it passes the new value to every registered UI, whose base implementation
    /// forwards it to the UI's movie as the ActionScript `OnUseGamepad` call. While set,
    /// [`SendMouseEvents`](crate::ui::ui_manager::UIManager::SendMouseEvents) parks the Scaleform mouse at
    /// [`GAMEPAD_PARKED_MOUSE_POSITION`](crate::ui::ui_manager::UIManager::GAMEPAD_PARKED_MOUSE_POSITION) on both axes and [`MousePointerVisibility`](crate::ui::ui_manager::UIManager::MousePointerVisibility) hides
    /// the overlay cursor.
    pub m_IsUsingGamepad: bool,
    _field_147e: [u8; 1],
    /// The third `Render` gate: whether UI rendering is enabled at all.
    pub m_RenderingEnabled: bool,
    _field_1480: [u8; 4],
    /// The movie stage's authored width (`m_CachedStageSize.x`), refreshed every frame from the loaded
    /// movie. The world-to-screen mapping ([`Convert3DCoords`](crate::ui::ui_manager::UIManager::Convert3DCoords)) maps NDC
    /// into this.
    pub m_CachedStageWidth: f32,
    /// The movie stage's authored height; see [`m_CachedStageWidth`](crate::ui::ui_manager::UIManager::m_CachedStageWidth).
    pub m_CachedStageHeight: f32,
    /// The movie stage's height over its width (`m_CachedStageRatio`), refreshed with
    /// [`m_CachedStageWidth`](crate::ui::ui_manager::UIManager::m_CachedStageWidth). [`Convert3DCoords`](crate::ui::ui_manager::UIManager::Convert3DCoords)
    /// compares it with [`m_CachedViewportRatio`](crate::ui::ui_manager::UIManager::m_CachedViewportRatio): when the viewport is
    /// relatively taller it scales the projected y by `viewport ratio / stage ratio`, and when it
    /// is relatively wider it scales x by `stage ratio / viewport ratio`, so the stage keeps its
    /// shape and the visible frame extends past the stage on the longer axis.
    pub m_CachedStageRatio: f32,
    /// The centre of the stage in stage coordinates (`m_CachedScreenCenter`).
    /// [`HUDUI::UpdatePOIs`](crate::ui::hud_ui::HUDUI::UpdatePOIs) measures each marker's label box from it.
    pub m_CachedScreenCenter: crate::types::math::Vector2,
    /// The cached viewport width (`m_CachedViewportSize.x`), refreshed every frame from the graphics
    /// device's display resolution. [`ComputeSafeArea`](crate::ui::ui_manager::UIManager::ComputeSafeArea) reads it to expand
    /// the UI safe area to the viewport's aspect.
    pub m_CachedViewportWidth: i32,
    /// The cached viewport height; see [`m_CachedViewportWidth`](crate::ui::ui_manager::UIManager::m_CachedViewportWidth).
    pub m_CachedViewportHeight: i32,
    /// The cached viewport aspect ratio (`m_CachedViewportSize.y / .x`, i.e. device height / width),
    /// refreshed every frame from the graphics device. [`Convert3DCoords`](crate::ui::ui_manager::UIManager::Convert3DCoords)
    /// uses it -- not the width/height fields -- to aspect-correct world-to-screen.
    pub m_CachedViewportRatio: f32,
    _field_14a4: [u8; 4],
}
fn _UIManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x14A8], UIManager>([0u8; 0x14A8]);
    }
    unreachable!()
}
impl UIManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417317920usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl UIManager {
    pub fn vftable(&self) -> *const crate::ui::ui_manager::UIManagerVftable {
        self.vftable as *const crate::ui::ui_manager::UIManagerVftable
    }
    pub const RenderImpl_ADDRESS: usize = 0x141007B70;
    /// The UI render: takes [`m_DeferredRenderLock`](crate::ui::ui_manager::UIManager::m_DeferredRenderLock), checks the
    /// three render gates, retargets the Scaleform render-thread ids, drains the thread command
    /// queue, binds the display render target, and draws the movie's latest captured display tree
    /// (`GetDisplayHandle` -> `RTHandle::NextCapture` -> `HAL::Draw`) within a
    /// `BeginFrame`/`BeginScene` pair. Runs on a CPU-fragment worker (kicked by `StartRender`,
    /// joined by `SyncRender`).
    pub unsafe fn RenderImpl(
        &mut self,
        context: *mut crate::graphics_engine::graphics_engine::HContext_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *mut crate::graphics_engine::graphics_engine::HContext_t,
            ) = ::std::mem::transmute(Self::RenderImpl_ADDRESS);
            f(self as *mut Self as _, context)
        }
    }
    pub const InitPlatformRT_ADDRESS: usize = 0x140F696E0;
    /// Binds the UI render target: builds a [`RenderTargetData`](crate::ui::ui_manager::RenderTargetData) from the engine surface's
    /// render-target and depth-stencil views via [`RenderTargetData::UpdateData`](crate::ui::ui_manager::RenderTargetData::UpdateData). Called at startup
    /// and on every device or resolution reset; `a2` carries the target side length (the buffer is
    /// square: width = height = a2). This only creates the render buffer -- the Scaleform movie's
    /// viewport is set separately by [`SetMovieViewport`](crate::ui::ui_manager::UIManager::SetMovieViewport).
    pub unsafe fn InitPlatformRT(&mut self, a2: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, a2: i32) = ::std::mem::transmute(
                Self::InitPlatformRT_ADDRESS,
            );
            f(self as *mut Self as _, a2)
        }
    }
    pub const RestoreAfterResetImpl_ADDRESS: usize = 0x140FA9C70;
    /// Re-runs [`InitPlatformRT`](crate::ui::ui_manager::UIManager::InitPlatformRT) after a device or resolution reset. Also
    /// recomputes the movie size, sets the movie viewport, and recomputes the safe area. Gated by an
    /// internal reset counter: it does nothing unless the counter is at 1.
    pub unsafe fn RestoreAfterResetImpl(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RestoreAfterResetImpl_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const ComputeMovieSizeOnViewSize_ADDRESS: usize = 0x140F46830;
    /// Recomputes the movie render rectangle ([`m_MovieScaleWidth`](crate::ui::ui_manager::UIManager::m_MovieScaleWidth) /
    /// [`m_MovieScaleHeight`](crate::ui::ui_manager::UIManager::m_MovieScaleHeight)). It first refreshes the cached stage and
    /// viewport sizes from the live device resolution (via an internal `UpdateCachedValues`), then
    /// sizes the movie rectangle from them, so the rectangle always reflects the device aspect.
    pub unsafe fn ComputeMovieSizeOnViewSize(&mut self, a2: bool, a3: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, a2: bool, a3: bool) = ::std::mem::transmute(
                Self::ComputeMovieSizeOnViewSize_ADDRESS,
            );
            f(self as *mut Self as _, a2, a3)
        }
    }
    pub const SetMovieViewport_ADDRESS: usize = 0x140F1B260;
    /// Sets the Scaleform movie's viewport to `width` x `height`, centering the movie (of size
    /// [`m_MovieScaleWidth`](crate::ui::ui_manager::UIManager::m_MovieScaleWidth) x
    /// [`m_MovieScaleHeight`](crate::ui::ui_manager::UIManager::m_MovieScaleHeight)) within it at offset
    /// `((width - m_MovieScaleWidth) / 2, (height - m_MovieScaleHeight) / 2)`. This is the viewport the
    /// Scaleform HAL renders into.
    pub unsafe fn SetMovieViewport(&mut self, width: i32, height: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, width: i32, height: i32) = ::std::mem::transmute(
                Self::SetMovieViewport_ADDRESS,
            );
            f(self as *mut Self as _, width, height)
        }
    }
    pub const ComputeSafeArea_ADDRESS: usize = 0x140F89B30;
    /// Computes [`m_SafeArea`](crate::ui::ui_manager::UIManager::m_SafeArea) and pushes it to every UI. The safe area is the
    /// stage inset by half of [`m_SafeAreaPercentage`](crate::ui::ui_manager::UIManager::m_SafeAreaPercentage) of its size on
    /// each side, then widened on the axis where the viewport (from
    /// [`m_CachedViewportWidth`](crate::ui::ui_manager::UIManager::m_CachedViewportWidth) and
    /// [`m_CachedViewportHeight`](crate::ui::ui_manager::UIManager::m_CachedViewportHeight)) is relatively longer than the
    /// stage, by half the extra extent the viewport covers at the stage's scale. So with a viewport
    /// taller than the stage's aspect, the safe area reaches above and below the stage into the
    /// visible frame. It reads the cached viewport size, not the movie viewport, so the two can
    /// differ.
    pub unsafe fn ComputeSafeArea(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ComputeSafeArea_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const ComputeGameViewArea_ADDRESS: usize = 0x140F89CB0;
    /// Computes the game view area from the current movie viewport.
    pub unsafe fn ComputeGameViewArea(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ComputeGameViewArea_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const Convert3DCoords_ADDRESS: usize = 0x140F69A70;
    /// World-to-screen: projects `world` through `vp`, divides by w, aspect-corrects, and maps NDC to
    /// pixels. Returns false when the point is behind the camera.
    pub unsafe fn Convert3DCoords(
        &self,
        world: *const crate::types::math::Vector3,
        out_x: *mut f32,
        out_y: *mut f32,
        vp: *const crate::types::math::Matrix4,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                world: *const crate::types::math::Vector3,
                out_x: *mut f32,
                out_y: *mut f32,
                vp: *const crate::types::math::Matrix4,
            ) -> bool = ::std::mem::transmute(Self::Convert3DCoords_ADDRESS);
            f(self as *const Self as _, world, out_x, out_y, vp)
        }
    }
    pub const Convert3DCoordsDefault_ADDRESS: usize = 0x140F899A0;
    /// The default-VP world-to-screen wrapper `CHUDUI::UpdateGrappleReticle` uses for the grapple
    /// reticle (its only callers): fetches the render camera's view-projection internally and
    /// forwards to [`Convert3DCoords`](crate::ui::ui_manager::UIManager::Convert3DCoords).
    pub unsafe fn Convert3DCoordsDefault(
        &mut self,
        world: *const crate::types::math::Vector3,
        out_x: *mut f32,
        out_y: *mut f32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                world: *const crate::types::math::Vector3,
                out_x: *mut f32,
                out_y: *mut f32,
            ) -> bool = ::std::mem::transmute(Self::Convert3DCoordsDefault_ADDRESS);
            f(self as *mut Self as _, world, out_x, out_y)
        }
    }
    pub const Get2DInfo_ADDRESS: usize = 0x140F69CB0;
    /// Marker placement: projects `world` with [`Convert3DCoords`](crate::ui::ui_manager::UIManager::Convert3DCoords) through
    /// `vp` and adds `offset` (stage pixels). A point in front of the camera inside
    /// [`m_SafeArea`](crate::ui::ui_manager::UIManager::m_SafeArea) shrunk by `margin` is
    /// [`SCREEN_POS_ONSCREEN`](crate::ui::ui_manager::ScreenPos::SCREEN_POS_ONSCREEN). Otherwise, without
    /// `clamp_to_screen` the result is
    /// [`SCREEN_POS_OFFSCREEN_NO_CLAMP_IN_FRONT_CAMERA`](crate::ui::ui_manager::ScreenPos::SCREEN_POS_OFFSCREEN_NO_CLAMP_IN_FRONT_CAMERA)
    /// or [`SCREEN_POS_OFFSCREEN_NO_CLAMP_BEHIND_CAMERA`](crate::ui::ui_manager::ScreenPos::SCREEN_POS_OFFSCREEN_NO_CLAMP_BEHIND_CAMERA);
    /// with it, a point outside the camera's horizontal field of view (`fov`, radians, with the
    /// `camera` transform giving the view direction) is first swung back toward the view by its
    /// yaw beyond the half-angle and re-projected, and the result is clamped to the shrunk safe
    /// area by [`ClampToScreen`](crate::ui::ui_manager::UIManager::ClampToScreen), which also sets the edge code.
    /// [`HUDUI::UpdatePOIs`](crate::ui::hud_ui::HUDUI::UpdatePOIs) places every point of interest
    /// through it.
    pub unsafe fn Get2DInfo(
        &self,
        world: *const crate::types::math::Vector3,
        vp: *const crate::types::math::Matrix4,
        camera: *const crate::types::math::Matrix4,
        fov: f32,
        out_x: *mut f32,
        out_y: *mut f32,
        out_pos: *mut crate::ui::ui_manager::ScreenPos,
        margin: f32,
        clamp_to_screen: bool,
        offset: crate::types::math::Vector2,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                world: *const crate::types::math::Vector3,
                vp: *const crate::types::math::Matrix4,
                camera: *const crate::types::math::Matrix4,
                fov: f32,
                out_x: *mut f32,
                out_y: *mut f32,
                out_pos: *mut crate::ui::ui_manager::ScreenPos,
                margin: f32,
                clamp_to_screen: bool,
                offset: crate::types::math::Vector2,
            ) = ::std::mem::transmute(Self::Get2DInfo_ADDRESS);
            f(
                self as *const Self as _,
                world,
                vp,
                camera,
                fov,
                out_x,
                out_y,
                out_pos,
                margin,
                clamp_to_screen,
                offset,
            )
        }
    }
    pub const ClampToScreen_ADDRESS: usize = 0x140F470A0;
    /// Clamps an off-screen point to the screen-rect edge or corner and sets the position code.
    /// Static.
    pub unsafe fn ClampToScreen(
        x: *mut f32,
        y: *mut f32,
        pos: *mut crate::ui::ui_manager::ScreenPos,
        min_x: f32,
        max_x: f32,
        min_y: f32,
        max_y: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                x: *mut f32,
                y: *mut f32,
                pos: *mut crate::ui::ui_manager::ScreenPos,
                min_x: f32,
                max_x: f32,
                min_y: f32,
                max_y: f32,
            ) = ::std::mem::transmute(Self::ClampToScreen_ADDRESS);
            f(x, y, pos, min_x, max_x, min_y, max_y)
        }
    }
    pub const StartRenderImpl_ADDRESS: usize = 0x140F1B030;
    /// Begins the UI render job, posting to the async render thread.
    pub unsafe fn StartRenderImpl(
        &mut self,
        context: *const crate::graphics_engine::graphics_engine::HContext_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *const crate::graphics_engine::graphics_engine::HContext_t,
            ) = ::std::mem::transmute(Self::StartRenderImpl_ADDRESS);
            f(self as *mut Self as _, context)
        }
    }
    pub const SyncRenderImpl_ADDRESS: usize = 0x140F1B0C0;
    /// A barrier that spin-waits until the async UI render job has finished.
    pub unsafe fn SyncRenderImpl(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::SyncRenderImpl_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const SubmitImpl_ADDRESS: usize = 0x140F1B0D0;
    /// The final commit: submits the render HAL's output under a graphics scoped lock.
    pub unsafe fn SubmitImpl(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::SubmitImpl_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const RenderStaticBackGroundImpl_ADDRESS: usize = 0x140F46C20;
    /// Draws the pause / menu static background.
    pub unsafe fn RenderStaticBackGroundImpl(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RenderStaticBackGroundImpl_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const RenderOffScreenTexturesImpl_ADDRESS: usize = 0x1410076C0;
    /// Renders Scaleform UI to offscreen textures for in-world screens. This is not the HUD.
    pub unsafe fn RenderOffScreenTexturesImpl(
        &mut self,
        ctx: *mut *mut crate::graphics_engine::graphics_engine::HContext_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut *mut crate::graphics_engine::graphics_engine::HContext_t,
            ) = ::std::mem::transmute(Self::RenderOffScreenTexturesImpl_ADDRESS);
            f(self as *mut Self as _, ctx)
        }
    }
    pub const IsUsingStaticBackGround_ADDRESS: usize = 0x140F1B4C0;
    /// Whether a static background is being shown.
    pub unsafe fn IsUsingStaticBackGround(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsUsingStaticBackGround_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const PreUpdate_ADDRESS: usize = 0x141049000;
    /// The per-frame UI input step: computes gamepad use, runs
    /// [`MousePointerVisibility`](crate::ui::ui_manager::UIManager::MousePointerVisibility), dispatches the
    /// highest-input-priority `CUIBase`'s input management, then calls
    /// [`SendMouseEvents`](crate::ui::ui_manager::UIManager::SendMouseEvents) and drains the queued key events. On a frame with
    /// no input-priority UI it skips both the dispatch and `SendMouseEvents`.
    pub unsafe fn PreUpdate(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::PreUpdate_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const SetMousePos_ADDRESS: usize = 0x140F46810;
    /// Stores a window-client-pixel mouse position into [`m_MouseX`](crate::ui::ui_manager::UIManager::m_MouseX) /
    /// [`m_MouseY`](crate::ui::ui_manager::UIManager::m_MouseY) and immediately runs
    /// [`SendMouseEvents`](crate::ui::ui_manager::UIManager::SendMouseEvents). Called by `WndProc` on `WM_MOUSEMOVE`
    /// with the `lParam` client coordinates.
    pub unsafe fn SetMousePos(&mut self, x: i32, y: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, x: i32, y: i32) = ::std::mem::transmute(
                Self::SetMousePos_ADDRESS,
            );
            f(self as *mut Self as _, x, y)
        }
    }
    pub const GetMousePos_ADDRESS: usize = 0x140F1B1F0;
    /// Reads back [`m_MouseX`](crate::ui::ui_manager::UIManager::m_MouseX) / [`m_MouseY`](crate::ui::ui_manager::UIManager::m_MouseY).
    pub unsafe fn GetMousePos(&self, x: *mut i32, y: *mut i32) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                x: *mut i32,
                y: *mut i32,
            ) = ::std::mem::transmute(Self::GetMousePos_ADDRESS);
            f(self as *const Self as _, x, y)
        }
    }
    pub const SendMouseEvents_ADDRESS: usize = 0x140F1BA60;
    /// Feeds the frame's mouse state to the Scaleform movie. Converts
    /// [`m_MouseX`](crate::ui::ui_manager::UIManager::m_MouseX) / [`m_MouseY`](crate::ui::ui_manager::UIManager::m_MouseY) to movie-viewport
    /// pixels by subtracting the centering offset
    /// `(m_CachedViewportSize - m_MovieScale size) / 2`, then sends
    /// [`MouseEvent`](crate::ui::scaleform::MouseEvent)s through
    /// [`MovieImpl::HandleEvent`](crate::ui::scaleform::MovieImpl::HandleEvent): a move event only when
    /// the DirectInput mouse reported a non-zero x or y delta this frame (also repositioning the
    /// overlay cursor sprite via `COverlayUI::SetMouseCursorPosition`), a wheel event from the z
    /// delta, and down/up events from the steering action map's
    /// [`MOUSE1`](crate::input::input_action_map::Action::MOUSE1) /
    /// [`MOUSE2`](crate::input::input_action_map::Action::MOUSE2) effector states
    /// ([`Clicked`](crate::input::input_action_map::EffectorState::Clicked) for the pressed edge,
    /// [`Released`](crate::input::input_action_map::EffectorState::Released) for the released edge). When
    /// [`m_IsUsingGamepad`](crate::ui::ui_manager::UIManager::m_IsUsingGamepad) is set it instead parks the mouse at
    /// [`GAMEPAD_PARKED_MOUSE_POSITION`](crate::ui::ui_manager::UIManager::GAMEPAD_PARKED_MOUSE_POSITION) on both axes. Returns false without the movie, the steering, or a mouse device.
    pub unsafe fn SendMouseEvents(&mut self, steering: *mut ::std::ffi::c_void) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                steering: *mut ::std::ffi::c_void,
            ) -> bool = ::std::mem::transmute(Self::SendMouseEvents_ADDRESS);
            f(self as *mut Self as _, steering)
        }
    }
    pub const GetMovieSpaceMouseCursor_ADDRESS: usize = 0x140F1BA30;
    /// Maps a movie-viewport-pixel mouse position into movie stage coordinates:
    /// `out = (pos - m_MouseDelta) * m_MouseScaleFac` per axis (see
    /// [`m_MouseDeltaX`](crate::ui::ui_manager::UIManager::m_MouseDeltaX)).
    pub unsafe fn GetMovieSpaceMouseCursor(
        &self,
        viewport_x: f32,
        viewport_y: f32,
        out: *mut crate::types::math::Vector2,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                viewport_x: f32,
                viewport_y: f32,
                out: *mut crate::types::math::Vector2,
            ) = ::std::mem::transmute(Self::GetMovieSpaceMouseCursor_ADDRESS);
            f(self as *const Self as _, viewport_x, viewport_y, out)
        }
    }
    pub const MousePointerVisibility_ADDRESS: usize = 0x140F46E60;
    /// Shows or hides the overlay mouse cursor from the frame's UI state: visible only when the
    /// overlay UI is active, no gamepad is in use, and no cursor-hiding HUD state or message box
    /// applies (via `COverlayUI::ShowMouseCursor` / `HideMouseCursor`).
    pub unsafe fn MousePointerVisibility(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::MousePointerVisibility_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const UpdateCachedValues_ADDRESS: usize = 0x140F1AEA0;
    /// Refreshes [`m_CachedStageWidth`](crate::ui::ui_manager::UIManager::m_CachedStageWidth) /
    /// [`m_CachedStageHeight`](crate::ui::ui_manager::UIManager::m_CachedStageHeight) from the loaded movie and
    /// [`m_CachedViewportWidth`](crate::ui::ui_manager::UIManager::m_CachedViewportWidth) /
    /// [`m_CachedViewportHeight`](crate::ui::ui_manager::UIManager::m_CachedViewportHeight) /
    /// [`m_CachedViewportRatio`](crate::ui::ui_manager::UIManager::m_CachedViewportRatio) from the graphics device.
    /// First step of [`ComputeMovieSizeOnViewSize`](crate::ui::ui_manager::UIManager::ComputeMovieSizeOnViewSize).
    pub unsafe fn UpdateCachedValues(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdateCachedValues_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    /// Returns the UI render context.
    pub unsafe fn GetUIContext(&mut self) -> *mut ::std::ffi::c_void {
        unsafe {
            let f = (&raw const (*self.vftable()).GetUIContext).read();
            f(self as *mut Self as _)
        }
    }
    /// See [`StartRenderImpl`](crate::ui::ui_manager::UIManager::StartRenderImpl).
    pub unsafe fn StartRender(
        &mut self,
        context: *const crate::graphics_engine::graphics_engine::HContext_t,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).StartRender).read();
            f(self as *mut Self as _, context)
        }
    }
    /// See [`SyncRenderImpl`](crate::ui::ui_manager::UIManager::SyncRenderImpl).
    pub unsafe fn SyncRender(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).SyncRender).read();
            f(self as *mut Self as _)
        }
    }
    /// See [`SubmitImpl`](crate::ui::ui_manager::UIManager::SubmitImpl).
    pub unsafe fn Submit(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).Submit).read();
            f(self as *mut Self as _)
        }
    }
    /// See [`RenderImpl`](crate::ui::ui_manager::UIManager::RenderImpl).
    pub unsafe fn Render(
        &mut self,
        context: *mut crate::graphics_engine::graphics_engine::HContext_t,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).Render).read();
            f(self as *mut Self as _, context)
        }
    }
    /// See [`RenderOffScreenTexturesImpl`](crate::ui::ui_manager::UIManager::RenderOffScreenTexturesImpl).
    pub unsafe fn RenderOffScreenTextures(
        &mut self,
        ctx: *mut *mut crate::graphics_engine::graphics_engine::HContext_t,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).RenderOffScreenTextures).read();
            f(self as *mut Self as _, ctx)
        }
    }
    /// See [`RenderStaticBackGroundImpl`](crate::ui::ui_manager::UIManager::RenderStaticBackGroundImpl).
    pub unsafe fn RenderStaticBackGround(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).RenderStaticBackGround).read();
            f(self as *mut Self as _)
        }
    }
    /// Releases device-dependent UI resources before a device reset.
    pub unsafe fn PrepareForReset(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).PrepareForReset).read();
            f(self as *mut Self as _)
        }
    }
    /// See [`RestoreAfterResetImpl`](crate::ui::ui_manager::UIManager::RestoreAfterResetImpl).
    pub unsafe fn RestoreAfterReset(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).RestoreAfterReset).read();
            f(self as *mut Self as _)
        }
    }
    /// Shows the UI.
    pub unsafe fn Show(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).Show).read();
            f(self as *mut Self as _)
        }
    }
    /// Hides the UI.
    pub unsafe fn Hide(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).Hide).read();
            f(self as *mut Self as _)
        }
    }
}
impl UIManager {
    /// The x and y at which [`SendMouseEvents`](crate::ui::ui_manager::UIManager::SendMouseEvents) places the Scaleform mouse
    /// while [`m_IsUsingGamepad`](crate::ui::ui_manager::UIManager::m_IsUsingGamepad) is set.
    pub const GAMEPAD_PARKED_MOUSE_POSITION: f32 = -1000.0;
}
impl std::convert::AsRef<UIManager> for UIManager {
    fn as_ref(&self) -> &UIManager {
        self
    }
}
impl std::convert::AsMut<UIManager> for UIManager {
    fn as_mut(&mut self) -> &mut UIManager {
        self
    }
}
#[repr(C, align(8))]
pub struct UIManagerVftable {
    /// Returns the UI render context.
    pub GetUIContext: unsafe extern "system" fn(
        this: *mut crate::ui::ui_manager::UIManager,
    ) -> *mut ::std::ffi::c_void,
    /// See [`StartRenderImpl`](crate::ui::ui_manager::UIManager::StartRenderImpl).
    pub StartRender: unsafe extern "system" fn(
        this: *mut crate::ui::ui_manager::UIManager,
        context: *const crate::graphics_engine::graphics_engine::HContext_t,
    ),
    /// See [`SyncRenderImpl`](crate::ui::ui_manager::UIManager::SyncRenderImpl).
    pub SyncRender: unsafe extern "system" fn(
        this: *mut crate::ui::ui_manager::UIManager,
    ),
    /// See [`SubmitImpl`](crate::ui::ui_manager::UIManager::SubmitImpl).
    pub Submit: unsafe extern "system" fn(this: *mut crate::ui::ui_manager::UIManager),
    /// See [`RenderImpl`](crate::ui::ui_manager::UIManager::RenderImpl).
    pub Render: unsafe extern "system" fn(
        this: *mut crate::ui::ui_manager::UIManager,
        context: *mut crate::graphics_engine::graphics_engine::HContext_t,
    ),
    /// See [`RenderOffScreenTexturesImpl`](crate::ui::ui_manager::UIManager::RenderOffScreenTexturesImpl).
    pub RenderOffScreenTextures: unsafe extern "system" fn(
        this: *mut crate::ui::ui_manager::UIManager,
        ctx: *mut *mut crate::graphics_engine::graphics_engine::HContext_t,
    ),
    /// See [`RenderStaticBackGroundImpl`](crate::ui::ui_manager::UIManager::RenderStaticBackGroundImpl).
    pub RenderStaticBackGround: unsafe extern "system" fn(
        this: *mut crate::ui::ui_manager::UIManager,
    ),
    /// Releases device-dependent UI resources before a device reset.
    pub PrepareForReset: unsafe extern "system" fn(
        this: *mut crate::ui::ui_manager::UIManager,
    ),
    /// See [`RestoreAfterResetImpl`](crate::ui::ui_manager::UIManager::RestoreAfterResetImpl).
    pub RestoreAfterReset: unsafe extern "system" fn(
        this: *mut crate::ui::ui_manager::UIManager,
    ),
    /// Shows the UI.
    pub Show: unsafe extern "system" fn(this: *mut crate::ui::ui_manager::UIManager),
    /// Hides the UI.
    pub Hide: unsafe extern "system" fn(this: *mut crate::ui::ui_manager::UIManager),
}
fn _UIManagerVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x58], UIManagerVftable>([0u8; 0x58]);
    }
    unreachable!()
}
impl UIManagerVftable {}
impl std::convert::AsRef<UIManagerVftable> for UIManagerVftable {
    fn as_ref(&self) -> &UIManagerVftable {
        self
    }
}
impl std::convert::AsMut<UIManagerVftable> for UIManagerVftable {
    fn as_mut(&mut self) -> &mut UIManagerVftable {
        self
    }
}
#[repr(C, align(8))]
/// The Scaleform `Render::D3D1x::TextureManager` behind the UI (only the render-thread id is
/// modeled).
pub struct UITextureManager {
    _field_0: [u8; 72],
    /// The render thread id; `CUIManager::Render` sets it to the calling thread each call.
    pub RenderThreadId: u32,
    _field_4c: [u8; 4],
}
fn _UITextureManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x50], UITextureManager>([0u8; 0x50]);
    }
    unreachable!()
}
impl UITextureManager {}
impl std::convert::AsRef<UITextureManager> for UITextureManager {
    fn as_ref(&self) -> &UITextureManager {
        self
    }
}
impl std::convert::AsMut<UITextureManager> for UITextureManager {
    fn as_mut(&mut self) -> &mut UITextureManager {
        self
    }
}
#[repr(C, align(8))]
/// The `CUiThreadCommandQueue`: commands queued for execution on the UI render worker.
pub struct UiThreadCommandQueue {
    _field_0: [u8; 152],
    /// The thread id the queue's render interfaces report; `CUIManager::Render` sets it to the
    /// calling thread before executing.
    pub m_RenderThreadId: u32,
    _field_9c: [u8; 4],
}
fn _UiThreadCommandQueue_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xA0], UiThreadCommandQueue>([0u8; 0xA0]);
    }
    unreachable!()
}
impl UiThreadCommandQueue {
    pub const Execute_ADDRESS: usize = 0x140FFAD30;
    /// Executes the queued render-thread commands. UI render worker only.
    pub unsafe fn Execute(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Execute_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<UiThreadCommandQueue> for UiThreadCommandQueue {
    fn as_ref(&self) -> &UiThreadCommandQueue {
        self
    }
}
impl std::convert::AsMut<UiThreadCommandQueue> for UiThreadCommandQueue {
    fn as_mut(&mut self) -> &mut UiThreadCommandQueue {
        self
    }
}
pub const GetIUIManager_ADDRESS: usize = 0x1400995A0;
/// Returns the single [`UIManager`](crate::ui::ui_manager::UIManager) instance.
pub unsafe fn GetIUIManager() -> *mut crate::ui::ui_manager::UIManager {
    unsafe {
        let f: unsafe extern "system" fn() -> *mut crate::ui::ui_manager::UIManager = ::std::mem::transmute(
            GetIUIManager_ADDRESS,
        );
        f()
    }
}
