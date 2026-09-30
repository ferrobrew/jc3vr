#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A node in the display tree returned by [`GetDisplayObjectsTree`](crate::ui::scaleform::Movie::GetDisplayObjectsTree). Inherits from
/// [`RefCountImpl`](crate::ui::scaleform::RefCountImpl) and embeds a `Scaleform::String` for the name and a
/// `Scaleform::ArrayDataBase<Ptr<AmpMovieObjectDesc>>` for children.
///
/// Layout verified from `DisplayObjContainer::GetChildDescTree`.
pub struct AmpMovieObjectDesc {
    pub ref_count_impl: crate::ui::scaleform::RefCountImpl,
    /// The clip's instance name, or `"Unnamed"`: a `Scaleform::String`, i.e. a pointer to a
    /// [`StringDataDesc`](crate::ui::scaleform::StringDataDesc) with heap flags packed into the pointer's low bits (mask off
    /// [`HEAP_FLAGS_MASK`](crate::ui::scaleform::StringDataDesc::HEAP_FLAGS_MASK)).
    pub name: *const u8,
    /// Pointer to the child array. Each element is a `Ptr<AmpMovieObjectDesc>` (a raw pointer
    /// to another [`AmpMovieObjectDesc`](crate::ui::scaleform::AmpMovieObjectDesc)).
    pub children: *mut *mut crate::ui::scaleform::AmpMovieObjectDesc,
    /// Number of children in [`children`](crate::ui::scaleform::AmpMovieObjectDesc::children).
    pub child_count: u64,
    /// Allocated capacity of [`children`](crate::ui::scaleform::AmpMovieObjectDesc::children).
    pub child_capacity: u64,
}
fn _AmpMovieObjectDesc_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], AmpMovieObjectDesc>([0u8; 0x30]);
    }
    unreachable!()
}
impl AmpMovieObjectDesc {
    pub fn vftable(&self) -> *const crate::ui::scaleform::RefCountImplVftable {
        self.ref_count_impl.vftable() as *const crate::ui::scaleform::RefCountImplVftable
    }
    /// Increments the reference count atomically.
    pub unsafe fn AddRef(&self) {
        unsafe { self.ref_count_impl.AddRef() }
    }
    /// Decrements the reference count atomically. When it reaches zero, calls the virtual
    /// [`destructor`](crate::ui::scaleform::AmpMovieObjectDesc::destructor).
    pub unsafe fn Release(&self) {
        unsafe { self.ref_count_impl.Release() }
    }
    /// Virtual destructor. Called by `Release` when the refcount reaches zero.
    pub unsafe fn destructor(&mut self, flags: u32) {
        unsafe {
            let f = (&raw const (*self.vftable()).destructor).read();
            f(self as *mut Self as _, flags)
        }
    }
}
impl std::convert::AsRef<crate::ui::scaleform::RefCountImpl> for AmpMovieObjectDesc {
    fn as_ref(&self) -> &crate::ui::scaleform::RefCountImpl {
        &self.ref_count_impl
    }
}
impl std::convert::AsMut<crate::ui::scaleform::RefCountImpl> for AmpMovieObjectDesc {
    fn as_mut(&mut self) -> &mut crate::ui::scaleform::RefCountImpl {
        &mut self.ref_count_impl
    }
}
impl std::convert::AsRef<AmpMovieObjectDesc> for AmpMovieObjectDesc {
    fn as_ref(&self) -> &AmpMovieObjectDesc {
        self
    }
}
impl std::convert::AsMut<AmpMovieObjectDesc> for AmpMovieObjectDesc {
    fn as_mut(&mut self) -> &mut AmpMovieObjectDesc {
        self
    }
}
#[repr(C, align(8))]
/// A pinned `DisplayHandle<TreeRoot>` as returned by
/// [`GetDisplayHandle`](crate::ui::scaleform::MovieImpl::GetDisplayHandle): one refcounted `HandleData` pointer.
pub struct DisplayHandle {
    /// The refcounted `RTHandle::HandleData` (starts with [`RefCountImpl`](crate::ui::scaleform::RefCountImpl), so
    /// [`RefCountImpl::AddRef`](crate::ui::scaleform::RefCountImpl::AddRef) pins a copied handle).
    pub pData: *mut crate::ui::scaleform::RefCountImpl,
}
fn _DisplayHandle_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], DisplayHandle>([0u8; 0x8]);
    }
    unreachable!()
}
impl DisplayHandle {}
impl std::convert::AsRef<DisplayHandle> for DisplayHandle {
    fn as_ref(&self) -> &DisplayHandle {
        self
    }
}
impl std::convert::AsMut<DisplayHandle> for DisplayHandle {
    fn as_mut(&mut self) -> &mut DisplayHandle {
        self
    }
}
#[derive(Default)]
#[repr(C, align(16))]
/// A display object's presentation state, read and written through
/// [`ValueObjectInterface::GetDisplayInfo`](crate::ui::scaleform::ValueObjectInterface::GetDisplayInfo) / [`SetDisplayInfo`](crate::ui::scaleform::ValueObjectInterface::SetDisplayInfo).
/// [`VarsSet`](crate::ui::scaleform::DisplayInfo::VarsSet) selects which fields a write applies.
pub struct DisplayInfo {
    pub X: f64,
    pub Y: f64,
    pub Rotation: f64,
    pub XScale: f64,
    pub YScale: f64,
    pub Alpha: f64,
    pub Z: f64,
    pub XRotation: f64,
    pub YRotation: f64,
    pub ZScale: f64,
    pub FOV: f64,
    _field_58: [u8; 8],
    /// `Render::Matrix3x4<float>` (3D view matrix).
    pub ViewMatrix3D: [f32; 12],
    /// `Render::Matrix4x4<float>` (3D projection matrix).
    pub ProjectionMatrix3D: [f32; 16],
    /// `Render::EdgeAAMode`.
    pub EdgeAAMode: u32,
    /// Which fields a [`SetDisplayInfo`](crate::ui::scaleform::ValueObjectInterface::SetDisplayInfo) call applies
    /// (`V_*` bits, such as [`V_VISIBLE`](crate::ui::scaleform::DisplayInfo::V_VISIBLE)).
    pub VarsSet: u16,
    pub Visible: bool,
    _field_d7: [u8; 9],
}
fn _DisplayInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xE0], DisplayInfo>([0u8; 0xE0]);
    }
    unreachable!()
}
impl DisplayInfo {}
impl DisplayInfo {
    /// The [`VarsSet`](crate::ui::scaleform::DisplayInfo::VarsSet) bit selecting [`Visible`](crate::ui::scaleform::DisplayInfo::Visible).
    pub const V_VISIBLE: u32 = 64;
}
impl std::convert::AsRef<DisplayInfo> for DisplayInfo {
    fn as_ref(&self) -> &DisplayInfo {
        self
    }
}
impl std::convert::AsMut<DisplayInfo> for DisplayInfo {
    fn as_mut(&mut self) -> &mut DisplayInfo {
        self
    }
}
#[repr(C, align(1))]
/// A Scaleform `GFx::DisplayObjectBase`: the display-side object behind a display-list entry.
/// Reached from a managed display-object [`Value`](crate::ui::scaleform::Value)'s [`Object::m_DisplayObject`](crate::ui::scaleform::Object::m_DisplayObject) (guarded by the
/// traits check `GetDisplayInfo` performs through [`Object::m_Traits`](crate::ui::scaleform::Object::m_Traits)).
pub struct DisplayObjectBase {
    _field_0: [u8; 8],
}
fn _DisplayObjectBase_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], DisplayObjectBase>([0u8; 0x8]);
    }
    unreachable!()
}
impl DisplayObjectBase {
    pub const GetRenderNode_ADDRESS: usize = 0x1419EB6F0;
    /// The object's render-tree node, created lazily. Capture (game update) thread only.
    pub unsafe fn GetRenderNode(&self) -> *mut crate::ui::scaleform::TreeNode {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
            ) -> *mut crate::ui::scaleform::TreeNode = ::std::mem::transmute(
                Self::GetRenderNode_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<DisplayObjectBase> for DisplayObjectBase {
    fn as_ref(&self) -> &DisplayObjectBase {
        self
    }
}
impl std::convert::AsMut<DisplayObjectBase> for DisplayObjectBase {
    fn as_mut(&mut self) -> &mut DisplayObjectBase {
        self
    }
}
#[repr(C, align(8))]
/// `GFx::Loader`: the Scaleform movie loader.
pub struct Loader {}
impl Loader {
    pub const CreateMovie_ADDRESS: usize = 0x141976DD0;
    /// Loads (or fetches from the resource library) the movie definition at `filename` with the
    /// given load flags.
    pub unsafe fn CreateMovie(
        &mut self,
        filename: *const u8,
        load_constants: u32,
        memory_arena: u64,
    ) -> *mut ::std::ffi::c_void {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                filename: *const u8,
                load_constants: u32,
                memory_arena: u64,
            ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(
                Self::CreateMovie_ADDRESS,
            );
            f(self as *mut Self as _, filename, load_constants, memory_arena)
        }
    }
}
impl Loader {
    /// The load flag that skips import binding: `MovieBindProcess::BindNextFrame` tests it before
    /// resolving a movie's imported symbol libraries (`LoadDisableImports`).
    pub const LOAD_DISABLE_IMPORTS: u32 = 1048576;
}
impl std::convert::AsRef<Loader> for Loader {
    fn as_ref(&self) -> &Loader {
        self
    }
}
impl std::convert::AsMut<Loader> for Loader {
    fn as_mut(&mut self) -> &mut Loader {
        self
    }
}
#[repr(C, align(8))]
/// A Scaleform [`MemoryHeap`](crate::ui::scaleform::MemoryHeap). Opaque apart from its vtable, through which allocation goes.
pub struct MemoryHeap {
    vftable: *const crate::ui::scaleform::MemoryHeapVftable,
}
fn _MemoryHeap_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], MemoryHeap>([0u8; 0x8]);
    }
    unreachable!()
}
impl MemoryHeap {
    pub fn vftable(&self) -> *const crate::ui::scaleform::MemoryHeapVftable {
        self.vftable as *const crate::ui::scaleform::MemoryHeapVftable
    }
    /// Allocates `size` bytes from the heap.
    pub unsafe fn Alloc(&mut self, size: u64) -> *mut ::std::ffi::c_void {
        unsafe {
            let f = (&raw const (*self.vftable()).Alloc).read();
            f(self as *mut Self as _, size)
        }
    }
}
impl std::convert::AsRef<MemoryHeap> for MemoryHeap {
    fn as_ref(&self) -> &MemoryHeap {
        self
    }
}
impl std::convert::AsMut<MemoryHeap> for MemoryHeap {
    fn as_mut(&mut self) -> &mut MemoryHeap {
        self
    }
}
#[repr(C, align(8))]
pub struct MemoryHeapVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MemoryHeap),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MemoryHeap),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MemoryHeap),
    _vfunc_3: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MemoryHeap),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MemoryHeap),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MemoryHeap),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MemoryHeap),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MemoryHeap),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MemoryHeap),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MemoryHeap),
    /// Allocates `size` bytes from the heap.
    pub Alloc: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MemoryHeap,
        size: u64,
    ) -> *mut ::std::ffi::c_void,
}
fn _MemoryHeapVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x58], MemoryHeapVftable>([0u8; 0x58]);
    }
    unreachable!()
}
impl MemoryHeapVftable {}
impl std::convert::AsRef<MemoryHeapVftable> for MemoryHeapVftable {
    fn as_ref(&self) -> &MemoryHeapVftable {
        self
    }
}
impl std::convert::AsMut<MemoryHeapVftable> for MemoryHeapVftable {
    fn as_mut(&mut self) -> &mut MemoryHeapVftable {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// A `GFx::MouseEvent` (a `GFx::Event` with the mouse payload): the argument
/// `CUIManager::SendMouseEvents` builds for [`MovieImpl::HandleEvent`](crate::ui::scaleform::MovieImpl::HandleEvent). `x` and `y` are in
/// movie-viewport pixels: window-client pixels minus the centering offset
/// `(cached viewport size - movie rectangle size) / 2`, i.e. relative to the top-left corner of
/// the centered movie rectangle (see
/// [`UIManager::m_MovieScaleWidth`](crate::ui::ui_manager::UIManager::m_MovieScaleWidth)).
pub struct MouseEvent {
    /// The `GFx::Event::Type` tag (`TYPE_MOUSE_*`).
    pub Type: u32,
    /// `GFx::KeyModifiers::States` (ctrl/alt/shift bits); the engine always sends 0.
    pub Modifiers: u8,
    _field_5: [u8; 3],
    pub x: f32,
    pub y: f32,
    /// The wheel amount in Flash line units; the engine sends `lZ / 120 * 3` from the DirectInput
    /// mouse's z axis.
    pub ScrollDelta: f32,
    /// The button index for down/up events: [`BUTTON_LEFT`](crate::ui::scaleform::MouseEvent::BUTTON_LEFT) or
    /// [`BUTTON_RIGHT`](crate::ui::scaleform::MouseEvent::BUTTON_RIGHT).
    pub Button: u32,
    pub MouseIndex: u32,
}
fn _MouseEvent_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1C], MouseEvent>([0u8; 0x1C]);
    }
    unreachable!()
}
impl MouseEvent {}
impl MouseEvent {
    /// The [`Button`](crate::ui::scaleform::MouseEvent::Button) index of the left button.
    pub const BUTTON_LEFT: u32 = 0;
    /// The [`Button`](crate::ui::scaleform::MouseEvent::Button) index of the right button.
    pub const BUTTON_RIGHT: u32 = 1;
    /// The [`Type`](crate::ui::scaleform::MouseEvent::Type) of a button-press event.
    pub const TYPE_MOUSE_DOWN: u32 = 2;
    /// The [`Type`](crate::ui::scaleform::MouseEvent::Type) of a mouse-move event.
    pub const TYPE_MOUSE_MOVE: u32 = 1;
    /// The [`Type`](crate::ui::scaleform::MouseEvent::Type) of a button-release event.
    pub const TYPE_MOUSE_UP: u32 = 3;
    /// The [`Type`](crate::ui::scaleform::MouseEvent::Type) of a wheel event ([`ScrollDelta`](crate::ui::scaleform::MouseEvent::ScrollDelta)
    /// carries the amount).
    pub const TYPE_MOUSE_WHEEL: u32 = 4;
}
impl std::convert::AsRef<MouseEvent> for MouseEvent {
    fn as_ref(&self) -> &MouseEvent {
        self
    }
}
impl std::convert::AsMut<MouseEvent> for MouseEvent {
    fn as_mut(&mut self) -> &mut MouseEvent {
        self
    }
}
#[repr(C, align(8))]
/// A Scaleform `GFx::MouseState`: one tracked mouse's per-movie state
/// ([`MovieImpl::MouseStates`](crate::ui::scaleform::MovieImpl::MouseStates)). `MouseState::UpdateState` fills it while
/// [`MovieImpl::ProcessInput`](crate::ui::scaleform::MovieImpl::ProcessInput) drains the input-events queue during `Advance`: it copies
/// [`CurButtonsState`](crate::ui::scaleform::MouseState::CurButtonsState) into
/// [`PrevButtonsState`](crate::ui::scaleform::MouseState::PrevButtonsState), applies the entry's button/wheel payload,
/// sets or clears [`FLAG_MOUSE_MOVED`](crate::ui::scaleform::MouseState::FLAG_MOUSE_MOVED) by integer-comparing the
/// entry's position against [`LastPosition`](crate::ui::scaleform::MouseState::LastPosition), and stores the entry's
/// position. `MovieImpl::ProcessMouse` then hit-tests the display tree at that stored position
/// directly (`GetTopMostEntity` recurses with each display object's own local transforms) -- no
/// further viewport, view-scale, or flag-dependent transform runs between the queue and the hit
/// test.
pub struct MouseState {
    /// `WeakPtr<InteractiveObject>`: the entity under the cursor (the hover target), written
    /// during `ProcessInput` via `MouseState::SetTopmostEntity`.
    pub TopmostEntity: *mut ::std::ffi::c_void,
    /// `WeakPtr<InteractiveObject>`: the previous hover target.
    pub PrevTopmostEntity: *mut ::std::ffi::c_void,
    /// `WeakPtr<InteractiveObject>`: the entity that received the last button press.
    pub ActiveEntity: *mut ::std::ffi::c_void,
    /// `ArrayLH<WeakPtr<InteractiveObject>>` (`{data, size, capacity}`): the per-button press
    /// targets.
    pub MouseButtonDownEntities: [u64; 3],
    /// The current button bitmask (`MOUSE_BUTTON_*` bits), as last applied from the queue. [`MovieImpl::NotifyMouseState`](crate::ui::scaleform::MovieImpl::NotifyMouseState) diffs its `buttons` argument against this when
    /// queueing.
    pub CurButtonsState: u32,
    /// The button bitmask before the last processed queue entry.
    pub PrevButtonsState: u32,
    /// The mouse position in stage twips (`Render::Point<float>`): the authoritative position
    /// hit-testing uses, i.e. the queued output of the
    /// [`ViewportMatrix`](crate::ui::scaleform::MovieImpl::ViewportMatrix)-inverse transform once `Advance` has
    /// drained the queue. Divide by 20 for stage pixels. Between a
    /// [`NotifyMouseState`](crate::ui::scaleform::MovieImpl::NotifyMouseState) call and the next `Advance`, the new
    /// position exists only in the input-events queue; this field still holds the previous one.
    pub LastPosition: [f32; 2],
    /// The cursor type forced via `MovieImpl::ChangeMouseCursorType`, if any.
    pub PresetCursorType: u32,
    /// The current cursor type.
    pub CursorType: u32,
    /// The wheel delta of the last processed queue entry (zero when it carried no wheel bit).
    pub WheelDelta: i32,
    /// The `FLAG_*` state bits.
    pub Flags: u8,
    _field_4d: [u8; 3],
}
fn _MouseState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x50], MouseState>([0u8; 0x50]);
    }
    unreachable!()
}
impl MouseState {}
impl MouseState {
    /// [`Flags`](crate::ui::scaleform::MouseState::Flags) bit: this mouse has received input since the movie started.
    pub const FLAG_ACTIVATED: u8 = 16;
    /// [`Flags`](crate::ui::scaleform::MouseState::Flags) bit: the mouse was inside the active entity on the last
    /// test.
    pub const FLAG_MOUSE_INSIDE_ENTITY_LAST: u8 = 4;
    /// [`Flags`](crate::ui::scaleform::MouseState::Flags) bit: the position changed on the last processed queue entry
    /// (compared as truncated integers).
    pub const FLAG_MOUSE_MOVED: u8 = 8;
    /// [`Flags`](crate::ui::scaleform::MouseState::Flags) bit: the previous topmost entity was null.
    pub const FLAG_PREV_TOPMOST_ENTITY_WAS_NULL: u8 = 2;
    /// [`Flags`](crate::ui::scaleform::MouseState::Flags) bit: the topmost entity resolved to null on the last update.
    pub const FLAG_TOPMOST_ENTITY_IS_NULL: u8 = 1;
    /// The [`CurButtonsState`](crate::ui::scaleform::MouseState::CurButtonsState) bit for the left button
    /// (`MouseButton_Left`).
    pub const MOUSE_BUTTON_LEFT: u32 = 1;
    /// The [`CurButtonsState`](crate::ui::scaleform::MouseState::CurButtonsState) bit for the middle button
    /// (`MouseButton_Middle`).
    pub const MOUSE_BUTTON_MIDDLE: u32 = 4;
    /// The [`CurButtonsState`](crate::ui::scaleform::MouseState::CurButtonsState) bit for the right button
    /// (`MouseButton_Right`).
    pub const MOUSE_BUTTON_RIGHT: u32 = 2;
}
impl std::convert::AsRef<MouseState> for MouseState {
    fn as_ref(&self) -> &MouseState {
        self
    }
}
impl std::convert::AsMut<MouseState> for MouseState {
    fn as_mut(&mut self) -> &mut MouseState {
        self
    }
}
#[repr(C, align(8))]
/// The Scaleform AS3 `MovieRoot` (the `ASMovieRootBase` interface), which `CUIManager::m_Movie`
/// points at. The engine drives the movie through this interface's virtuals; the bound
/// [`SetVariable`](crate::ui::scaleform::Movie::SetVariable) / [`GetVariable`](crate::ui::scaleform::Movie::GetVariable) /
/// [`Invoke`](crate::ui::scaleform::Movie::Invoke) are its concrete implementations. The vtable is `MovieRoot`'s at
/// [`VFTABLE`](crate::ui::scaleform::Movie::VFTABLE).
pub struct Movie {
    vftable: *const crate::ui::scaleform::MovieVftable,
    _field_8: [u8; 8],
    /// The backing `GFx::MovieImpl` (`GetDisplayObjectsTree` reads `pMovieImpl->pMainMovie`
    /// through it).
    pub pMovieImpl: *mut crate::ui::scaleform::MovieImpl,
}
fn _Movie_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], Movie>([0u8; 0x18]);
    }
    unreachable!()
}
impl Movie {
    pub fn vftable(&self) -> *const crate::ui::scaleform::MovieVftable {
        self.vftable as *const crate::ui::scaleform::MovieVftable
    }
    pub const GetDisplayObjectsTreeImpl_ADDRESS: usize = 0x141BED530;
    /// The `AS3::MovieRoot` implementation of the
    /// [`GetDisplayObjectsTree`](crate::ui::scaleform::Movie::GetDisplayObjectsTree) virtual: allocates the root
    /// [`AmpMovieObjectDesc`](crate::ui::scaleform::AmpMovieObjectDesc) from `heap`, names it after the root movie clip, and fills its
    /// children through `DisplayObjContainer::GetChildDescTree`. Returns null without a root clip.
    pub unsafe fn GetDisplayObjectsTreeImpl(
        &self,
        heap: *mut crate::ui::scaleform::MemoryHeap,
    ) -> *mut crate::ui::scaleform::AmpMovieObjectDesc {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                heap: *mut crate::ui::scaleform::MemoryHeap,
            ) -> *mut crate::ui::scaleform::AmpMovieObjectDesc = ::std::mem::transmute(
                Self::GetDisplayObjectsTreeImpl_ADDRESS,
            );
            f(self as *const Self as _, heap)
        }
    }
    pub const SetVariable_ADDRESS: usize = 0x141C47FA0;
    /// Sets an AS3 variable by clip path. The path is a dot-separated string like
    /// `"MCI_hud.MCI_poi_stage._visible"`, resolved from the root timeline. `set_var_type` (one of
    /// [`SV_NORMAL`](crate::ui::scaleform::Movie::SV_NORMAL), [`SV_STICKY`](crate::ui::scaleform::Movie::SV_STICKY), and
    /// [`SV_PERMANENT`](crate::ui::scaleform::Movie::SV_PERMANENT)) controls persistence across clip re-creation. Call on
    /// the capture (game update) thread.
    pub unsafe fn SetVariable(
        &self,
        path: *const u8,
        value: *const crate::ui::scaleform::Value,
        set_var_type: u32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                path: *const u8,
                value: *const crate::ui::scaleform::Value,
                set_var_type: u32,
            ) -> bool = ::std::mem::transmute(Self::SetVariable_ADDRESS);
            f(self as *const Self as _, path, value, set_var_type)
        }
    }
    pub const GetVariable_ADDRESS: usize = 0x141C47DF0;
    /// Reads an AS3 variable by clip path into a [`Value`](crate::ui::scaleform::Value). Returns false if the path is not
    /// found. A returned managed value (display object, string) must be released through the
    /// movie's object interface; plain bool/int/number values need no cleanup.
    pub unsafe fn GetVariable(
        &self,
        value: *mut crate::ui::scaleform::Value,
        path: *const u8,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                value: *mut crate::ui::scaleform::Value,
                path: *const u8,
            ) -> bool = ::std::mem::transmute(Self::GetVariable_ADDRESS);
            f(self as *const Self as _, value, path)
        }
    }
    pub const Invoke_ADDRESS: usize = 0x141C49CD0;
    /// Invokes an AS3 method on the movie's root timeline by name. This is the call path used by
    /// the engine's `CUIBase::Invoke`. The `args` are a `GFx::Value` array; `result` receives the
    /// return value (or null). Note the parameter order: `result` comes *before* `args`, unlike
    /// the engine's `CUIBase::Invoke` wrapper (which also adds a timeout).
    pub unsafe fn Invoke(
        &self,
        method_name: *const u8,
        result: *mut ::std::ffi::c_void,
        args: *const ::std::ffi::c_void,
        num_args: u32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                method_name: *const u8,
                result: *mut ::std::ffi::c_void,
                args: *const ::std::ffi::c_void,
                num_args: u32,
            ) -> bool = ::std::mem::transmute(Self::Invoke_ADDRESS);
            f(self as *const Self as _, method_name, result, args, num_args)
        }
    }
    /// Recursively walks the runtime display tree from the root movie clip: returns a tree of
    /// [`AmpMovieObjectDesc`](crate::ui::scaleform::AmpMovieObjectDesc) nodes, each carrying the clip's instance name (or `"Unnamed"`)
    /// and a child array, calling `DisplayObject::GetName` on every child and recursing into
    /// containers. `heap` is the Scaleform `MemoryHeap*` used for the allocations (e.g.
    /// [`MovieImpl::pHeap`](crate::ui::scaleform::MovieImpl::pHeap)); the tree is freed by releasing the root node. The implementation
    /// is [`GetDisplayObjectsTreeImpl`](crate::ui::scaleform::Movie::GetDisplayObjectsTreeImpl). Call on the capture
    /// (game update) thread, where the display tree is stable.
    pub unsafe fn GetDisplayObjectsTree(
        &self,
        heap: *mut crate::ui::scaleform::MemoryHeap,
    ) -> *mut crate::ui::scaleform::AmpMovieObjectDesc {
        unsafe {
            let f = (&raw const (*self.vftable()).GetDisplayObjectsTree).read();
            f(self as *const Self as _, heap)
        }
    }
}
impl Movie {
    /// The [`SetVariable`](crate::ui::scaleform::Movie::SetVariable) `set_var_type` (`GFx::Movie::SetVarType`) for a
    /// plain assignment with no persistence across clip re-creation.
    pub const SV_NORMAL: u32 = 0;
    /// The `set_var_type` that keeps the value for the life of the movie.
    pub const SV_PERMANENT: u32 = 2;
    /// The `set_var_type` that re-applies the value when the clip is re-created.
    pub const SV_STICKY: u32 = 1;
    /// The `AS3::MovieRoot` vtable address (the `ASMovieRootBase` layout, which holds
    /// [`GetDisplayObjectsTree`](crate::ui::scaleform::Movie::GetDisplayObjectsTree) and the virtuals whose
    /// implementations are the bound SetVariable/GetVariable/Invoke).
    pub const VFTABLE: u64 = 5408691888;
}
impl std::convert::AsRef<Movie> for Movie {
    fn as_ref(&self) -> &Movie {
        self
    }
}
impl std::convert::AsMut<Movie> for Movie {
    fn as_mut(&mut self) -> &mut Movie {
        self
    }
}
#[repr(C, align(8))]
/// The `GFx::MovieImpl` behind [`UIManager::m_Movie`](crate::ui::ui_manager::UIManager::m_Movie): the
/// `GFx::Movie` instance. The rendering-side virtuals live on its own vtable; the AS3 side
/// (SetVariable, Invoke, the display tree) lives on [`pASMovieRoot`](crate::ui::scaleform::MovieImpl::pASMovieRoot).
pub struct MovieImpl {
    vftable: *const crate::ui::scaleform::MovieImplVftable,
    _field_8: [u8; 16],
    /// The AS3 `MovieRoot` (the [`Movie`](crate::ui::scaleform::Movie) interface): SetVariable, Invoke, and the display tree.
    pub pASMovieRoot: *mut crate::ui::scaleform::Movie,
    _field_20: [u8; 24],
    /// The `GFx::Value::ObjectInterface` for this movie: the dispatcher every `GFx::Value`
    /// display-object/member operation goes through.
    pub pObjectInterface: *mut ::std::ffi::c_void,
    /// The Scaleform [`MemoryHeap`](crate::ui::scaleform::MemoryHeap) the movie allocates from.
    pub pHeap: *mut crate::ui::scaleform::MemoryHeap,
    /// The main `MovieDefImpl`. [`UpdateViewport`](crate::ui::scaleform::MovieImpl::UpdateViewport) reads the stage
    /// frame rectangle (twips) through it; while null, the view scale and offset reset to
    /// identity.
    pub pMainMovieDef: *mut ::std::ffi::c_void,
    /// The root `DisplayObjContainer` (the main movie clip).
    pub pMainMovie: *mut ::std::ffi::c_void,
    _field_58: [u8; 48],
    /// The movie's render-tree root ([`TreeRoot`](crate::ui::scaleform::TreeRoot)): the entry `CUIManager::Render` draws via the
    /// display handle. The whole display tree's render nodes hang off it.
    pub pRenderRoot: *mut crate::ui::scaleform::TreeRoot,
    _field_90: [u8; 16],
    /// The movie's current [`Viewport`](crate::ui::scaleform::Viewport), as last copied in by
    /// [`SetViewportImpl`](crate::ui::scaleform::MovieImpl::SetViewportImpl). [`TreeRoot::SetViewport`](crate::ui::scaleform::TreeRoot::SetViewport) takes it
    /// directly (copying the `Render::Viewport` prefix).
    pub Viewport: crate::ui::scaleform::Viewport,
    /// The larger of `1 / ViewScaleX` and `1 / ViewScaleY`, recomputed by
    /// [`UpdateViewport`](crate::ui::scaleform::MovieImpl::UpdateViewport) (0.005 when a scale is zero).
    pub PixelScale: f32,
    /// The stage-pixel to view-rectangle-pixel scale on X, derived from
    /// [`ViewScaleMode`](crate::ui::scaleform::MovieImpl::ViewScaleMode) by [`UpdateViewport`](crate::ui::scaleform::MovieImpl::UpdateViewport)
    /// (for `SM_SHOW_ALL`/`SM_NO_BORDER`, the visible-frame to view-rectangle ratio).
    pub ViewScaleX: f32,
    /// See [`ViewScaleX`](crate::ui::scaleform::MovieImpl::ViewScaleX).
    pub ViewScaleY: f32,
    /// [`VisibleFrameRect`](crate::ui::scaleform::MovieImpl::VisibleFrameRect) `x1` in stage pixels (`x1 / 20`).
    pub ViewOffsetX: f32,
    /// [`VisibleFrameRect`](crate::ui::scaleform::MovieImpl::VisibleFrameRect) `y1` in stage pixels (`y1 / 20`).
    pub ViewOffsetY: f32,
    /// The `SM_*` stage scale mode. Constructed as [`SM_SHOW_ALL`](crate::ui::scaleform::MovieImpl::SM_SHOW_ALL); the
    /// engine never changes it on the main UI movie (`CRenderToTextureUI`/`CDynUI` set it on
    /// their own movies).
    pub ViewScaleMode: u32,
    /// The `Align_*` stage alignment used by the `SM_NO_SCALE` mode. Constructed as
    /// [`ALIGN_CENTER`](crate::ui::scaleform::MovieImpl::ALIGN_CENTER).
    pub ViewAlignment: u32,
    /// The stage region mapped onto the view rectangle, in twips (`x1, y1, x2, y2`), computed by
    /// [`UpdateViewport`](crate::ui::scaleform::MovieImpl::UpdateViewport) from the [`Viewport`](crate::ui::scaleform::MovieImpl::Viewport),
    /// the stage frame rectangle, and the [`ViewScaleMode`](crate::ui::scaleform::MovieImpl::ViewScaleMode). Under
    /// `SM_SHOW_ALL` with a view rectangle matching the stage aspect, this is the whole stage.
    pub VisibleFrameRect: [f32; 4],
    _field_100: [u8; 16],
    /// The stage-to-viewport matrix (`Matrix2x4<float>`, rows `[sx, shx, shy, tx]` and
    /// `[shx, sy, shy, ty]`; in practice `sx` at `[0]`, `tx` at `[3]`, `sy` at `[5]`, and `ty` at
    /// `[7]`) the movie sets on [`pRenderRoot`](crate::ui::scaleform::MovieImpl::pRenderRoot) via `TreeNode::SetMatrix`
    /// whenever the viewport changes. Built by
    /// [`ResetViewportMatrix`](crate::ui::scaleform::MovieImpl::ResetViewportMatrix) as `sx = Viewport.Width /
    /// (VisibleFrameRect.x2 - VisibleFrameRect.x1)`, `tx = -VisibleFrameRect.x1 * sx` (and
    /// likewise for Y), mapping stage twips to pixels relative to the view rectangle's top-left
    /// corner -- `Viewport.Left`/`Top` are *not* part of the matrix.
    ///
    /// This same matrix, inverted, is the entire mouse-to-stage transform:
    /// [`HandleEvent`](crate::ui::scaleform::MovieImpl::HandleEvent) and
    /// [`NotifyMouseState`](crate::ui::scaleform::MovieImpl::NotifyMouseState) run incoming positions through
    /// `TransformByInverse` before queueing, so mouse coordinates are expected in view-rectangle
    /// pixels (`stage_twips = (pos - t) / s` per axis).
    pub ViewportMatrix: [f32; 8],
    _field_130: [u8; 8416],
    /// The tracked mice ([`MouseState`](crate::ui::scaleform::MouseState) per index; the engine's main UI movie uses only index
    /// 0). The array ends exactly at [`MouseCursorCount`](crate::ui::scaleform::MovieImpl::MouseCursorCount).
    pub MouseStates: [crate::ui::scaleform::MouseState; 6],
    /// The number of mouse cursors the movie tracks (see
    /// [`SetMouseCursorCount`](crate::ui::scaleform::MovieImpl::SetMouseCursorCount)); mouse input with an index at or
    /// beyond it is dropped.
    pub MouseCursorCount: u32,
    _field_23f4: [u8; 12028],
    /// The movie's `FLAG_*` state bits (`MovieImpl::Flags`), among them
    /// [`FLAG_NEED_MOUSE_UPDATE`](crate::ui::scaleform::MovieImpl::FLAG_NEED_MOUSE_UPDATE).
    pub Flags: u32,
    _field_52f4: [u8; 92],
    /// The movie's embedded `Render::Context` (the snapshot pipeline: active/pending/displaying
    /// snapshots, the capture locks, and the once-a-frame consumption latch).
    pub RenderContext: crate::ui::scaleform::RenderContext,
}
fn _MovieImpl_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x5468], MovieImpl>([0u8; 0x5468]);
    }
    unreachable!()
}
impl MovieImpl {
    pub fn vftable(&self) -> *const crate::ui::scaleform::MovieImplVftable {
        self.vftable as *const crate::ui::scaleform::MovieImplVftable
    }
    pub const CreateString_ADDRESS: usize = 0x14197E4E0;
    /// Creates an AS3 string value in the movie (`GFx::Movie::CreateString`, forwarding to the
    /// AS3 movie root at [`pASMovieRoot`](crate::ui::scaleform::MovieImpl::pASMovieRoot)).
    pub unsafe fn CreateString(
        &mut self,
        value: *mut crate::ui::scaleform::Value,
        string: *const u8,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                value: *mut crate::ui::scaleform::Value,
                string: *const u8,
            ) = ::std::mem::transmute(Self::CreateString_ADDRESS);
            f(self as *mut Self as _, value, string)
        }
    }
    pub const CreateObject_ADDRESS: usize = 0x14197E500;
    /// Constructs an instance of the AS3 class `class_name` (`Object` when null) with `num_args`
    /// constructor arguments into `value` (`GFx::Movie::CreateObject`). The class is looked up in
    /// the application domain of the running AS3 code, or the VM's default domain when none is
    /// running, so every class a loaded movie defines is reachable by its qualified name. A
    /// constructor that throws is logged and leaves `value` undefined.
    pub unsafe fn CreateObject(
        &mut self,
        value: *mut crate::ui::scaleform::Value,
        class_name: *const u8,
        args: *const crate::ui::scaleform::Value,
        num_args: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                value: *mut crate::ui::scaleform::Value,
                class_name: *const u8,
                args: *const crate::ui::scaleform::Value,
                num_args: u32,
            ) = ::std::mem::transmute(Self::CreateObject_ADDRESS);
            f(self as *mut Self as _, value, class_name, args, num_args)
        }
    }
    pub const CreateArray_ADDRESS: usize = 0x14197E520;
    /// Creates an empty AS3 `Array` into `value` (`GFx::Movie::CreateArray`).
    pub unsafe fn CreateArray(&mut self, value: *mut crate::ui::scaleform::Value) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                value: *mut crate::ui::scaleform::Value,
            ) = ::std::mem::transmute(Self::CreateArray_ADDRESS);
            f(self as *mut Self as _, value)
        }
    }
    pub const CreateFunction_ADDRESS: usize = 0x14197E530;
    /// Wraps a native `GFx::FunctionHandler` in an AS3 function object into `value`
    /// (`GFx::Movie::CreateFunction`). The AS3 movie root adds a reference to `handler` and keeps
    /// `user_data` beside it for the handler's calls.
    pub unsafe fn CreateFunction(
        &mut self,
        value: *mut crate::ui::scaleform::Value,
        handler: *mut ::std::ffi::c_void,
        user_data: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                value: *mut crate::ui::scaleform::Value,
                handler: *mut ::std::ffi::c_void,
                user_data: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::CreateFunction_ADDRESS);
            f(self as *mut Self as _, value, handler, user_data)
        }
    }
    pub const SetExternalInterfaceRetVal_ADDRESS: usize = 0x14197E550;
    /// Sets the return value of the ExternalInterface call being handled
    /// (`GFx::Movie::SetExternalInterfaceRetVal`).
    pub unsafe fn SetExternalInterfaceRetVal(
        &mut self,
        value: *const crate::ui::scaleform::Value,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                value: *const crate::ui::scaleform::Value,
            ) = ::std::mem::transmute(Self::SetExternalInterfaceRetVal_ADDRESS);
            f(self as *mut Self as _, value)
        }
    }
    pub const CaptureImpl_ADDRESS: usize = 0x14198B7D0;
    /// The concrete implementation behind the [`Capture`](crate::ui::scaleform::MovieImpl::Capture) virtual: gates on `Context::HasChanges` when `if_changed` is set, suspends the GC, and
    /// runs `Render::Context::Capture` to publish the frame's display-tree changes as the pending
    /// snapshot. `CUIManager::PreRender` calls it right after `Advance`, on the game update
    /// thread with the deferred render lock held -- the only point in the frame where every
    /// display-tree writer is quiescent.
    pub unsafe fn CaptureImpl(&mut self, if_changed: bool) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, if_changed: bool) -> u64 = ::std::mem::transmute(
                Self::CaptureImpl_ADDRESS,
            );
            f(self as *mut Self as _, if_changed)
        }
    }
    pub const HandleEventImpl_ADDRESS: usize = 0x1419830D0;
    /// The concrete implementation behind [`HandleEvent`](crate::ui::scaleform::MovieImpl::HandleEvent). For mouse move/down/up/wheel events it transforms the event's position by the
    /// inverse of [`ViewportMatrix`](crate::ui::scaleform::MovieImpl::ViewportMatrix) (view-rectangle pixels to stage
    /// twips) and queues the result on the input-events queue; nothing is stored raw. The next
    /// `Advance` drains the queue into AS3 mouse processing.
    pub unsafe fn HandleEventImpl(
        &mut self,
        event: *const crate::ui::scaleform::MouseEvent,
    ) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                event: *const crate::ui::scaleform::MouseEvent,
            ) -> u32 = ::std::mem::transmute(Self::HandleEventImpl_ADDRESS);
            f(self as *mut Self as _, event)
        }
    }
    pub const NotifyMouseStateImpl_ADDRESS: usize = 0x141983830;
    /// The concrete implementation behind [`NotifyMouseState`](crate::ui::scaleform::MovieImpl::NotifyMouseState). Transforms `(x, y)` by the inverse of
    /// [`ViewportMatrix`](crate::ui::scaleform::MovieImpl::ViewportMatrix), queues a mouse move at the resulting stage
    /// position (twips), then diffs `buttons` against the tracked per-mouse button state and
    /// queues a button event per changed bit.
    pub unsafe fn NotifyMouseStateImpl(
        &mut self,
        x: f32,
        y: f32,
        buttons: u32,
        mouse_index: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                x: f32,
                y: f32,
                buttons: u32,
                mouse_index: u32,
            ) = ::std::mem::transmute(Self::NotifyMouseStateImpl_ADDRESS);
            f(self as *mut Self as _, x, y, buttons, mouse_index)
        }
    }
    pub const SetViewportImpl_ADDRESS: usize = 0x14198E4F0;
    /// The concrete implementation behind the `SetViewport` virtual. When `desc` differs from
    /// [`Viewport`](crate::ui::scaleform::MovieImpl::Viewport) (a memcmp over the whole [`Viewport`](crate::ui::scaleform::Viewport)), copies
    /// it in, runs [`UpdateViewport`](crate::ui::scaleform::MovieImpl::UpdateViewport), and pushes the new state to the
    /// render tree ([`TreeRoot::SetViewport`](crate::ui::scaleform::TreeRoot::SetViewport), background color, and `TreeNode::SetMatrix` with
    /// [`ViewportMatrix`](crate::ui::scaleform::MovieImpl::ViewportMatrix) on [`pRenderRoot`](crate::ui::scaleform::MovieImpl::pRenderRoot)).
    pub unsafe fn SetViewportImpl(
        &mut self,
        desc: *const crate::ui::scaleform::Viewport,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                desc: *const crate::ui::scaleform::Viewport,
            ) = ::std::mem::transmute(Self::SetViewportImpl_ADDRESS);
            f(self as *mut Self as _, desc)
        }
    }
    pub const UpdateViewport_ADDRESS: usize = 0x14198ACC0;
    /// Recomputes [`VisibleFrameRect`](crate::ui::scaleform::MovieImpl::VisibleFrameRect), the view scales and offsets,
    /// and [`PixelScale`](crate::ui::scaleform::MovieImpl::PixelScale) from [`Viewport`](crate::ui::scaleform::MovieImpl::Viewport), the
    /// stage frame rectangle, and [`ViewScaleMode`](crate::ui::scaleform::MovieImpl::ViewScaleMode), then calls
    /// [`ResetViewportMatrix`](crate::ui::scaleform::MovieImpl::ResetViewportMatrix). Called only from
    /// [`SetViewportImpl`](crate::ui::scaleform::MovieImpl::SetViewportImpl); nothing engine-side recomputes the
    /// viewport per frame.
    pub unsafe fn UpdateViewport(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdateViewport_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const ResetViewportMatrix_ADDRESS: usize = 0x141987380;
    /// Rebuilds [`ViewportMatrix`](crate::ui::scaleform::MovieImpl::ViewportMatrix) from
    /// [`Viewport`](crate::ui::scaleform::MovieImpl::Viewport) `Width`/`Height` and
    /// [`VisibleFrameRect`](crate::ui::scaleform::MovieImpl::VisibleFrameRect) (see the matrix field for the formula).
    /// Does not push the matrix to the render tree.
    pub unsafe fn ResetViewportMatrix(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ResetViewportMatrix_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const ProcessInput_ADDRESS: usize = 0x141994890;
    /// Drains the input-events queue during `Advance`: keyboard entries go to focus handling,
    /// mouse entries to `ProcessMouse` (which runs `MouseState::UpdateState` and hit-tests the
    /// display tree at the entry's stage-twips position). Afterwards, for any tracked mouse that
    /// saw no entry this drain while [`FLAG_NEED_MOUSE_UPDATE`](crate::ui::scaleform::MovieImpl::FLAG_NEED_MOUSE_UPDATE) is
    /// set and the mouse is [`FLAG_ACTIVATED`](crate::ui::scaleform::MouseState::FLAG_ACTIVATED), re-resolves the hover
    /// target from [`MouseState::LastPosition`](crate::ui::scaleform::MouseState::LastPosition).
    pub unsafe fn ProcessInput(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ProcessInput_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const GetMouseStateImpl_ADDRESS: usize = 0x141983920;
    /// The concrete implementation behind [`GetMouseState`](crate::ui::scaleform::MovieImpl::GetMouseState).
    pub unsafe fn GetMouseStateImpl(
        &mut self,
        mouse_index: u32,
        x: *mut f32,
        y: *mut f32,
        buttons: *mut u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                mouse_index: u32,
                x: *mut f32,
                y: *mut f32,
                buttons: *mut u32,
            ) = ::std::mem::transmute(Self::GetMouseStateImpl_ADDRESS);
            f(self as *mut Self as _, mouse_index, x, y, buttons)
        }
    }
    pub const HitTestImpl_ADDRESS: usize = 0x1419839A0;
    /// The concrete implementation behind [`HitTest`](crate::ui::scaleform::MovieImpl::HitTest).
    pub unsafe fn HitTestImpl(
        &mut self,
        x: f32,
        y: f32,
        test_cond: u32,
        controller_idx: u32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                x: f32,
                y: f32,
                test_cond: u32,
                controller_idx: u32,
            ) -> bool = ::std::mem::transmute(Self::HitTestImpl_ADDRESS);
            f(self as *mut Self as _, x, y, test_cond, controller_idx)
        }
    }
    /// Advances the movie by `delta_t` seconds: processes queued
    /// input, runs the timeline and AS3 frame, and, when `capture` is set, finishes with
    /// [`CaptureImpl`](crate::ui::scaleform::MovieImpl::CaptureImpl). Returns the time until the next frame is due.
    pub unsafe fn Advance(
        &mut self,
        delta_t: f32,
        frame_catch_up_count: u32,
        capture: bool,
    ) -> f32 {
        unsafe {
            let f = (&raw const (*self.vftable()).Advance).read();
            f(self as *mut Self as _, delta_t, frame_catch_up_count, capture)
        }
    }
    /// Snapshots the display tree for the render thread. Must be
    /// called from the current capture thread (see
    /// [`SetCaptureThread`](crate::ui::scaleform::MovieImpl::SetCaptureThread)); `if_changed = false` forces a fresh
    /// snapshot. The next [`NextCapture`](crate::ui::scaleform::RTHandle::NextCapture) on the render side picks it up.
    pub unsafe fn Capture(&mut self, if_changed: bool) -> u64 {
        unsafe {
            let f = (&raw const (*self.vftable()).Capture).read();
            f(self as *mut Self as _, if_changed)
        }
    }
    /// Returns the movie's display handle: the pinned
    /// `DisplayHandle<TreeRoot>` whose `pData` the renderer copies (with an `AddRef`) into a
    /// stack [`RTHandle`](crate::ui::scaleform::RTHandle) before consuming the frame's capture.
    pub unsafe fn GetDisplayHandle(
        &mut self,
    ) -> *mut crate::ui::scaleform::DisplayHandle {
        unsafe {
            let f = (&raw const (*self.vftable()).GetDisplayHandle).read();
            f(self as *mut Self as _)
        }
    }
    /// Reassigns which thread owns the capture. The engine hands
    /// ownership between the update thread and the render thread this way
    /// (`RenderOffScreenTextures` does exactly this per frame).
    pub unsafe fn SetCaptureThread(&mut self, thread_id: u32) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetCaptureThread).read();
            f(self as *mut Self as _, thread_id)
        }
    }
    /// Shuts down the movie's render context: a thunk into
    /// `Render::ContextImpl::Context::Shutdown` on the embedded
    /// [`RenderContext`](crate::ui::scaleform::MovieImpl::RenderContext). `wait`
    /// blocks until the render thread has released the context.
    pub unsafe fn ShutdownRendering(&mut self, wait: bool) {
        unsafe {
            let f = (&raw const (*self.vftable()).ShutdownRendering).read();
            f(self as *mut Self as _, wait)
        }
    }
    /// Routes a `GFx::Event` to the movie. For a
    /// [`MouseEvent`](crate::ui::scaleform::MouseEvent) the movie records the new mouse state, which the next `Advance`
    /// processes into AS3 mouse events (hover, press, release). Call on the capture (game
    /// update) thread. `CUIManager::SendMouseEvents` is the engine's only mouse feeder, and
    /// only emits a move event on frames where the DirectInput mouse reported a non-zero
    /// delta.
    pub unsafe fn HandleEvent(
        &mut self,
        event: *const crate::ui::scaleform::MouseEvent,
    ) -> u32 {
        unsafe {
            let f = (&raw const (*self.vftable()).HandleEvent).read();
            f(self as *mut Self as _, event)
        }
    }
    /// Reads one mouse's current state: the position comes
    /// from [`MouseStates`](crate::ui::scaleform::MovieImpl::MouseStates)`[mouse_index].LastPosition` converted to
    /// view-rectangle pixels (`(twips * 0.05 - ViewOffset) / ViewScale` per axis, the
    /// same space [`NotifyMouseState`](crate::ui::scaleform::MovieImpl::NotifyMouseState) takes), and `buttons`
    /// from [`CurButtonsState`](crate::ui::scaleform::MouseState::CurButtonsState). Out pointers may be null; a
    /// `mouse_index` at or beyond [`MouseCursorCount`](crate::ui::scaleform::MovieImpl::MouseCursorCount) leaves
    /// them untouched.
    pub unsafe fn GetMouseState(
        &mut self,
        mouse_index: u32,
        x: *mut f32,
        y: *mut f32,
        buttons: *mut u32,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).GetMouseState).read();
            f(self as *mut Self as _, mouse_index, x, y, buttons)
        }
    }
    /// Directly overwrites one mouse's state: position in
    /// movie-viewport pixels (the same space as [`MouseEvent`](crate::ui::scaleform::MouseEvent)) plus a button bitmask
    /// ([`MOUSE_BUTTON_LEFT`](crate::ui::scaleform::MouseState::MOUSE_BUTTON_LEFT) and
    /// [`MOUSE_BUTTON_RIGHT`](crate::ui::scaleform::MouseState::MOUSE_BUTTON_RIGHT)), bypassing the event objects. Processed on the next `Advance`, like
    /// [`HandleEvent`](crate::ui::scaleform::MovieImpl::HandleEvent).
    pub unsafe fn NotifyMouseState(
        &mut self,
        x: f32,
        y: f32,
        buttons: u32,
        mouse_index: u32,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).NotifyMouseState).read();
            f(self as *mut Self as _, x, y, buttons, mouse_index)
        }
    }
    /// Hit-tests the display tree at a point in view-rectangle pixels: transforms by the inverse of
    /// [`ViewportMatrix`](crate::ui::scaleform::MovieImpl::ViewportMatrix), then tests each root level's shapes.
    /// `test_cond` is one of the `HIT_TEST_*` constants, such as
    /// [`HIT_TEST_SHAPES`](crate::ui::scaleform::MovieImpl::HIT_TEST_SHAPES).
    pub unsafe fn HitTest(
        &mut self,
        x: f32,
        y: f32,
        test_cond: u32,
        controller_idx: u32,
    ) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).HitTest).read();
            f(self as *mut Self as _, x, y, test_cond, controller_idx)
        }
    }
    /// Sets how many mice the movie tracks; `0` disables mouse
    /// processing entirely. `CUIManager::RestoreAfterReset` sets `1` when a DirectInput mouse
    /// device exists and `0` otherwise.
    pub unsafe fn SetMouseCursorCount(&mut self, count: u32) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetMouseCursorCount).read();
            f(self as *mut Self as _, count)
        }
    }
    /// Returns the movie's memory heap, [`pHeap`](crate::ui::scaleform::MovieImpl::pHeap).
    pub unsafe fn GetHeap(&self) -> *mut crate::ui::scaleform::MemoryHeap {
        unsafe {
            let f = (&raw const (*self.vftable()).GetHeap).read();
            f(self as *const Self as _)
        }
    }
}
impl MovieImpl {
    /// The [`ViewAlignment`](crate::ui::scaleform::MovieImpl::ViewAlignment) that centres the stage (`Align_Center`), the
    /// constructor default.
    pub const ALIGN_CENTER: u32 = 0;
    /// The [`Flags`](crate::ui::scaleform::MovieImpl::Flags) bit (`Flag_NeedMouseUpdate`) marking mouse state pending
    /// re-resolution; [`ProcessInput`](crate::ui::scaleform::MovieImpl::ProcessInput) tests and clears it.
    pub const FLAG_NEED_MOUSE_UPDATE: u32 = 128;
    /// The [`HitTest`](crate::ui::scaleform::MovieImpl::HitTest) `test_cond` (`GFx::Movie::HitTestType`) that tests bounds.
    pub const HIT_TEST_BOUNDS: u32 = 0;
    /// The `test_cond` that tests for button events from `controller_idx`.
    pub const HIT_TEST_BUTTON_EVENTS: u32 = 2;
    /// The `test_cond` that tests shapes.
    pub const HIT_TEST_SHAPES: u32 = 1;
    /// The `test_cond` that tests shapes, counting invisible ones.
    pub const HIT_TEST_SHAPES_NO_INVISIBLE: u32 = 3;
    /// The [`ViewScaleMode`](crate::ui::scaleform::MovieImpl::ViewScaleMode) that stretches the stage to the view
    /// rectangle exactly, ignoring aspect.
    pub const SM_EXACT_FIT: u32 = 2;
    /// The [`ViewScaleMode`](crate::ui::scaleform::MovieImpl::ViewScaleMode) that fills the view rectangle, preserving
    /// aspect (cropping as needed).
    pub const SM_NO_BORDER: u32 = 3;
    /// The [`ViewScaleMode`](crate::ui::scaleform::MovieImpl::ViewScaleMode) that maps the movie 1:1 at
    /// [`Viewport::Scale`](crate::ui::scaleform::Viewport::Scale) with no stretching (the view rectangle crops or pads the stage).
    pub const SM_NO_SCALE: u32 = 0;
    /// The [`ViewScaleMode`](crate::ui::scaleform::MovieImpl::ViewScaleMode) that fits the whole stage inside the view
    /// rectangle, preserving aspect (letterboxing as needed). The constructor default.
    pub const SM_SHOW_ALL: u32 = 1;
}
impl std::convert::AsRef<MovieImpl> for MovieImpl {
    fn as_ref(&self) -> &MovieImpl {
        self
    }
}
impl std::convert::AsMut<MovieImpl> for MovieImpl {
    fn as_mut(&mut self) -> &mut MovieImpl {
        self
    }
}
#[repr(C, align(8))]
pub struct MovieImplVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_3: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_11: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_12: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_13: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_14: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_15: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_16: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_17: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_18: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_19: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_20: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_21: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_22: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_23: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    /// Advances the movie by `delta_t` seconds: processes queued
    /// input, runs the timeline and AS3 frame, and, when `capture` is set, finishes with
    /// [`CaptureImpl`](crate::ui::scaleform::MovieImpl::CaptureImpl). Returns the time until the next frame is due.
    pub Advance: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        delta_t: f32,
        frame_catch_up_count: u32,
        capture: bool,
    ) -> f32,
    /// Snapshots the display tree for the render thread. Must be
    /// called from the current capture thread (see
    /// [`SetCaptureThread`](crate::ui::scaleform::MovieImpl::SetCaptureThread)); `if_changed = false` forces a fresh
    /// snapshot. The next [`NextCapture`](crate::ui::scaleform::RTHandle::NextCapture) on the render side picks it up.
    pub Capture: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        if_changed: bool,
    ) -> u64,
    /// Returns the movie's display handle: the pinned
    /// `DisplayHandle<TreeRoot>` whose `pData` the renderer copies (with an `AddRef`) into a
    /// stack [`RTHandle`](crate::ui::scaleform::RTHandle) before consuming the frame's capture.
    pub GetDisplayHandle: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
    ) -> *mut crate::ui::scaleform::DisplayHandle,
    /// Reassigns which thread owns the capture. The engine hands
    /// ownership between the update thread and the render thread this way
    /// (`RenderOffScreenTextures` does exactly this per frame).
    pub SetCaptureThread: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        thread_id: u32,
    ),
    /// Shuts down the movie's render context: a thunk into
    /// `Render::ContextImpl::Context::Shutdown` on the embedded
    /// [`RenderContext`](crate::ui::scaleform::MovieImpl::RenderContext). `wait`
    /// blocks until the render thread has released the context.
    pub ShutdownRendering: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        wait: bool,
    ),
    _vfunc_29: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_30: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_31: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_32: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_33: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_34: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    /// Routes a `GFx::Event` to the movie. For a
    /// [`MouseEvent`](crate::ui::scaleform::MouseEvent) the movie records the new mouse state, which the next `Advance`
    /// processes into AS3 mouse events (hover, press, release). Call on the capture (game
    /// update) thread. `CUIManager::SendMouseEvents` is the engine's only mouse feeder, and
    /// only emits a move event on frames where the DirectInput mouse reported a non-zero
    /// delta.
    pub HandleEvent: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        event: *const crate::ui::scaleform::MouseEvent,
    ) -> u32,
    /// Reads one mouse's current state: the position comes
    /// from [`MouseStates`](crate::ui::scaleform::MovieImpl::MouseStates)`[mouse_index].LastPosition` converted to
    /// view-rectangle pixels (`(twips * 0.05 - ViewOffset) / ViewScale` per axis, the
    /// same space [`NotifyMouseState`](crate::ui::scaleform::MovieImpl::NotifyMouseState) takes), and `buttons`
    /// from [`CurButtonsState`](crate::ui::scaleform::MouseState::CurButtonsState). Out pointers may be null; a
    /// `mouse_index` at or beyond [`MouseCursorCount`](crate::ui::scaleform::MovieImpl::MouseCursorCount) leaves
    /// them untouched.
    pub GetMouseState: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        mouse_index: u32,
        x: *mut f32,
        y: *mut f32,
        buttons: *mut u32,
    ),
    /// Directly overwrites one mouse's state: position in
    /// movie-viewport pixels (the same space as [`MouseEvent`](crate::ui::scaleform::MouseEvent)) plus a button bitmask
    /// ([`MOUSE_BUTTON_LEFT`](crate::ui::scaleform::MouseState::MOUSE_BUTTON_LEFT) and
    /// [`MOUSE_BUTTON_RIGHT`](crate::ui::scaleform::MouseState::MOUSE_BUTTON_RIGHT)), bypassing the event objects. Processed on the next `Advance`, like
    /// [`HandleEvent`](crate::ui::scaleform::MovieImpl::HandleEvent).
    pub NotifyMouseState: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        x: f32,
        y: f32,
        buttons: u32,
        mouse_index: u32,
    ),
    /// Hit-tests the display tree at a point in view-rectangle pixels: transforms by the inverse of
    /// [`ViewportMatrix`](crate::ui::scaleform::MovieImpl::ViewportMatrix), then tests each root level's shapes.
    /// `test_cond` is one of the `HIT_TEST_*` constants, such as
    /// [`HIT_TEST_SHAPES`](crate::ui::scaleform::MovieImpl::HIT_TEST_SHAPES).
    pub HitTest: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        x: f32,
        y: f32,
        test_cond: u32,
        controller_idx: u32,
    ) -> bool,
    _vfunc_39: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_40: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_41: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_42: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    /// Sets how many mice the movie tracks; `0` disables mouse
    /// processing entirely. `CUIManager::RestoreAfterReset` sets `1` when a DirectInput mouse
    /// device exists and `0` otherwise.
    pub SetMouseCursorCount: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        count: u32,
    ),
    _vfunc_44: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_45: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_46: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_47: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    /// Returns the movie's memory heap, [`pHeap`](crate::ui::scaleform::MovieImpl::pHeap).
    pub GetHeap: unsafe extern "system" fn(
        this: *const crate::ui::scaleform::MovieImpl,
    ) -> *mut crate::ui::scaleform::MemoryHeap,
}
fn _MovieImplVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x188], MovieImplVftable>([0u8; 0x188]);
    }
    unreachable!()
}
impl MovieImplVftable {}
impl std::convert::AsRef<MovieImplVftable> for MovieImplVftable {
    fn as_ref(&self) -> &MovieImplVftable {
        self
    }
}
impl std::convert::AsMut<MovieImplVftable> for MovieImplVftable {
    fn as_mut(&mut self) -> &mut MovieImplVftable {
        self
    }
}
#[repr(C, align(8))]
pub struct MovieVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_3: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_11: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_12: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_13: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_14: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_15: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_16: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_17: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_18: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_19: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_20: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_21: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_22: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_23: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_24: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_25: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_26: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_27: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_28: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_29: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_30: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_31: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_32: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_33: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_34: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    /// Recursively walks the runtime display tree from the root movie clip: returns a tree of
    /// [`AmpMovieObjectDesc`](crate::ui::scaleform::AmpMovieObjectDesc) nodes, each carrying the clip's instance name (or `"Unnamed"`)
    /// and a child array, calling `DisplayObject::GetName` on every child and recursing into
    /// containers. `heap` is the Scaleform `MemoryHeap*` used for the allocations (e.g.
    /// [`MovieImpl::pHeap`](crate::ui::scaleform::MovieImpl::pHeap)); the tree is freed by releasing the root node. The implementation
    /// is [`GetDisplayObjectsTreeImpl`](crate::ui::scaleform::Movie::GetDisplayObjectsTreeImpl). Call on the capture
    /// (game update) thread, where the display tree is stable.
    pub GetDisplayObjectsTree: unsafe extern "system" fn(
        this: *const crate::ui::scaleform::Movie,
        heap: *mut crate::ui::scaleform::MemoryHeap,
    ) -> *mut crate::ui::scaleform::AmpMovieObjectDesc,
}
fn _MovieVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x120], MovieVftable>([0u8; 0x120]);
    }
    unreachable!()
}
impl MovieVftable {}
impl std::convert::AsRef<MovieVftable> for MovieVftable {
    fn as_ref(&self) -> &MovieVftable {
        self
    }
}
impl std::convert::AsMut<MovieVftable> for MovieVftable {
    fn as_mut(&mut self) -> &mut MovieVftable {
        self
    }
}
#[repr(C, align(8))]
/// A Scaleform AS3 object (the managed payload behind a display-object [`Value::mValue`](crate::ui::scaleform::Value::mValue)): its type
/// [`m_Traits`](crate::ui::scaleform::Object::m_Traits) classify it, and for a display object its
/// [`m_DisplayObject`](crate::ui::scaleform::Object::m_DisplayObject) is the presentation-side [`DisplayObjectBase`](crate::ui::scaleform::DisplayObjectBase).
pub struct Object {
    _field_0: [u8; 40],
    /// The object's type traits (class metadata), used to check it really is a display object
    /// before reaching [`m_DisplayObject`](crate::ui::scaleform::Object::m_DisplayObject).
    pub m_Traits: *mut crate::ui::scaleform::Traits,
    _field_30: [u8; 88],
    /// The presentation-side display object, valid once [`m_Traits`](crate::ui::scaleform::Object::m_Traits) confirm a
    /// display-object kind.
    pub m_DisplayObject: *mut crate::ui::scaleform::DisplayObjectBase,
}
fn _Object_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x90], Object>([0u8; 0x90]);
    }
    unreachable!()
}
impl Object {}
impl std::convert::AsRef<Object> for Object {
    fn as_ref(&self) -> &Object {
        self
    }
}
impl std::convert::AsMut<Object> for Object {
    fn as_mut(&mut self) -> &mut Object {
        self
    }
}
#[repr(C, align(8))]
/// A stack `Render::ContextImpl::RTHandle` over a copied
/// [`DisplayHandle::pData`](crate::ui::scaleform::DisplayHandle::pData) (AddRef'd first, destructed after use).
pub struct RTHandle {
    /// The shared, refcounted handle data.
    pub pData: *mut crate::ui::scaleform::RefCountImpl,
}
fn _RTHandle_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], RTHandle>([0u8; 0x8]);
    }
    unreachable!()
}
impl RTHandle {
    pub const NextCapture_ADDRESS: usize = 0x1419A7970;
    /// Consumes the context's pending snapshot into the displaying one (at most once per HAL
    /// frame: the context's [`NextCaptureCalledInFrame`](crate::ui::scaleform::RenderContext::NextCaptureCalledInFrame)
    /// latch short-circuits repeats). Returns whether the handle is valid to draw. `notify` is
    /// the HAL's context notify ([`RenderHAL::GetContextNotify`](crate::ui::scaleform::RenderHAL::GetContextNotify)); `frame_id` is 0.
    pub unsafe fn NextCapture(
        &mut self,
        notify: *mut ::std::ffi::c_void,
        frame_id: u64,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                notify: *mut ::std::ffi::c_void,
                frame_id: u64,
            ) -> bool = ::std::mem::transmute(Self::NextCapture_ADDRESS);
            f(self as *mut Self as _, notify, frame_id)
        }
    }
    pub const GetRenderEntry_ADDRESS: usize = 0x1419A4620;
    /// The handle's root entry in the current displaying snapshot, for `HAL::Draw`.
    pub unsafe fn GetRenderEntry(&self) -> *mut crate::ui::scaleform::TreeRoot {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
            ) -> *mut crate::ui::scaleform::TreeRoot = ::std::mem::transmute(
                Self::GetRenderEntry_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const Destruct_ADDRESS: usize = 0x1419A4600;
    /// The RTHandle destructor (releases the handle data).
    pub unsafe fn Destruct(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Destruct_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<RTHandle> for RTHandle {
    fn as_ref(&self) -> &RTHandle {
        self
    }
}
impl std::convert::AsMut<RTHandle> for RTHandle {
    fn as_mut(&mut self) -> &mut RTHandle {
        self
    }
}
#[repr(C, align(8))]
/// Scaleform [`RefCountImpl`](crate::ui::scaleform::RefCountImpl) -- the base class for reference-counted Scaleform objects. Provides
/// a vtable pointer and an atomic [`ref_count`](crate::ui::scaleform::RefCountImpl::ref_count). `AddRef`/`Release` are
/// non-virtual; the virtual [`destructor`](crate::ui::scaleform::RefCountImpl::destructor) runs when the refcount reaches zero.
pub struct RefCountImpl {
    vftable: *const crate::ui::scaleform::RefCountImplVftable,
    /// Atomic reference count. Incremented by `AddRef`, decremented by `Release`.
    pub ref_count: u32,
    _field_c: [u8; 4],
}
fn _RefCountImpl_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], RefCountImpl>([0u8; 0x10]);
    }
    unreachable!()
}
impl RefCountImpl {
    pub fn vftable(&self) -> *const crate::ui::scaleform::RefCountImplVftable {
        self.vftable as *const crate::ui::scaleform::RefCountImplVftable
    }
    pub const AddRef_ADDRESS: usize = 0x141998430;
    /// Increments the reference count atomically.
    pub unsafe fn AddRef(&self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) = ::std::mem::transmute(
                Self::AddRef_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const Release_ADDRESS: usize = 0x141998440;
    /// Decrements the reference count atomically. When it reaches zero, calls the virtual
    /// [`destructor`](crate::ui::scaleform::AmpMovieObjectDesc::destructor).
    pub unsafe fn Release(&self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) = ::std::mem::transmute(
                Self::Release_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    /// Virtual destructor. Called by `Release` when the refcount reaches zero.
    pub unsafe fn destructor(&mut self, flags: u32) {
        unsafe {
            let f = (&raw const (*self.vftable()).destructor).read();
            f(self as *mut Self as _, flags)
        }
    }
}
impl std::convert::AsRef<RefCountImpl> for RefCountImpl {
    fn as_ref(&self) -> &RefCountImpl {
        self
    }
}
impl std::convert::AsMut<RefCountImpl> for RefCountImpl {
    fn as_mut(&mut self) -> &mut RefCountImpl {
        self
    }
}
#[repr(C, align(8))]
pub struct RefCountImplVftable {
    /// Virtual destructor. Called by `Release` when the refcount reaches zero.
    pub destructor: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RefCountImpl,
        flags: u32,
    ),
}
fn _RefCountImplVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], RefCountImplVftable>([0u8; 0x8]);
    }
    unreachable!()
}
impl RefCountImplVftable {}
impl std::convert::AsRef<RefCountImplVftable> for RefCountImplVftable {
    fn as_ref(&self) -> &RefCountImplVftable {
        self
    }
}
impl std::convert::AsMut<RefCountImplVftable> for RefCountImplVftable {
    fn as_mut(&mut self) -> &mut RefCountImplVftable {
        self
    }
}
#[repr(C, align(8))]
/// The Scaleform `Render::ContextImpl::Context`: the display-tree snapshot pipeline between the
/// update thread and the renderer. `Capture` (update thread) merges the active snapshot's changes
/// into the pending snapshot; [`NextCapture`](crate::ui::scaleform::RTHandle::NextCapture) (render thread) consumes the pending
/// snapshot into the displaying one, at most once per HAL frame. Only some fields are modeled;
/// the declared size covers only the modeled prefix.
pub struct RenderContext {
    _field_0: [u8; 153],
    /// The once-a-frame consumption latch: while set, [`NextCapture`](crate::ui::scaleform::RTHandle::NextCapture) keeps the current
    /// displaying snapshot instead of consuming the pending one. Set by the first `NextCapture`
    /// of a HAL frame and cleared by `HAL::EndFrame` (`EndFrameContextNotify`), so within one
    /// `CUIManager::Render` call every draw sees the same snapshot.
    pub NextCaptureCalledInFrame: bool,
    _field_9a: [u8; 86],
    /// The per-slot snapshot frame ids, indexed like `pSnapshots`: `[0]` the active snapshot
    /// (incremented by every `Capture`), `[1]` pending, `[2]` displaying, `[3]` finalizing. The
    /// gap between `[0]` and `[2]` is how far the displayed UI trails the update thread -- a
    /// growing gap means the render side is not consuming captures.
    pub SnapshotFrameIds: [u64; 4],
    _field_110: [u8; 8],
}
fn _RenderContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x118], RenderContext>([0u8; 0x118]);
    }
    unreachable!()
}
impl RenderContext {
    pub const CreateEntryTreeRoot_ADDRESS: usize = 0x141994560;
    /// Creates a fresh [`TreeRoot`](crate::ui::scaleform::TreeRoot) entry in this context (`Context::CreateEntry<TreeRoot>`):
    /// allocates its `NodeData` from the context heap and registers the entry. The root starts
    /// parentless with no viewport; [`TreeRoot::SetViewport`](crate::ui::scaleform::TreeRoot::SetViewport) sets its viewport and
    /// [`TreeNode::SetMatrix`](crate::ui::scaleform::TreeNode::SetMatrix) its stage matrix. Call on the capture (game update) thread.
    pub unsafe fn CreateEntryTreeRoot(&mut self) -> *mut crate::ui::scaleform::TreeRoot {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
            ) -> *mut crate::ui::scaleform::TreeRoot = ::std::mem::transmute(
                Self::CreateEntryTreeRoot_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const CreateEntryTreeContainer_ADDRESS: usize = 0x141990AF0;
    /// Creates a fresh empty [`TreeContainer`](crate::ui::scaleform::TreeContainer) entry in this context
    /// (`Context::CreateEntry<TreeContainer>`). Call on the capture (game update) thread.
    pub unsafe fn CreateEntryTreeContainer(
        &mut self,
    ) -> *mut crate::ui::scaleform::TreeContainer {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
            ) -> *mut crate::ui::scaleform::TreeContainer = ::std::mem::transmute(
                Self::CreateEntryTreeContainer_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<RenderContext> for RenderContext {
    fn as_ref(&self) -> &RenderContext {
        self
    }
}
impl std::convert::AsMut<RenderContext> for RenderContext {
    fn as_mut(&mut self) -> &mut RenderContext {
        self
    }
}
#[repr(C, align(8))]
/// The Scaleform `Render::D3D1x::HAL` behind `CUIManager::m_RenderHAL`: the D3D11 rendering
/// backend the UI render worker draws through.
pub struct RenderHAL {
    vftable: *const crate::ui::scaleform::RenderHALVftable,
    _field_8: [u8; 291744],
    /// The `ID3D11Device` the HAL was initialized with.
    pub pDevice: *mut ::std::ffi::c_void,
    /// The `ID3D11DeviceContext` every HAL draw goes through (possibly a deferred context; see
    /// the HAL's `UsingDeferredContext` flag). Work recorded on it from the UI render worker is
    /// ordered with the HAL's own draws.
    pub pDeviceContext: *mut ::std::ffi::c_void,
}
fn _RenderHAL_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x473B8], RenderHAL>([0u8; 0x473B8]);
    }
    unreachable!()
}
impl RenderHAL {
    pub fn vftable(&self) -> *const crate::ui::scaleform::RenderHALVftable {
        self.vftable as *const crate::ui::scaleform::RenderHALVftable
    }
    pub const CreateRenderTarget_ADDRESS: usize = 0x141DE1110;
    /// Builds a [`RenderTarget`](crate::ui::scaleform::RenderTarget) from D3D color/depth views (`ID3D11RenderTargetView*` /
    /// `ID3D11DepthStencilView*`); the concrete implementation behind the `CreateRenderTarget`
    /// virtual.
    /// `RenderOffScreenTextures` creates one per off-screen draw and releases it afterwards.
    /// Render thread only.
    pub unsafe fn CreateRenderTarget(
        &mut self,
        color: *mut ::std::ffi::c_void,
        depth: *mut ::std::ffi::c_void,
    ) -> *mut crate::ui::scaleform::RenderTarget {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                color: *mut ::std::ffi::c_void,
                depth: *mut ::std::ffi::c_void,
            ) -> *mut crate::ui::scaleform::RenderTarget = ::std::mem::transmute(
                Self::CreateRenderTarget_ADDRESS,
            );
            f(self as *mut Self as _, color, depth)
        }
    }
    pub const Draw_ADDRESS: usize = 0x1419B4850;
    /// Draws a [`TreeRoot`](crate::ui::scaleform::TreeRoot)'s displaying-snapshot subtree into the bound target
    /// (`Scaleform::Render::HAL::Draw`). Render thread only, within `BeginFrame`/`BeginScene`.
    pub unsafe fn Draw(&mut self, root: *mut crate::ui::scaleform::TreeRoot) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                root: *mut crate::ui::scaleform::TreeRoot,
            ) = ::std::mem::transmute(Self::Draw_ADDRESS);
            f(self as *mut Self as _, root)
        }
    }
    /// Binds `target` as the frame's display render target. `CUIManager::Render`
    /// calls it with `m_RenderBuffer` before `BeginFrame`, paired with a trailing
    /// [`PopRenderTarget`](crate::ui::scaleform::RenderHAL::PopRenderTarget).
    pub unsafe fn SetRenderTarget(
        &mut self,
        target: *mut crate::ui::scaleform::RenderTarget,
        set_state: bool,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetRenderTarget).read();
            f(self as *mut Self as _, target, set_state)
        }
    }
    /// Pushes `target` for nested rendering within a frame, as
    /// `RenderOffScreenTextures` does per off-screen movie. `frame_rect` is `[x0, y0, x1,
    /// y1]` floats; `clear_color` points at a packed color (0 = transparent).
    pub unsafe fn PushRenderTarget(
        &mut self,
        target: *mut crate::ui::scaleform::RenderTarget,
        flags: u64,
        frame_rect: *const f32,
        clear_color: *const u32,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).PushRenderTarget).read();
            f(self as *mut Self as _, target, flags, frame_rect, clear_color)
        }
    }
    /// Pops the pushed (or set) render target.
    pub unsafe fn PopRenderTarget(&mut self, flags: u32) {
        unsafe {
            let f = (&raw const (*self.vftable()).PopRenderTarget).read();
            f(self as *mut Self as _, flags)
        }
    }
    /// Begins the HAL frame. `EndFrame` clears every context's once-a-frame
    /// consumption latch via `EndFrameContextNotify`.
    pub unsafe fn BeginFrame(&mut self) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).BeginFrame).read();
            f(self as *mut Self as _)
        }
    }
    /// Ends the HAL frame.
    pub unsafe fn EndFrame(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).EndFrame).read();
            f(self as *mut Self as _)
        }
    }
    /// Begins a scene (draw batch) within the frame.
    pub unsafe fn BeginScene(&mut self) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).BeginScene).read();
            f(self as *mut Self as _)
        }
    }
    /// Ends the scene.
    pub unsafe fn EndScene(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).EndScene).read();
            f(self as *mut Self as _)
        }
    }
    /// The HAL's `Render::ContextImpl::RenderNotify`, passed to
    /// [`RTHandle::NextCapture`](crate::ui::scaleform::RTHandle::NextCapture).
    pub unsafe fn GetContextNotify(&mut self) -> *mut ::std::ffi::c_void {
        unsafe {
            let f = (&raw const (*self.vftable()).GetContextNotify).read();
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<RenderHAL> for RenderHAL {
    fn as_ref(&self) -> &RenderHAL {
        self
    }
}
impl std::convert::AsMut<RenderHAL> for RenderHAL {
    fn as_mut(&mut self) -> &mut RenderHAL {
        self
    }
}
#[repr(C, align(8))]
pub struct RenderHALVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    /// Binds `target` as the frame's display render target. `CUIManager::Render`
    /// calls it with `m_RenderBuffer` before `BeginFrame`, paired with a trailing
    /// [`PopRenderTarget`](crate::ui::scaleform::RenderHAL::PopRenderTarget).
    pub SetRenderTarget: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderHAL,
        target: *mut crate::ui::scaleform::RenderTarget,
        set_state: bool,
    ),
    /// Pushes `target` for nested rendering within a frame, as
    /// `RenderOffScreenTextures` does per off-screen movie. `frame_rect` is `[x0, y0, x1,
    /// y1]` floats; `clear_color` points at a packed color (0 = transparent).
    pub PushRenderTarget: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderHAL,
        target: *mut crate::ui::scaleform::RenderTarget,
        flags: u64,
        frame_rect: *const f32,
        clear_color: *const u32,
    ),
    /// Pops the pushed (or set) render target.
    pub PopRenderTarget: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderHAL,
        flags: u32,
    ),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    /// Begins the HAL frame. `EndFrame` clears every context's once-a-frame
    /// consumption latch via `EndFrameContextNotify`.
    pub BeginFrame: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderHAL,
    ) -> bool,
    /// Ends the HAL frame.
    pub EndFrame: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_13: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_14: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    /// Begins a scene (draw batch) within the frame.
    pub BeginScene: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderHAL,
    ) -> bool,
    /// Ends the scene.
    pub EndScene: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_17: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_18: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    /// The HAL's `Render::ContextImpl::RenderNotify`, passed to
    /// [`RTHandle::NextCapture`](crate::ui::scaleform::RTHandle::NextCapture).
    pub GetContextNotify: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderHAL,
    ) -> *mut ::std::ffi::c_void,
}
fn _RenderHALVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xA0], RenderHALVftable>([0u8; 0xA0]);
    }
    unreachable!()
}
impl RenderHALVftable {}
impl std::convert::AsRef<RenderHALVftable> for RenderHALVftable {
    fn as_ref(&self) -> &RenderHALVftable {
        self
    }
}
impl std::convert::AsMut<RenderHALVftable> for RenderHALVftable {
    fn as_mut(&mut self) -> &mut RenderHALVftable {
        self
    }
}
#[repr(C, align(8))]
/// A Scaleform `Render::RenderTarget`: the target wrapper `SetRenderTarget`/`PushRenderTarget`
/// take. `RenderOffScreenTextures` creates one per draw from D3D views
/// ([`RenderHAL::CreateRenderTarget`](crate::ui::scaleform::RenderHAL::CreateRenderTarget)) and releases it after the pop.
pub struct RenderTarget {
    vftable: *const crate::ui::scaleform::RenderTargetVftable,
    _field_8: [u8; 16],
}
fn _RenderTarget_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], RenderTarget>([0u8; 0x18]);
    }
    unreachable!()
}
impl RenderTarget {
    pub fn vftable(&self) -> *const crate::ui::scaleform::RenderTargetVftable {
        self.vftable as *const crate::ui::scaleform::RenderTargetVftable
    }
    /// Releases the target (refcounted).
    pub unsafe fn Release(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).Release).read();
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<RenderTarget> for RenderTarget {
    fn as_ref(&self) -> &RenderTarget {
        self
    }
}
impl std::convert::AsMut<RenderTarget> for RenderTarget {
    fn as_mut(&mut self) -> &mut RenderTarget {
        self
    }
}
#[repr(C, align(8))]
pub struct RenderTargetVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderTarget),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderTarget),
    /// Releases the target (refcounted).
    pub Release: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderTarget,
    ),
}
fn _RenderTargetVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], RenderTargetVftable>([0u8; 0x18]);
    }
    unreachable!()
}
impl RenderTargetVftable {}
impl std::convert::AsRef<RenderTargetVftable> for RenderTargetVftable {
    fn as_ref(&self) -> &RenderTargetVftable {
        self
    }
}
impl std::convert::AsMut<RenderTargetVftable> for RenderTargetVftable {
    fn as_mut(&mut self) -> &mut RenderTargetVftable {
        self
    }
}
#[repr(C, align(8))]
/// The engine's ExternalInterface handler (`CScaleformEIHandler`), installed on the UI loader so
/// every movie it creates shares it. AS3 `ExternalInterface.call("<class name>:<command>", ...)`
/// arrives at [`Callback`](crate::ui::scaleform::ScaleformEIHandler::Callback).
pub struct ScaleformEIHandler {
    _field_0: [u8; 24],
    /// The `GetFastHash` of `"ROOT"`: calls with this prefix come from the root movie's own AS3
    /// and are not dispatched.
    pub m_RootHash: u32,
    _field_1c: [u8; 4],
}
fn _ScaleformEIHandler_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], ScaleformEIHandler>([0u8; 0x20]);
    }
    unreachable!()
}
impl ScaleformEIHandler {
    pub const Callback_ADDRESS: usize = 0x140F1C940;
    /// Splits `method_name` at the colon and hashes both halves with `GetFastHash`. Unless the
    /// prefix is the root's, finds the target screen: the movie's user data when set, otherwise
    /// the screen registered under the prefix (`CUIManager::FindUI`). Calls the screen's
    /// [`HandleCommand`](crate::ui::ui_base::UIBase::HandleCommand) with the command hash and the
    /// arguments; a call with an unknown prefix is dropped. Runs synchronously inside the calling
    /// AS3 code, on the capture thread.
    pub unsafe fn Callback(
        &mut self,
        movie: *mut crate::ui::scaleform::MovieImpl,
        method_name: *const u8,
        args: *const crate::ui::scaleform::Value,
        num_args: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                movie: *mut crate::ui::scaleform::MovieImpl,
                method_name: *const u8,
                args: *const crate::ui::scaleform::Value,
                num_args: u32,
            ) = ::std::mem::transmute(Self::Callback_ADDRESS);
            f(self as *mut Self as _, movie, method_name, args, num_args)
        }
    }
}
impl std::convert::AsRef<ScaleformEIHandler> for ScaleformEIHandler {
    fn as_ref(&self) -> &ScaleformEIHandler {
        self
    }
}
impl std::convert::AsMut<ScaleformEIHandler> for ScaleformEIHandler {
    fn as_mut(&mut self) -> &mut ScaleformEIHandler {
        self
    }
}
#[repr(C, align(8))]
/// The engine's Scaleform file opener (`CScaleformFileOpener`), installed on the UI loader: every
/// file a movie loads (movies, imports, images) opens through [`OpenFile`](crate::ui::scaleform::ScaleformFileOpener::OpenFile).
pub struct ScaleformFileOpener {}
impl ScaleformFileOpener {
    pub const OpenFile_ADDRESS: usize = 0x141035600;
    /// Opens `url` as a `Scaleform::File`. Returns the UI manager's file cache entry for `url`
    /// when there is one (adding a reference); otherwise sizes the file on the game's virtual
    /// file system, reads it into a new `CScaleformCachedFile`, and offers that to the file cache,
    /// which keeps only a fixed set of movies. Returns null when the file does not exist.
    pub unsafe fn OpenFile(&mut self, url: *const u8) -> *mut ::std::ffi::c_void {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                url: *const u8,
            ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(Self::OpenFile_ADDRESS);
            f(self as *mut Self as _, url)
        }
    }
}
impl std::convert::AsRef<ScaleformFileOpener> for ScaleformFileOpener {
    fn as_ref(&self) -> &ScaleformFileOpener {
        self
    }
}
impl std::convert::AsMut<ScaleformFileOpener> for ScaleformFileOpener {
    fn as_mut(&mut self) -> &mut ScaleformFileOpener {
        self
    }
}
#[repr(C, align(8))]
/// Static clip names placed on the root timeline of `ui/hud.gfx` via PlaceObject2 tags. The root
/// movie (`ui/root.gfx`) creates its clips entirely through ActionScript, so it has no static
/// PlaceObject2 tags; `hud.gfx` uses a mix of static placement and dynamic creation.
///
/// | Instance name | Depth | Char ID |
/// |---|---|---|
/// | `MCI_hud` | 1432 | 1548 |
/// | `MCI_poi_stage` | 180 | 1066 |
/// | `MCI_weapon_selection_wheel` | 182 | 1078 |
/// | `MCI_safe_area_center` | 1 | 1065 |
/// | `MCI_safe_area_top_left` | 839 | 1538 |
/// | `MCI_safe_area_top_middle` | 636 | 1532 |
/// | `MCI_safe_area_top_right` | 562 | 1395 |
/// | `MCI_safe_area_bottom_left` | 450 | 1330 |
/// | `MCI_safe_area_bottom_middle` | 307 | 1286 |
/// | `MCI_safe_area_bottom_right` | 246 | 1161 |
///
/// UI movie registry from `root.gfx` maps each `CUIBase` subclass to its `.gfx` file:
/// `CSharedLibUI` -> `shared_lib`, `COverlayUI` -> `overlay`, `CHUDUI` -> `hud`, `CIntroUI` ->
/// `intro`, `CTitleUI` -> `title`, `CPauseUI` -> `pause`, `CTutorialsUI` -> `tutorials`,
/// `CCreditsUI` -> `credits`, `CLobbyUI` -> `lobby`, `CMainUI` -> `main`, `CCommLinkUI` ->
/// `comm_link`, `CCommCollectiblesUI` -> `comm_collectibles`, `CCommCommunityUI` ->
/// `comm_community`, `CCommMapUI` -> `comm_map`, `CCommSkillUI` -> `comm_skill`,
/// `CCommStatsUI` -> `comm_stats`, `CCommBragsFeatsUI` -> `comm_brags_feats`,
/// `CCommStoreUI` -> `comm_store`, `CRewardUI` -> `reward`, `CROMUI` -> (none).
pub struct ScaleformInfo {}
impl ScaleformInfo {}
impl std::convert::AsRef<ScaleformInfo> for ScaleformInfo {
    fn as_ref(&self) -> &ScaleformInfo {
        self
    }
}
impl std::convert::AsMut<ScaleformInfo> for ScaleformInfo {
    fn as_mut(&mut self) -> &mut ScaleformInfo {
        self
    }
}
#[repr(C, align(8))]
/// The heap data descriptor behind a `Scaleform::String`: an allocated size and a refcount,
/// followed by the NUL-terminated character data. Reached from the string's pointer after masking
/// off the heap flags ([`HEAP_FLAGS_MASK`](crate::ui::scaleform::StringDataDesc::HEAP_FLAGS_MASK)).
pub struct StringDataDesc {
    /// The allocated size.
    pub Size: u64,
    /// The reference count.
    pub RefCount: u32,
    /// The first character of the NUL-terminated string data.
    pub m_Data: ::std::ffi::c_char,
    _field_d: [u8; 3],
}
fn _StringDataDesc_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], StringDataDesc>([0u8; 0x10]);
    }
    unreachable!()
}
impl StringDataDesc {}
impl StringDataDesc {
    /// The low bits of a `Scaleform::String` pointer that carry heap flags rather than address.
    pub const HEAP_FLAGS_MASK: u64 = 7;
}
impl std::convert::AsRef<StringDataDesc> for StringDataDesc {
    fn as_ref(&self) -> &StringDataDesc {
        self
    }
}
impl std::convert::AsMut<StringDataDesc> for StringDataDesc {
    fn as_mut(&mut self) -> &mut StringDataDesc {
        self
    }
}
#[repr(C, align(8))]
/// A Scaleform AS3 object's type traits: the class metadata identifying an [`Object`](crate::ui::scaleform::Object)'s kind. The
/// display-object type-id range and flag values are hard-coded magic in the engine too (the same
/// constants appear inline in `Value::ObjectInterface::GetDisplayInfo`); their deeper AS3-builtin-type
/// meaning is not recoverable from the stripped Scaleform code, so they are named here by role.
pub struct Traits {
    _field_0: [u8; 112],
    /// Trait flags; see [`FLAG_NON_DISPLAY_OBJECT`](crate::ui::scaleform::Traits::FLAG_NON_DISPLAY_OBJECT).
    pub m_Flags: u8,
    _field_71: [u8; 7],
    /// The traits' AS3 builtin-class type id; see [`is_display_object`](crate::ui::scaleform::Traits::is_display_object).
    pub m_TypeId: u32,
    _field_7c: [u8; 4],
}
fn _Traits_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x80], Traits>([0u8; 0x80]);
    }
    unreachable!()
}
impl Traits {}
impl Traits {
    /// The number of type ids in the display-object range (so display objects are
    /// `FIRST..FIRST + COUNT`).
    pub const DISPLAY_OBJECT_TYPE_ID_COUNT: u32 = 12;
    /// The first [`m_TypeId`](crate::ui::scaleform::Traits::m_TypeId) of the display-object AS3 builtin-class range.
    pub const DISPLAY_OBJECT_TYPE_ID_FIRST: u32 = 24;
    /// A [`m_Flags`](crate::ui::scaleform::Traits::m_Flags) bit marking a kind excluded from the display-object check.
    pub const FLAG_NON_DISPLAY_OBJECT: u8 = 32;
}
impl std::convert::AsRef<Traits> for Traits {
    fn as_ref(&self) -> &Traits {
        self
    }
}
impl std::convert::AsMut<Traits> for Traits {
    fn as_mut(&mut self) -> &mut Traits {
        self
    }
}
#[repr(C, align(1))]
/// A render-tree container node (`Render::TreeContainer`): a [`TreeNode`](crate::ui::scaleform::TreeNode) whose `NodeData` holds
/// a child `TreeNodeArray`. The display side mirrors every `DisplayObjContainer` into one, and
/// addresses children by cached numeric index (`GFx::DisplayList`'s `TreeIndex`), so removing a
/// child without the display list's knowledge shifts its siblings out of step with their cached
/// indices.
pub struct TreeContainer {
    _field_0: [u8; 56],
}
fn _TreeContainer_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], TreeContainer>([0u8; 0x38]);
    }
    unreachable!()
}
impl TreeContainer {
    pub const Insert_ADDRESS: usize = 0x1419E6720;
    /// Inserts `node` at `index`, recording
    /// [`CHANGE_CHILD_INSERT`](crate::ui::scaleform::TreeNode::CHANGE_CHILD_INSERT): refcounts the node, sets its
    /// [`pParent`](crate::ui::scaleform::TreeNode::pParent), and propagates. Capture (game update) thread only.
    pub unsafe fn Insert(
        &mut self,
        index: u64,
        node: *mut crate::ui::scaleform::TreeNode,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                index: u64,
                node: *mut crate::ui::scaleform::TreeNode,
            ) -> bool = ::std::mem::transmute(Self::Insert_ADDRESS);
            f(self as *mut Self as _, index, node)
        }
    }
    pub const Remove_ADDRESS: usize = 0x1419E6790;
    /// Removes `count` children starting at `index`, recording
    /// [`CHANGE_CHILD_REMOVE`](crate::ui::scaleform::TreeNode::CHANGE_CHILD_REMOVE): nulls each child's
    /// [`pParent`](crate::ui::scaleform::TreeNode::pParent) and drops its refcount (destroying it at zero). Capture
    /// (game update) thread only.
    pub unsafe fn Remove(&mut self, index: u64, count: u64) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, index: u64, count: u64) = ::std::mem::transmute(
                Self::Remove_ADDRESS,
            );
            f(self as *mut Self as _, index, count)
        }
    }
    pub const GetAt_ADDRESS: usize = 0x1419EA460;
    /// The child at `index` of the active snapshot's child array, or null out of range.
    pub unsafe fn GetAt(&self, index: u64) -> *mut crate::ui::scaleform::TreeNode {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                index: u64,
            ) -> *mut crate::ui::scaleform::TreeNode = ::std::mem::transmute(
                Self::GetAt_ADDRESS,
            );
            f(self as *const Self as _, index)
        }
    }
    pub const GetSize_ADDRESS: usize = 0x14197DA10;
    /// The active snapshot's child count.
    pub unsafe fn GetSize(&self) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> u64 = ::std::mem::transmute(
                Self::GetSize_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<TreeContainer> for TreeContainer {
    fn as_ref(&self) -> &TreeContainer {
        self
    }
}
impl std::convert::AsMut<TreeContainer> for TreeContainer {
    fn as_mut(&mut self) -> &mut TreeContainer {
        self
    }
}
#[repr(C, align(8))]
/// A Scaleform render-tree node: a `Render::ContextImpl::Entry` (a slot in a 0x1000-aligned entry
/// page) whose per-snapshot `NodeData` carries the transform, visibility, and bounds.
/// [`TreeContainer`](crate::ui::scaleform::TreeContainer) and [`TreeRoot`](crate::ui::scaleform::TreeRoot) share this header; a pointer to either casts to this
/// freely. Each write to an entry's `NodeData` records a `Render::TreeChangeConstants` change
/// bit (`Change_*`) on the entry.
pub struct TreeNode {
    _field_0: [u8; 8],
    /// The entry's reference count. [`TreeContainer::Insert`](crate::ui::scaleform::TreeContainer::Insert) increments it and
    /// [`TreeContainer::Remove`](crate::ui::scaleform::TreeContainer::Remove) decrements it (destroying the entry at zero), so a node moved
    /// between containers without an extra count held across the remove is destroyed.
    pub RefCount: u64,
    /// The displaying-snapshot `EntryData` the renderer reads (low bit carries a flag).
    pub pNative: *mut ::std::ffi::c_void,
    /// The renderer's `TreeCacheNode` for this entry, once drawn.
    pub pRenderer: *mut ::std::ffi::c_void,
    /// The parent container entry, or null while detached. [`TreeContainer::Remove`](crate::ui::scaleform::TreeContainer::Remove) nulls it;
    /// a null parent on a node the display side owned means the display list dropped it.
    pub pParent: *mut crate::ui::scaleform::TreeNode,
    _field_28: [u8; 16],
}
fn _TreeNode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], TreeNode>([0u8; 0x38]);
    }
    unreachable!()
}
impl TreeNode {
    pub const SetMatrix_ADDRESS: usize = 0x1419E7230;
    /// Writes the node's 2D transform (`Matrix2x4<float>`, 8 floats) through the entry-change
    /// protocol. Call on the capture (game update) thread.
    pub unsafe fn SetMatrix(&mut self, matrix: *const f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, matrix: *const f32) = ::std::mem::transmute(
                Self::SetMatrix_ADDRESS,
            );
            f(self as *mut Self as _, matrix)
        }
    }
    pub const DestroyHelper_ADDRESS: usize = 0x1419A6110;
    /// Destroys the entry (`Entry::destroyHelper`): queues its `NodeData` on the context's
    /// destroyed-nodes list (freed at the next capture) and frees the slot. Call only when
    /// [`RefCount`](crate::ui::scaleform::TreeNode::RefCount) reached zero, on the capture (game update) thread.
    pub unsafe fn DestroyHelper(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::DestroyHelper_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl TreeNode {
    /// The change bit for a child insertion (`Change_ChildInsert`).
    pub const CHANGE_CHILD_INSERT: u32 = 256;
    /// The change bit for a child removal (`Change_ChildRemove`).
    pub const CHANGE_CHILD_REMOVE: u32 = 512;
    /// The change bit for a viewport write (`Change_Viewport`).
    pub const CHANGE_VIEWPORT: u32 = 4096;
}
impl std::convert::AsRef<TreeNode> for TreeNode {
    fn as_ref(&self) -> &TreeNode {
        self
    }
}
impl std::convert::AsMut<TreeNode> for TreeNode {
    fn as_mut(&mut self) -> &mut TreeNode {
        self
    }
}
#[repr(C, align(1))]
/// A render-tree root (`Render::TreeRoot`): a [`TreeContainer`](crate::ui::scaleform::TreeContainer) whose `NodeData` adds the
/// viewport and background color. `HAL::Draw` takes one; the engine draws several per frame
/// through the same HAL (the UI movie, the off-screen movies, the debug text).
pub struct TreeRoot {
    _field_0: [u8; 56],
}
fn _TreeRoot_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], TreeRoot>([0u8; 0x38]);
    }
    unreachable!()
}
impl TreeRoot {
    pub const SetViewport_ADDRESS: usize = 0x1419E6860;
    /// Writes the root's viewport, recording [`CHANGE_VIEWPORT`](crate::ui::scaleform::TreeNode::CHANGE_VIEWPORT) only
    /// when the viewport differs. `viewport` is a [`Viewport`](crate::ui::scaleform::Viewport) (e.g. [`MovieImpl::Viewport`](crate::ui::scaleform::MovieImpl::Viewport));
    /// the `Render::Viewport` prefix, up to and including [`Flags`](crate::ui::scaleform::Viewport::Flags), is copied.
    /// Capture (game update) thread only.
    pub unsafe fn SetViewport(
        &mut self,
        viewport: *const crate::ui::scaleform::Viewport,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                viewport: *const crate::ui::scaleform::Viewport,
            ) = ::std::mem::transmute(Self::SetViewport_ADDRESS);
            f(self as *mut Self as _, viewport)
        }
    }
}
impl std::convert::AsRef<TreeRoot> for TreeRoot {
    fn as_ref(&self) -> &TreeRoot {
        self
    }
}
impl std::convert::AsMut<TreeRoot> for TreeRoot {
    fn as_mut(&mut self) -> &mut TreeRoot {
        self
    }
}
#[repr(C, align(8))]
/// A Scaleform `GFx::Value`: the tagged union the AS3 interface traffics in. Starts with a
/// `ListNode<Value>` (managed values are tracked on the movie's `ExternalObjRefs` list); a
/// stack-constructed unmanaged value leaves the list pointers and
/// [`pObjectInterface`](crate::ui::scaleform::Value::pObjectInterface) null.
pub struct Value {
    /// `ListNode<Value>::pPrev`; null for unmanaged values.
    pub pPrev: *mut crate::ui::scaleform::Value,
    /// `ListNode<Value>::pNext`; null for unmanaged values.
    pub pNext: *mut crate::ui::scaleform::Value,
    /// The owning movie's object interface; null for unmanaged values.
    pub pObjectInterface: *mut crate::ui::scaleform::ValueObjectInterface,
    /// The `ValueType` tag, possibly with `VTC_*` control bits (so not modeled as an enum).
    pub Type: u32,
    _field_1c: [u8; 4],
    /// The value union: `bool` / `i32` / `u32` / `f64` / `*const c_char` / object pointer,
    /// selected by [`Type`](crate::ui::scaleform::Value::Type).
    pub mValue: u64,
    /// Auxiliary data (e.g. the closure's user pointer).
    pub DataAux: u64,
}
fn _Value_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], Value>([0u8; 0x30]);
    }
    unreachable!()
}
impl Value {}
impl Value {
    /// The managed bit: set on values owned by the movie (which must be released through the
    /// object interface, never constructed by hand).
    pub const VTC_MANAGED_BIT: u32 = 64;
    /// The type tag for an AS3 `Array` (managed).
    pub const VT_ARRAY: u32 = 9;
    /// The type tag for a boolean value ([`mValue`](crate::ui::scaleform::Value::mValue) carries the `bool` in its
    /// first byte).
    pub const VT_BOOLEAN: u32 = 2;
    /// The type tag for a closure (managed): a method bound to its object.
    pub const VT_CLOSURE: u32 = 11;
    /// The type tag for a display-object value (managed; owned by the movie).
    pub const VT_DISPLAY_OBJECT: u32 = 10;
    /// The type tag for an int value.
    pub const VT_INT: u32 = 3;
    /// The type tag for a number (f64) value.
    pub const VT_NUMBER: u32 = 5;
    /// The type tag for an AS3 object that is neither an array nor a display object (managed).
    pub const VT_OBJECT: u32 = 8;
    /// The type tag for a string value (`mValue` is a `*const c_char`).
    pub const VT_STRING: u32 = 6;
    /// The mask isolating the base `VT_*` value-type tag in [`Type`](crate::ui::scaleform::Value::Type) from the `VTC_*`
    /// control bits (the game compares `Type & VT_TYPE_MASK` against the `VT_*` tags).
    pub const VT_TYPE_MASK: u32 = 143;
}
impl std::convert::AsRef<Value> for Value {
    fn as_ref(&self) -> &Value {
        self
    }
}
impl std::convert::AsMut<Value> for Value {
    fn as_mut(&mut self) -> &mut Value {
        self
    }
}
#[repr(C, align(1))]
/// The `GFx::Value::ObjectInterface`: the dispatcher for operations on managed values (display
/// objects, arrays). One per movie ([`MovieImpl::pObjectInterface`](crate::ui::scaleform::MovieImpl::pObjectInterface)); a managed [`Value`](crate::ui::scaleform::Value) carries
/// its owning interface in [`Value::pObjectInterface`](crate::ui::scaleform::Value::pObjectInterface).
pub struct ValueObjectInterface {
    _field_0: [u8; 8],
}
fn _ValueObjectInterface_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], ValueObjectInterface>([0u8; 0x8]);
    }
    unreachable!()
}
impl ValueObjectInterface {
    pub const GetDisplayInfo_ADDRESS: usize = 0x141BB8690;
    /// Reads a display object's [`DisplayInfo`](crate::ui::scaleform::DisplayInfo) (position, scale, alpha, visibility). `data` is the
    /// value's [`mValue`](crate::ui::scaleform::Value::mValue) payload. Returns false when the value is not a display
    /// object.
    pub unsafe fn GetDisplayInfo(
        &mut self,
        data: *mut ::std::ffi::c_void,
        info: *mut crate::ui::scaleform::DisplayInfo,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                data: *mut ::std::ffi::c_void,
                info: *mut crate::ui::scaleform::DisplayInfo,
            ) -> bool = ::std::mem::transmute(Self::GetDisplayInfo_ADDRESS);
            f(self as *mut Self as _, data, info)
        }
    }
    pub const SetDisplayInfo_ADDRESS: usize = 0x141BAEF00;
    /// Writes the `DisplayInfo` fields selected by [`DisplayInfo::VarsSet`](crate::ui::scaleform::DisplayInfo::VarsSet) directly at the
    /// display-object level -- no AVM path resolution and no AS3 property setters.
    /// `CHUDUI::UpdatePOIs` drives POI positions and visibility through it every frame.
    pub unsafe fn SetDisplayInfo(
        &mut self,
        data: *mut ::std::ffi::c_void,
        info: *const crate::ui::scaleform::DisplayInfo,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                data: *mut ::std::ffi::c_void,
                info: *const crate::ui::scaleform::DisplayInfo,
            ) -> bool = ::std::mem::transmute(Self::SetDisplayInfo_ADDRESS);
            f(self as *mut Self as _, data, info)
        }
    }
    pub const ObjectRelease_ADDRESS: usize = 0x141BC8D70;
    /// Releases a managed value: unlinks it from the movie's external-references list and drops
    /// the AS3 object reference. Call on the capture thread. `data` is the value's
    /// [`mValue`](crate::ui::scaleform::Value::mValue) payload.
    pub unsafe fn ObjectRelease(
        &mut self,
        value: *mut crate::ui::scaleform::Value,
        data: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                value: *mut crate::ui::scaleform::Value,
                data: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::ObjectRelease_ADDRESS);
            f(self as *mut Self as _, value, data)
        }
    }
    pub const ObjectAddRef_ADDRESS: usize = 0x141BB8560;
    /// Adds a reference to a managed value that is being copied into `value` (which must already
    /// carry the owning interface). `data` is the value's [`mValue`](crate::ui::scaleform::Value::mValue) payload.
    pub unsafe fn ObjectAddRef(
        &mut self,
        value: *mut crate::ui::scaleform::Value,
        data: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                value: *mut crate::ui::scaleform::Value,
                data: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::ObjectAddRef_ADDRESS);
            f(self as *mut Self as _, value, data)
        }
    }
    pub const GetMember_ADDRESS: usize = 0x141C4CE20;
    /// Reads the AS3 member `name` of the object `data` into `value`, releasing whatever `value`
    /// held. `is_display_object` is whether the object is a display object (its
    /// [`Type`](crate::ui::scaleform::Value::Type) is [`VT_DISPLAY_OBJECT`](crate::ui::scaleform::Value::VT_DISPLAY_OBJECT)); the engine
    /// passes that test inline. Returns false when the member is not found.
    pub unsafe fn GetMember(
        &self,
        data: *mut ::std::ffi::c_void,
        name: *const u8,
        value: *mut crate::ui::scaleform::Value,
        is_display_object: bool,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                data: *mut ::std::ffi::c_void,
                name: *const u8,
                value: *mut crate::ui::scaleform::Value,
                is_display_object: bool,
            ) -> bool = ::std::mem::transmute(Self::GetMember_ADDRESS);
            f(self as *const Self as _, data, name, value, is_display_object)
        }
    }
    pub const SetMember_ADDRESS: usize = 0x141C45B00;
    /// Writes `value` to the AS3 member `name` of the object `data`. `is_display_object` as for
    /// [`GetMember`](crate::ui::scaleform::ValueObjectInterface::GetMember).
    pub unsafe fn SetMember(
        &mut self,
        data: *mut ::std::ffi::c_void,
        name: *const u8,
        value: *const crate::ui::scaleform::Value,
        is_display_object: bool,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                data: *mut ::std::ffi::c_void,
                name: *const u8,
                value: *const crate::ui::scaleform::Value,
                is_display_object: bool,
            ) -> bool = ::std::mem::transmute(Self::SetMember_ADDRESS);
            f(self as *mut Self as _, data, name, value, is_display_object)
        }
    }
    pub const Invoke_ADDRESS: usize = 0x141C4D380;
    /// Calls the AS3 method `name` of the object `data` with `num_args` arguments, storing the
    /// return value in `result` when it is not null. `is_display_object` as for
    /// [`GetMember`](crate::ui::scaleform::ValueObjectInterface::GetMember). Runs the method to completion before returning; call on
    /// the capture thread.
    pub unsafe fn Invoke(
        &mut self,
        data: *mut ::std::ffi::c_void,
        result: *mut crate::ui::scaleform::Value,
        name: *const u8,
        args: *const crate::ui::scaleform::Value,
        num_args: u64,
        is_display_object: bool,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                data: *mut ::std::ffi::c_void,
                result: *mut crate::ui::scaleform::Value,
                name: *const u8,
                args: *const crate::ui::scaleform::Value,
                num_args: u64,
                is_display_object: bool,
            ) -> bool = ::std::mem::transmute(Self::Invoke_ADDRESS);
            f(
                self as *mut Self as _,
                data,
                result,
                name,
                args,
                num_args,
                is_display_object,
            )
        }
    }
    pub const PushBack_ADDRESS: usize = 0x141BE3820;
    /// Appends `value` to the AS3 array `data`.
    pub unsafe fn PushBack(
        &mut self,
        data: *mut ::std::ffi::c_void,
        value: *const crate::ui::scaleform::Value,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                data: *mut ::std::ffi::c_void,
                value: *const crate::ui::scaleform::Value,
            ) -> bool = ::std::mem::transmute(Self::PushBack_ADDRESS);
            f(self as *mut Self as _, data, value)
        }
    }
    pub const RemoveElements_ADDRESS: usize = 0x141BA0E30;
    /// Removes `count` elements of the AS3 array `data` starting at `index`; a negative `count`
    /// removes through the end.
    pub unsafe fn RemoveElements(
        &mut self,
        data: *mut ::std::ffi::c_void,
        index: u32,
        count: i32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                data: *mut ::std::ffi::c_void,
                index: u32,
                count: i32,
            ) -> bool = ::std::mem::transmute(Self::RemoveElements_ADDRESS);
            f(self as *mut Self as _, data, index, count)
        }
    }
    pub const GetElement_ADDRESS: usize = 0x141C4E830;
    /// Reads element `index` of the AS3 array `data` into `value`, releasing whatever `value`
    /// held. Returns false past the end.
    pub unsafe fn GetElement(
        &self,
        data: *mut ::std::ffi::c_void,
        index: u32,
        value: *mut crate::ui::scaleform::Value,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                data: *mut ::std::ffi::c_void,
                index: u32,
                value: *mut crate::ui::scaleform::Value,
            ) -> bool = ::std::mem::transmute(Self::GetElement_ADDRESS);
            f(self as *const Self as _, data, index, value)
        }
    }
    pub const SetElement_ADDRESS: usize = 0x141BE3720;
    /// Writes `value` to element `index` of the AS3 array `data`, growing the array as needed.
    pub unsafe fn SetElement(
        &mut self,
        data: *mut ::std::ffi::c_void,
        index: u32,
        value: *const crate::ui::scaleform::Value,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                data: *mut ::std::ffi::c_void,
                index: u32,
                value: *const crate::ui::scaleform::Value,
            ) -> bool = ::std::mem::transmute(Self::SetElement_ADDRESS);
            f(self as *mut Self as _, data, index, value)
        }
    }
    pub const GetArraySize_ADDRESS: usize = 0x141B921B0;
    /// The length of the AS3 array `data`.
    pub unsafe fn GetArraySize(&self, data: *mut ::std::ffi::c_void) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                data: *mut ::std::ffi::c_void,
            ) -> u32 = ::std::mem::transmute(Self::GetArraySize_ADDRESS);
            f(self as *const Self as _, data)
        }
    }
}
impl std::convert::AsRef<ValueObjectInterface> for ValueObjectInterface {
    fn as_ref(&self) -> &ValueObjectInterface {
        self
    }
}
impl std::convert::AsMut<ValueObjectInterface> for ValueObjectInterface {
    fn as_mut(&mut self) -> &mut ValueObjectInterface {
        self
    }
}
#[derive(Copy, Clone, Default)]
#[repr(C, align(4))]
/// A Scaleform `GFx::Viewport` (a `Render::Viewport` plus `Scale` and `AspectRatio`): the
/// description [`MovieImpl::SetViewportImpl`](crate::ui::scaleform::MovieImpl::SetViewportImpl) copies into [`MovieImpl::Viewport`](crate::ui::scaleform::MovieImpl::Viewport). `BufferWidth`
/// and `BufferHeight` describe the whole output surface; `Left`/`Top`/`Width`/`Height` are the
/// view rectangle the movie occupies within it. `CUIManager::SetMovieViewport` builds one as
/// `{BufferWidth: vw, BufferHeight: vh, Left: (vw - movie_w) / 2, Top: (vh - movie_h) / 2,
/// Width: movie_w, Height: movie_h, Scissor*: 0, Flags: 0, Scale: 1.0, AspectRatio: 1.0}`, where
/// `movie_w`/`movie_h` are [`m_MovieScaleWidth`](crate::ui::ui_manager::UIManager::m_MovieScaleWidth)/`Height` at call time (not the
/// arguments).
pub struct Viewport {
    pub BufferWidth: i32,
    pub BufferHeight: i32,
    pub Left: i32,
    pub Top: i32,
    pub Width: i32,
    pub Height: i32,
    pub ScissorLeft: i32,
    pub ScissorTop: i32,
    pub ScissorWidth: i32,
    pub ScissorHeight: i32,
    /// `View_*` flags, among them [`VIEW_IS_RENDER_TEXTURE`](crate::ui::scaleform::Viewport::VIEW_IS_RENDER_TEXTURE),
    /// [`VIEW_USE_SCISSOR_RECT`](crate::ui::scaleform::Viewport::VIEW_USE_SCISSOR_RECT), and the
    /// [`VIEW_ORIENTATION_MASK`](crate::ui::scaleform::Viewport::VIEW_ORIENTATION_MASK) bits.
    pub Flags: u32,
    /// The view scale used by the `SM_NO_SCALE` mode; every engine caller passes 1.0.
    pub Scale: f32,
    /// The pixel aspect used as an extra horizontal factor by [`MovieImpl::UpdateViewport`](crate::ui::scaleform::MovieImpl::UpdateViewport);
    /// every engine caller passes 1.0.
    pub AspectRatio: f32,
}
fn _Viewport_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x34], Viewport>([0u8; 0x34]);
    }
    unreachable!()
}
impl Viewport {}
impl Viewport {
    /// The [`Flags`](crate::ui::scaleform::Viewport::Flags) bit for a render-texture target (`View_IsRenderTexture`).
    pub const VIEW_IS_RENDER_TEXTURE: u32 = 1;
    /// The [`Flags`](crate::ui::scaleform::Viewport::Flags) mask for the orientation (`View_Orientation_Mask`).
    pub const VIEW_ORIENTATION_MASK: u32 = 48;
    /// The [`Flags`](crate::ui::scaleform::Viewport::Flags) bit that enables the scissor rectangle
    /// (`View_UseScissorRect`).
    pub const VIEW_USE_SCISSOR_RECT: u32 = 4;
}
impl std::convert::AsRef<Viewport> for Viewport {
    fn as_ref(&self) -> &Viewport {
        self
    }
}
impl std::convert::AsMut<Viewport> for Viewport {
    fn as_mut(&mut self) -> &mut Viewport {
        self
    }
}
impl Traits {
    pub fn is_display_object(&self) -> bool {
        self.m_TypeId.wrapping_sub(Traits::DISPLAY_OBJECT_TYPE_ID_FIRST)
            < Traits::DISPLAY_OBJECT_TYPE_ID_COUNT
            && self.m_Flags & Traits::FLAG_NON_DISPLAY_OBJECT == 0
    }
}
#[allow(dead_code)]
impl Value {
    pub fn value_type(&self) -> u32 {
        self.Type & Value::VT_TYPE_MASK
    }
    /// The boolean payload, or `None` if this is not a boolean value.
    pub fn as_bool(&self) -> Option<bool> {
        (self.value_type() == Value::VT_BOOLEAN).then_some(self.mValue & 0xFF != 0)
    }
    /// The 32-bit integer payload, or `None` if this is not an int value.
    pub fn as_int(&self) -> Option<i32> {
        (self.value_type() == Value::VT_INT).then_some(self.mValue as i32)
    }
    /// The `f64` payload, or `None` if this is not a number value.
    pub fn as_number(&self) -> Option<f64> {
        (self.value_type() == Value::VT_NUMBER).then_some(f64::from_bits(self.mValue))
    }
    /// The NUL-terminated string payload, or `None` if this is not a string value.
    pub fn as_string(&self) -> Option<*const ::std::ffi::c_char> {
        (self.value_type() == Value::VT_STRING)
            .then_some(self.mValue as *const ::std::ffi::c_char)
    }
    /// The managed display object payload, or `None` if this is not a display-object value.
    pub fn as_display_object(&self) -> Option<*mut Object> {
        (self.value_type() == Value::VT_DISPLAY_OBJECT)
            .then_some(self.mValue as *mut Object)
    }
}
#[allow(dead_code)]
impl Value {
    /// An unmanaged boolean value, safe to pass to [`SetVariable`](Movie::SetVariable) (the movie copies it;
    /// nothing needs releasing).
    pub fn new_boolean(value: bool) -> Self {
        let mut v: Self = unsafe { ::core::mem::zeroed() };
        v.Type = Self::VT_BOOLEAN;
        v.mValue = value as u64;
        v
    }
}
impl StringDataDesc {
    pub fn as_ptr(&self) -> *const ::std::ffi::c_char {
        ::std::ptr::addr_of!(self.m_Data)
    }
}
#[allow(dead_code)]
impl AmpMovieObjectDesc {
    /// The instance name's [`StringDataDesc`], with the heap flags masked off the string pointer's
    /// low 3 bits, or `None` if the name is null.
    ///
    /// # Safety
    /// The descriptor must outlive the borrow (true while the node is alive).
    pub unsafe fn name_data(&self) -> Option<&StringDataDesc> {
        unsafe { ((self.name as usize & !7) as *const StringDataDesc).as_ref() }
    }
}
