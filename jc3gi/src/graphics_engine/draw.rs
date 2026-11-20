#![cfg_attr(any(), rustfmt::skip)]
pub type PixelShaderPtr = Option<
    windows::Win32::Graphics::Direct3D11::ID3D11PixelShader,
>;
pub type HullShaderPtr = Option<windows::Win32::Graphics::Direct3D11::ID3D11HullShader>;
pub type DomainShaderPtr = Option<
    windows::Win32::Graphics::Direct3D11::ID3D11DomainShader,
>;
crate::__bitflags! {
    #[doc =
    " The `flags` argument of [`Clear`](crate::graphics_engine::draw::Clear) (`Graphics::EClearFlags`): which targets of the bound render"]
    #[doc =
    " setup to clear. [`Clear`](crate::graphics_engine::draw::Clear) clears the colour targets for any of the channel bits and the"]
    #[doc = " depth-stencil view for the depth and stencil bits."] pub struct ClearFlags
    : u32 { const CLEAR_DEPTHBUFFER = 1usize as _; const CLEAR_STENCILBUFFER = 2usize as
    _; const CLEAR_RENDERBUFFER_R = 4usize as _; const CLEAR_RENDERBUFFER_G = 8usize as
    _; const CLEAR_RENDERBUFFER_B = 16usize as _; const CLEAR_RENDERBUFFER_A = 32usize as
    _; }
}
fn _ClearFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ClearFlags>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// Parameters to [`CreateDomainProgram`](crate::graphics_engine::draw::CreateDomainProgram): the compiled DXBC bytecode, its byte length, and a debug
/// name. Same layout as [`CreateVertexProgramParams`](crate::graphics_engine::draw::CreateVertexProgramParams).
pub struct CreateDomainProgramParams {
    pub m_Code: *const u8,
    pub m_Size: u64,
    pub m_Name: *const u8,
}
fn _CreateDomainProgramParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], CreateDomainProgramParams>([0u8; 0x18]);
    }
    unreachable!()
}
impl CreateDomainProgramParams {}
impl std::convert::AsRef<CreateDomainProgramParams> for CreateDomainProgramParams {
    fn as_ref(&self) -> &CreateDomainProgramParams {
        self
    }
}
impl std::convert::AsMut<CreateDomainProgramParams> for CreateDomainProgramParams {
    fn as_mut(&mut self) -> &mut CreateDomainProgramParams {
        self
    }
}
#[repr(C, align(8))]
/// Parameters to [`CreateFragmentProgram`](crate::graphics_engine::draw::CreateFragmentProgram): the compiled DXBC bytecode (`m_Code`) and its byte length
/// (`m_Size`), passed straight through to `ID3D11Device::CreatePixelShader`. `m_Size` is read as a
/// pointer-width value (the bytecode length argument to `CreatePixelShader`).
pub struct CreateFragmentProgramParams {
    pub m_Code: *const u8,
    pub m_Size: u64,
}
fn _CreateFragmentProgramParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], CreateFragmentProgramParams>([0u8; 0x10]);
    }
    unreachable!()
}
impl CreateFragmentProgramParams {}
impl std::convert::AsRef<CreateFragmentProgramParams> for CreateFragmentProgramParams {
    fn as_ref(&self) -> &CreateFragmentProgramParams {
        self
    }
}
impl std::convert::AsMut<CreateFragmentProgramParams> for CreateFragmentProgramParams {
    fn as_mut(&mut self) -> &mut CreateFragmentProgramParams {
        self
    }
}
#[repr(C, align(8))]
/// Parameters to [`CreateHullProgram`](crate::graphics_engine::draw::CreateHullProgram): the compiled DXBC bytecode, its byte length, and a debug name.
/// Same layout as [`CreateVertexProgramParams`](crate::graphics_engine::draw::CreateVertexProgramParams).
pub struct CreateHullProgramParams {
    pub m_Code: *const u8,
    pub m_Size: u64,
    pub m_Name: *const u8,
}
fn _CreateHullProgramParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], CreateHullProgramParams>([0u8; 0x18]);
    }
    unreachable!()
}
impl CreateHullProgramParams {}
impl std::convert::AsRef<CreateHullProgramParams> for CreateHullProgramParams {
    fn as_ref(&self) -> &CreateHullProgramParams {
        self
    }
}
impl std::convert::AsMut<CreateHullProgramParams> for CreateHullProgramParams {
    fn as_mut(&mut self) -> &mut CreateHullProgramParams {
        self
    }
}
#[repr(C, align(8))]
/// Parameters to [`CreateVertexProgram`](crate::graphics_engine::draw::CreateVertexProgram): the compiled DXBC bytecode, its byte length, and a debug
/// name (attached to the D3D object and used to size the retained bytecode copy).
pub struct CreateVertexProgramParams {
    pub m_Code: *const u8,
    pub m_Size: u64,
    pub m_Name: *const u8,
}
fn _CreateVertexProgramParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], CreateVertexProgramParams>([0u8; 0x18]);
    }
    unreachable!()
}
impl CreateVertexProgramParams {}
impl std::convert::AsRef<CreateVertexProgramParams> for CreateVertexProgramParams {
    fn as_ref(&self) -> &CreateVertexProgramParams {
        self
    }
}
impl std::convert::AsMut<CreateVertexProgramParams> for CreateVertexProgramParams {
    fn as_mut(&mut self) -> &mut CreateVertexProgramParams {
        self
    }
}
#[repr(C, align(8))]
pub struct HDomainProgram_t {
    pub m_Program: crate::graphics_engine::draw::DomainShaderPtr,
}
fn _HDomainProgram_t_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], HDomainProgram_t>([0u8; 0x8]);
    }
    unreachable!()
}
impl HDomainProgram_t {}
impl std::convert::AsRef<HDomainProgram_t> for HDomainProgram_t {
    fn as_ref(&self) -> &HDomainProgram_t {
        self
    }
}
impl std::convert::AsMut<HDomainProgram_t> for HDomainProgram_t {
    fn as_mut(&mut self) -> &mut HDomainProgram_t {
        self
    }
}
#[repr(C, align(8))]
/// The fragment program returned by [`CreateFragmentProgram`](crate::graphics_engine::draw::CreateFragmentProgram).
pub struct HFragmentProgram_t {
    /// The pixel shader written by `ID3D11Device::CreatePixelShader`, or null without a program.
    pub m_Program: crate::graphics_engine::draw::PixelShaderPtr,
}
fn _HFragmentProgram_t_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], HFragmentProgram_t>([0u8; 0x8]);
    }
    unreachable!()
}
impl HFragmentProgram_t {}
impl std::convert::AsRef<HFragmentProgram_t> for HFragmentProgram_t {
    fn as_ref(&self) -> &HFragmentProgram_t {
        self
    }
}
impl std::convert::AsMut<HFragmentProgram_t> for HFragmentProgram_t {
    fn as_mut(&mut self) -> &mut HFragmentProgram_t {
        self
    }
}
#[repr(C, align(8))]
pub struct HHullProgram_t {
    pub m_Program: crate::graphics_engine::draw::HullShaderPtr,
}
fn _HHullProgram_t_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], HHullProgram_t>([0u8; 0x8]);
    }
    unreachable!()
}
impl HHullProgram_t {}
impl std::convert::AsRef<HHullProgram_t> for HHullProgram_t {
    fn as_ref(&self) -> &HHullProgram_t {
        self
    }
}
impl std::convert::AsMut<HHullProgram_t> for HHullProgram_t {
    fn as_mut(&mut self) -> &mut HHullProgram_t {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The primitive topology passed to the draw wrappers. The patchlist variants are tessellation
/// control-point counts.
pub enum PrimitiveType {
    PRIMTYPE_POINTLIST = 1isize as _,
    PRIMTYPE_LINES = 2isize as _,
    PRIMTYPE_LINE_STRIP = 3isize as _,
    PRIMTYPE_TRIANGLES = 4isize as _,
    PRIMTYPE_TRIANGLE_STRIP = 5isize as _,
    PRIMTYPE_LINE_LOOP = 6isize as _,
    PRIMTYPE_TRIANGLE_FAN = 7isize as _,
    PRIMTYPE_PATCHLIST_1 = 33isize as _,
    PRIMTYPE_PATCHLIST_2 = 34isize as _,
    PRIMTYPE_PATCHLIST_3 = 35isize as _,
    PRIMTYPE_PATCHLIST_4 = 36isize as _,
}
fn _PrimitiveType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], PrimitiveType>([0u8; 0x4]);
    }
    unreachable!()
}
/// The constant-buffer size classes (`CB_SIZES`), in `float4` rows and ascending.
/// [`SetFragmentProgramConstantBufferSize`](crate::graphics_engine::draw::SetFragmentProgramConstantBufferSize) and [`SetVertexProgramConstantBufferSize`](crate::graphics_engine::draw::SetVertexProgramConstantBufferSize) round a
/// requested row count up to the first entry at or above it, and the context pre-creates one pooled
/// `ID3D11Buffer` per slot and size class.
pub unsafe fn get_constant_buffer_size_classes() -> &'static mut [u32; 17] {
    unsafe { &mut *(0x1425F9300 as *mut [u32; 17]) }
}
pub const DrawIndexed_ADDRESS: usize = 0x141967720;
/// An indexed draw.
pub unsafe fn DrawIndexed(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    prim: crate::graphics_engine::draw::PrimitiveType,
    arg2: i32,
    arg3: i32,
    vbuf: *mut ::std::ffi::c_void,
    ibuf: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            prim: crate::graphics_engine::draw::PrimitiveType,
            arg2: i32,
            arg3: i32,
            vbuf: *mut ::std::ffi::c_void,
            ibuf: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(DrawIndexed_ADDRESS);
        f(ctx, prim, arg2, arg3, vbuf, ibuf)
    }
}
pub const Draw_ADDRESS: usize = 0x141967680;
/// A non-indexed draw.
pub unsafe fn Draw(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    prim: crate::graphics_engine::draw::PrimitiveType,
    arg2: i32,
    arg3: i32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            prim: crate::graphics_engine::draw::PrimitiveType,
            arg2: i32,
            arg3: i32,
        ) = ::std::mem::transmute(Draw_ADDRESS);
        f(ctx, prim, arg2, arg3)
    }
}
pub const DrawInstanced_ADDRESS: usize = 0x141962CC0;
/// A non-indexed instanced draw: takes the context mutex, flushes the pending render state, and
/// issues `ID3D11DeviceContext::DrawInstanced` (vtable slot 21).
pub unsafe fn DrawInstanced(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    first_vertex: u32,
    vertex_count_per_instance: i32,
    first_instance: u32,
    instance_count: i32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            first_vertex: u32,
            vertex_count_per_instance: i32,
            first_instance: u32,
            instance_count: i32,
        ) = ::std::mem::transmute(DrawInstanced_ADDRESS);
        f(ctx, first_vertex, vertex_count_per_instance, first_instance, instance_count)
    }
}
pub const DrawIndexedWithBoundState_ADDRESS: usize = 0x141962DA0;
/// The `(context, first_index, index_count)` overload of `Graphics::DrawIndexed`, which draws with the
/// already-bound primitive type and index buffer: takes the context mutex, flushes the pending render
/// state, and issues `ID3D11DeviceContext::DrawIndexed` (vtable slot 12). The overload that also sets
/// the primitive type and buffers is [`DrawIndexed`](crate::graphics_engine::draw::DrawIndexed).
pub unsafe fn DrawIndexedWithBoundState(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    first_index: u32,
    index_count: i32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            first_index: u32,
            index_count: i32,
        ) = ::std::mem::transmute(DrawIndexedWithBoundState_ADDRESS);
        f(ctx, first_index, index_count)
    }
}
pub const DrawIndexedNoMutex_ADDRESS: usize = 0x141962D50;
/// An indexed draw without the context mutex: flushes the pending render state and issues
/// `ID3D11DeviceContext::DrawIndexed` (vtable slot 12).
pub unsafe fn DrawIndexedNoMutex(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    first_index: u32,
    index_count: i32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            first_index: u32,
            index_count: i32,
        ) = ::std::mem::transmute(DrawIndexedNoMutex_ADDRESS);
        f(ctx, first_index, index_count)
    }
}
pub const DrawIndexedInstancedNoMutex_ADDRESS: usize = 0x141962E20;
/// An indexed instanced draw without the context mutex: flushes the pending render state and issues
/// `ID3D11DeviceContext::DrawIndexedInstanced` (vtable slot 20).
pub unsafe fn DrawIndexedInstancedNoMutex(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    first_index: u32,
    index_count_per_instance: i32,
    instance_count: i32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            first_index: u32,
            index_count_per_instance: i32,
            instance_count: i32,
        ) = ::std::mem::transmute(DrawIndexedInstancedNoMutex_ADDRESS);
        f(ctx, first_index, index_count_per_instance, instance_count)
    }
}
pub const DrawIndexedInstanced_ADDRESS: usize = 0x141962E80;
/// An indexed instanced draw: takes the context mutex, flushes the pending render state, and issues
/// `ID3D11DeviceContext::DrawIndexedInstanced` (vtable slot 20).
pub unsafe fn DrawIndexedInstanced(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    first_index: u32,
    index_count_per_instance: u32,
    instance_count: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            first_index: u32,
            index_count_per_instance: u32,
            instance_count: u32,
        ) = ::std::mem::transmute(DrawIndexedInstanced_ADDRESS);
        f(ctx, first_index, index_count_per_instance, instance_count)
    }
}
pub const DrawIndexedInstancedIndirectNoMutex_ADDRESS: usize = 0x141962F10;
/// A GPU-driven indexed instanced draw without the context mutex: flushes the pending render state
/// and issues `ID3D11DeviceContext::DrawIndexedInstancedIndirect` (vtable slot 39) with the argument
/// buffer of `draw_params` at byte `offset`.
pub unsafe fn DrawIndexedInstancedIndirectNoMutex(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    draw_params: *mut ::std::ffi::c_void,
    offset: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            draw_params: *mut ::std::ffi::c_void,
            offset: u32,
        ) = ::std::mem::transmute(DrawIndexedInstancedIndirectNoMutex_ADDRESS);
        f(ctx, draw_params, offset)
    }
}
pub const DrawIndexedInstancedIndirect_ADDRESS: usize = 0x141962F60;
/// A GPU-driven indexed instanced draw: takes the context mutex, flushes the pending render state,
/// and issues `ID3D11DeviceContext::DrawIndexedInstancedIndirect` (vtable slot 39) with the argument
/// buffer of `draw_params` at byte `offset`.
pub unsafe fn DrawIndexedInstancedIndirect(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    draw_params: *mut ::std::ffi::c_void,
    offset: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            draw_params: *mut ::std::ffi::c_void,
            offset: u32,
        ) = ::std::mem::transmute(DrawIndexedInstancedIndirect_ADDRESS);
        f(ctx, draw_params, offset)
    }
}
pub const DrawInstancedIndirect_ADDRESS: usize = 0x141962FF0;
/// A GPU-driven non-indexed instanced draw: takes the context mutex, flushes the pending render
/// state, and issues `ID3D11DeviceContext::DrawInstancedIndirect` (vtable slot 40) with the argument
/// buffer of `draw_params` at byte `offset`.
pub unsafe fn DrawInstancedIndirect(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    draw_params: *mut ::std::ffi::c_void,
    offset: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            draw_params: *mut ::std::ffi::c_void,
            offset: u32,
        ) = ::std::mem::transmute(DrawInstancedIndirect_ADDRESS);
        f(ctx, draw_params, offset)
    }
}
pub const DrawIndexedInstancedMultiIndirect_ADDRESS: usize = 0x141963080;
/// `draw_count` GPU-driven indexed instanced draws under one hold of the context mutex: issues
/// `ID3D11DeviceContext::DrawIndexedInstancedIndirect` (vtable slot 39) once per draw, advancing the
/// argument offset by one 20-byte argument record each time.
pub unsafe fn DrawIndexedInstancedMultiIndirect(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    draw_count: u32,
    draw_params: *mut ::std::ffi::c_void,
    offset: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            draw_count: u32,
            draw_params: *mut ::std::ffi::c_void,
            offset: u32,
        ) = ::std::mem::transmute(DrawIndexedInstancedMultiIndirect_ADDRESS);
        f(ctx, draw_count, draw_params, offset)
    }
}
pub const Dispatch_ADDRESS: usize = 0x141962AD0;
/// A compute dispatch of `x` by `y` by `z` thread groups.
///
/// Unlike its neighbours above, this one's demangled symbol agrees with the disassembly: the body
/// takes exactly four arguments in `rcx`/`edx`/`r8d`/`r9d`, flushes the pending render state through
/// `Graphics::SetupRenderStates`, and forwards the three group counts to the backend's dispatch entry
/// under the context's critical section.
///
/// A dispatch is not rasterization, so none of the rasterizer state applies to it: neither the
/// viewport nor the scissor rectangle ([`SetScissorEnable`](crate::graphics_engine::draw::SetScissorEnable)) restricts which texels a dispatch's
/// threads address. The addressed region is decided entirely by the group counts passed here and by
/// the compute program's own mapping from `SV_DispatchThreadID` to texel.
pub unsafe fn Dispatch(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    x: u32,
    y: u32,
    z: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            x: u32,
            y: u32,
            z: u32,
        ) = ::std::mem::transmute(Dispatch_ADDRESS);
        f(ctx, x, y, z)
    }
}
pub const DispatchIndirect_ADDRESS: usize = 0x141962B60;
/// A GPU-driven compute dispatch.
pub unsafe fn DispatchIndirect(
    a1: *mut ::std::ffi::c_void,
    a2: *mut ::std::ffi::c_void,
    a3: *mut ::std::ffi::c_void,
    a4: *mut ::std::ffi::c_void,
    a5: *mut ::std::ffi::c_void,
    a6: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            a1: *mut ::std::ffi::c_void,
            a2: *mut ::std::ffi::c_void,
            a3: *mut ::std::ffi::c_void,
            a4: *mut ::std::ffi::c_void,
            a5: *mut ::std::ffi::c_void,
            a6: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(DispatchIndirect_ADDRESS);
        f(a1, a2, a3, a4, a5, a6)
    }
}
pub const AcquireThreadOwnership_ADDRESS: usize = 0x141954CD0;
/// Enters the context's critical section, if it has one. The wrappers without `NoMutex` in their
/// name take and leave it around their one call into Direct3D; a render block that issues several
/// calls in a row takes it once here and uses the `NoMutex` variants.
pub unsafe fn AcquireThreadOwnership(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        ) = ::std::mem::transmute(AcquireThreadOwnership_ADDRESS);
        f(ctx)
    }
}
pub const ReleaseThreadOwnership_ADDRESS: usize = 0x141954CF0;
/// Leaves the context's critical section that [`AcquireThreadOwnership`](crate::graphics_engine::draw::AcquireThreadOwnership) entered.
pub unsafe fn ReleaseThreadOwnership(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        ) = ::std::mem::transmute(ReleaseThreadOwnership_ADDRESS);
        f(ctx)
    }
}
pub const SetVertexProgramConstantBufferNoMutex_ADDRESS: usize = 0x141963DF0;
/// Binds a constant buffer to a vertex-program slot without the context mutex: issues
/// `ID3D11DeviceContext::VSSetConstantBuffers` (vtable slot 7) for the one slot, and for the first
/// four slots clears the context's record of the staged constants for that slot.
pub unsafe fn SetVertexProgramConstantBufferNoMutex(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    slot: i32,
    buffer: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            slot: i32,
            buffer: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(SetVertexProgramConstantBufferNoMutex_ADDRESS);
        f(ctx, slot, buffer)
    }
}
pub const SetVertexStreamNoMutex_ADDRESS: usize = 0x141963260;
/// Binds a vertex buffer to a stream without the context mutex: issues
/// `ID3D11DeviceContext::IASetVertexBuffers` (vtable slot 18) for the one stream, with the buffer's
/// current Direct3D buffer and the given offset and stride.
pub unsafe fn SetVertexStreamNoMutex(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    buffer: *mut ::std::ffi::c_void,
    stream: i32,
    offset: i32,
    stride: i32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            buffer: *mut ::std::ffi::c_void,
            stream: i32,
            offset: i32,
            stride: i32,
        ) = ::std::mem::transmute(SetVertexStreamNoMutex_ADDRESS);
        f(ctx, buffer, stream, offset, stride)
    }
}
pub const LockConstantBufferNoMutex_ADDRESS: usize = 0x141963C00;
/// Maps a constant buffer for writing without the context mutex: issues `ID3D11DeviceContext::Map`
/// (vtable slot 14) on the buffer's resource with `D3D11_MAP_WRITE_DISCARD`, whatever `lock_type`
/// asks, and returns the mapped pointer.
pub unsafe fn LockConstantBufferNoMutex(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    buffer: *mut ::std::ffi::c_void,
    lock_type: i32,
) -> *mut ::std::ffi::c_void {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            buffer: *mut ::std::ffi::c_void,
            lock_type: i32,
        ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(
            LockConstantBufferNoMutex_ADDRESS,
        );
        f(ctx, buffer, lock_type)
    }
}
pub const SetRenderSetup_ADDRESS: usize = 0x141966D20;
/// Binds a render setup, the render-target configuration a pass draws into.
pub unsafe fn SetRenderSetup(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    setup: *mut ::std::ffi::c_void,
    restore: bool,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            setup: *mut ::std::ffi::c_void,
            restore: bool,
        ) = ::std::mem::transmute(SetRenderSetup_ADDRESS);
        f(ctx, setup, restore)
    }
}
pub const Clear_ADDRESS: usize = 0x141967020;
/// Clears the currently-bound render setup. `flags` is a [`ClearFlags`](crate::graphics_engine::draw::ClearFlags) mask; `color` is a 4-float
/// RGBA pointer and may be null.
pub unsafe fn Clear(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    flags: u32,
    color: *mut ::std::ffi::c_void,
    depth: f32,
    stencil: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            flags: u32,
            color: *mut ::std::ffi::c_void,
            depth: f32,
            stencil: u32,
        ) = ::std::mem::transmute(Clear_ADDRESS);
        f(ctx, flags, color, depth, stencil)
    }
}
pub const SetStencilTestEnable_ADDRESS: usize = 0x141966730;
/// Stages the stencil-test enable into [`HContext_t::m_DepthStencilStateIndex`](crate::graphics_engine::graphics_engine::HContext_t::m_DepthStencilStateIndex); applied
/// at the next draw's state flush.
pub unsafe fn SetStencilTestEnable(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    enable: i32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            enable: i32,
        ) = ::std::mem::transmute(SetStencilTestEnable_ADDRESS);
        f(ctx, enable)
    }
}
pub const SetStencilFunc_ADDRESS: usize = 0x141966680;
/// Stages the stencil comparison function and read mask into
/// [`HContext_t::m_DepthStencilStateIndex`](crate::graphics_engine::graphics_engine::HContext_t::m_DepthStencilStateIndex) and the reference value into
/// [`HContext_t::m_StencilRef`](crate::graphics_engine::graphics_engine::HContext_t::m_StencilRef).
pub unsafe fn SetStencilFunc(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    func: i32,
    reference: u32,
    mask: u8,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            func: i32,
            reference: u32,
            mask: u8,
        ) = ::std::mem::transmute(SetStencilFunc_ADDRESS);
        f(ctx, func, reference, mask)
    }
}
pub const SetStencilMask_ADDRESS: usize = 0x1419666B0;
/// Stages the stencil write mask into [`HContext_t::m_DepthStencilStateIndex`](crate::graphics_engine::graphics_engine::HContext_t::m_DepthStencilStateIndex).
pub unsafe fn SetStencilMask(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    write_mask: u8,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            write_mask: u8,
        ) = ::std::mem::transmute(SetStencilMask_ADDRESS);
        f(ctx, write_mask)
    }
}
pub const SetStencilOp_ADDRESS: usize = 0x1419666F0;
/// Stages the stencil fail, depth-fail, and pass operations into
/// [`HContext_t::m_DepthStencilStateIndex`](crate::graphics_engine::graphics_engine::HContext_t::m_DepthStencilStateIndex).
pub unsafe fn SetStencilOp(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    fail: i32,
    depth_fail: i32,
    depth_pass: i32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            fail: i32,
            depth_fail: i32,
            depth_pass: i32,
        ) = ::std::mem::transmute(SetStencilOp_ADDRESS);
        f(ctx, fail, depth_fail, depth_pass)
    }
}
pub const CopySurfaceToTexture_ADDRESS: usize = 0x141954850;
/// Copies one surface into another texture: a whole-resource `ID3D11DeviceContext::CopyResource`
/// of `src`'s D3D11 resource into `dst`'s, issued under the context mutex. IDA
/// misplaces this function's name onto [`EndDraw`](crate::graphics_engine::draw::EndDraw) and labels this address
/// `NGraphicsEngine::CGPUProfiler::BeginScope`; the body is unambiguous.
pub unsafe fn CopySurfaceToTexture(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    dst: *mut ::std::ffi::c_void,
    src: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            dst: *mut ::std::ffi::c_void,
            src: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(CopySurfaceToTexture_ADDRESS);
        f(ctx, dst, src)
    }
}
pub const EndDraw_ADDRESS: usize = 0x14195ABA0;
/// Submits the frame's recorded command work: under the context's critical section it indexes the
/// context's command-slot ring and submits the two prebuilt command objects for the current slot
/// through the backend. It takes no arguments beyond the context.
///
/// IDA labels this address `Graphics::CopySurfaceToTexture`; the two
/// names are swapped (see [`CopySurfaceToTexture`](crate::graphics_engine::draw::CopySurfaceToTexture)).
pub unsafe fn EndDraw(ctx: *mut crate::graphics_engine::graphics_engine::HContext_t) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        ) = ::std::mem::transmute(EndDraw_ADDRESS);
        f(ctx)
    }
}
pub const ResolveSurface_ADDRESS: usize = 0x1419672B0;
/// Resolves an MSAA surface.
pub unsafe fn ResolveSurface(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    params: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            params: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(ResolveSurface_ADDRESS);
        f(ctx, params)
    }
}
pub const SetScissorEnable_ADDRESS: usize = 0x141966B80;
/// Sets the scissor-enable bit of the context's current rasterizer-state key, which becomes `D3D11_RASTERIZER_DESC::ScissorEnable` when the state object for that key is
/// created. The key is looked up (and the object created on first use) by the render-state flush that
/// precedes every draw, so the change takes effect from the next draw on. Only the enable flag lives
/// in the key: the rectangles themselves are passed straight through to the context's
/// `RSSetScissorRects` by a separate wrapper and are ignored while the flag is clear.
pub unsafe fn SetScissorEnable(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    enable: bool,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            enable: bool,
        ) = ::std::mem::transmute(SetScissorEnable_ADDRESS);
        f(ctx, enable)
    }
}
pub const GetRTVFromSurface_ADDRESS: usize = 0x141956240;
/// Returns the surface's render-target view.
pub unsafe fn GetRTVFromSurface(
    surface: *mut ::std::ffi::c_void,
) -> *mut ::std::ffi::c_void {
    unsafe {
        let f: unsafe extern "system" fn(
            surface: *mut ::std::ffi::c_void,
        ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(GetRTVFromSurface_ADDRESS);
        f(surface)
    }
}
pub const GetDSVFromSurface_ADDRESS: usize = 0x141956250;
/// Returns the surface's depth-stencil view.
pub unsafe fn GetDSVFromSurface(
    surface: *mut ::std::ffi::c_void,
) -> *mut ::std::ffi::c_void {
    unsafe {
        let f: unsafe extern "system" fn(
            surface: *mut ::std::ffi::c_void,
        ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(GetDSVFromSurface_ADDRESS);
        f(surface)
    }
}
pub const CreateFragmentProgram_ADDRESS: usize = 0x141953470;
/// The leaf fragment-program creator: it wraps `ID3D11Device::CreatePixelShader` over
/// `params.m_Code`/`params.m_Size`. `CreatePixelShader` copies the bytecode, so `params.m_Code` need
/// not remain valid past the call, and the returned holder retains no bytecode of its own. Static (no
/// `this`); the first argument is the graphics device.
pub unsafe fn CreateFragmentProgram(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    params: *mut crate::graphics_engine::draw::CreateFragmentProgramParams,
) -> *mut crate::graphics_engine::draw::HFragmentProgram_t {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            params: *mut crate::graphics_engine::draw::CreateFragmentProgramParams,
        ) -> *mut crate::graphics_engine::draw::HFragmentProgram_t = ::std::mem::transmute(
            CreateFragmentProgram_ADDRESS,
        );
        f(device, params)
    }
}
pub const CreateVertexProgram_ADDRESS: usize = 0x141953320;
/// The leaf vertex-program creator: it wraps `ID3D11Device::CreateVertexShader` over
/// `params.m_Code`/`params.m_Size`. Unlike [`CreateFragmentProgram`](crate::graphics_engine::draw::CreateFragmentProgram), the returned holder also
/// retains a heap copy of the bytecode (used later for input-layout creation), so both the D3D
/// object and the retained copy reflect whatever bytecode was passed in. Static (no `this`); the
/// first argument is the graphics device.
pub unsafe fn CreateVertexProgram(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    params: *const crate::graphics_engine::draw::CreateVertexProgramParams,
) -> *mut ::std::ffi::c_void {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            params: *const crate::graphics_engine::draw::CreateVertexProgramParams,
        ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(
            CreateVertexProgram_ADDRESS,
        );
        f(device, params)
    }
}
pub const CreateHullProgram_ADDRESS: usize = 0x141953690;
/// The leaf hull-program creator: it wraps `ID3D11Device::CreateHullShader` (vtable slot 16) over
/// `params.m_Code`/`params.m_Size`. `CreateHullShader` copies the bytecode, so `params.m_Code` need not
/// remain valid past the call; unlike [`CreateVertexProgram`](crate::graphics_engine::draw::CreateVertexProgram), the returned holder contains only the shader
/// pointer with no retained bytecode copy. Static (no `this`); the first argument is the graphics
/// device.
pub unsafe fn CreateHullProgram(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    params: *const crate::graphics_engine::draw::CreateHullProgramParams,
) -> *mut crate::graphics_engine::draw::HHullProgram_t {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            params: *const crate::graphics_engine::draw::CreateHullProgramParams,
        ) -> *mut crate::graphics_engine::draw::HHullProgram_t = ::std::mem::transmute(
            CreateHullProgram_ADDRESS,
        );
        f(device, params)
    }
}
pub const CreateDomainProgram_ADDRESS: usize = 0x1419537A0;
/// The leaf domain-program creator: it wraps `ID3D11Device::CreateDomainShader` (vtable slot 17) over
/// `params.m_Code`/`params.m_Size`. `CreateDomainShader` copies the bytecode, so `params.m_Code` need
/// not remain valid past the call; like [`CreateHullProgram`](crate::graphics_engine::draw::CreateHullProgram), the returned holder contains only the shader
/// pointer with no retained bytecode copy. Static (no `this`); the first argument is the graphics
/// device.
pub unsafe fn CreateDomainProgram(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    params: *const crate::graphics_engine::draw::CreateDomainProgramParams,
) -> *mut crate::graphics_engine::draw::HDomainProgram_t {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            params: *const crate::graphics_engine::draw::CreateDomainProgramParams,
        ) -> *mut crate::graphics_engine::draw::HDomainProgram_t = ::std::mem::transmute(
            CreateDomainProgram_ADDRESS,
        );
        f(device, params)
    }
}
pub const SetFragmentProgramConstants_ADDRESS: usize = 0x141964840;
/// Stages `count` float4 constants into the given fragment constant buffer slot, starting at
/// `start_offset`, writing to the context's per-slot staging buffer and setting the dirty flag. The
/// actual GPU upload happens at the next state flush.
///
/// The staging buffer is a single 512-row `float4` array per shader stage, held in the graphics
/// context (the fragment one follows the vertex one); the
/// four slots of a stage carve their windows out of it, each starting at the row
/// [`SetFragmentProgramConstantBufferSize`](crate::graphics_engine::draw::SetFragmentProgramConstantBufferSize) recorded for it. The write is therefore bounded by that
/// window and by the pooled buffer the slot is bound to, not by `count` alone. A row-level redundancy
/// check skips rows byte-identical to what is already staged, and a call that skips every row leaves
/// the dirty flag alone.
pub unsafe fn SetFragmentProgramConstants(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    cb_index: i32,
    start_offset: u32,
    data: *const f32,
    count: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            cb_index: i32,
            start_offset: u32,
            data: *const f32,
            count: u32,
        ) = ::std::mem::transmute(SetFragmentProgramConstants_ADDRESS);
        f(ctx, cb_index, start_offset, data, count)
    }
}
pub const SetFragmentProgramConstantBufferSize_ADDRESS: usize = 0x1419643B0;
/// Declares how large a fragment constant-buffer slot is for the draws that follow, and where in the
/// stage's shared staging array its window begins.
///
/// `count` is a number of `float4` rows. It is not stored as-is: the engine rounds it **up** to the
/// next entry of [`constant_buffer_size_classes`](get_constant_buffer_size_classes) and binds the pre-created `ID3D11Buffer` of that
/// size to the slot (a
/// `PSSetConstantBuffers` call, skipped when the size class is unchanged). The rounded size class,
/// not `count`, is what [`SetupRenderStates`](crate::graphics_engine::draw::SetupRenderStates) later uploads, so the constant buffer the shader sees is
/// the rounded one and the rows between `count` and the size class are uploaded as whatever the
/// staging array happens to hold. `start_offset` is recorded as the slot's base row within the
/// stage's 512-row staging array; every call passes `0`, so at most one staged slot per stage
/// is live at a time. Passing `count = 0` unbinds the slot and clears its dirty flag.
///
/// The function stores only the bound buffer, the size class, and the base row. It does not keep
/// `count`, and [`SetFragmentProgramConstants`](crate::graphics_engine::draw::SetFragmentProgramConstants) performs no bounds check against it.
pub unsafe fn SetFragmentProgramConstantBufferSize(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    cb_index: i32,
    start_offset: u32,
    count: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            cb_index: i32,
            start_offset: u32,
            count: u32,
        ) = ::std::mem::transmute(SetFragmentProgramConstantBufferSize_ADDRESS);
        f(ctx, cb_index, start_offset, count)
    }
}
pub const SetVertexProgramConstantBufferSize_ADDRESS: usize = 0x141964280;
/// The vertex analogue of [`SetFragmentProgramConstantBufferSize`](crate::graphics_engine::draw::SetFragmentProgramConstantBufferSize), over the vertex staging array and
/// the vertex slots' pooled buffers.
pub unsafe fn SetVertexProgramConstantBufferSize(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    cb_index: i32,
    start_offset: u32,
    count: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            cb_index: i32,
            start_offset: u32,
            count: u32,
        ) = ::std::mem::transmute(SetVertexProgramConstantBufferSize_ADDRESS);
        f(ctx, cb_index, start_offset, count)
    }
}
pub const SetupRenderStates_ADDRESS: usize = 0x14195FEA0;
/// Flushes the graphics context's deferred state to D3D11 ahead of a draw: for each of the four slots
/// of each shader stage whose dirty flag is set, maps its bound constant buffer with
/// `D3D11_MAP_WRITE_DISCARD` and copies `16 * size_class` bytes into it from the stage's staging array
/// starting at the slot's base row — the **rounded** size class from
/// [`SetFragmentProgramConstantBufferSize`](crate::graphics_engine::draw::SetFragmentProgramConstantBufferSize), not the requested row count — then resolves the
/// depth-stencil, blend, and rasterizer state objects from their cached descriptors.
pub unsafe fn SetupRenderStates(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        ) = ::std::mem::transmute(SetupRenderStates_ADDRESS);
        f(ctx)
    }
}
pub const SetVertexProgramConstants_ADDRESS: usize = 0x141964740;
/// Stages `count` float4 constants into the given **vertex** constant buffer slot, starting at
/// `start_offset` (in float4 rows), writing to the context's per-slot staging buffer and setting the
/// dirty flag; the GPU upload happens at the next state flush. The vertex analogue of
/// [`SetFragmentProgramConstants`](crate::graphics_engine::draw::SetFragmentProgramConstants). `cb_index` is the vertex-shader constant-buffer slot (e.g. `1`
/// for `cb1`). A row-level redundancy check skips rows byte-identical to what is already staged.
pub unsafe fn SetVertexProgramConstants(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    cb_index: i32,
    start_offset: u32,
    data: *const f32,
    count: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            cb_index: i32,
            start_offset: u32,
            data: *const f32,
            count: u32,
        ) = ::std::mem::transmute(SetVertexProgramConstants_ADDRESS);
        f(ctx, cb_index, start_offset, data, count)
    }
}
pub const SetGeometryProgramConstants_ADDRESS: usize = 0x141964A40;
/// Stages `count` float4 constants into the given **geometry** constant buffer slot, starting at
/// `start_offset` (in float4 rows), writing to the context's per-slot staging buffer and setting the
/// dirty flag; the GPU upload happens at the next state flush. The geometry analogue of
/// [`SetFragmentProgramConstants`](crate::graphics_engine::draw::SetFragmentProgramConstants). The clustered deferred-lighting pass is the volume user: it
/// stages the light-assignment geometry shader's `ProjMatrix` here as four rows on `cb0`.
pub unsafe fn SetGeometryProgramConstants(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    cb_index: i32,
    start_offset: u32,
    data: *const f32,
    count: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            cb_index: i32,
            start_offset: u32,
            data: *const f32,
            count: u32,
        ) = ::std::mem::transmute(SetGeometryProgramConstants_ADDRESS);
        f(ctx, cb_index, start_offset, data, count)
    }
}
