#![cfg_attr(any(), rustfmt::skip)]
pub const OperatorNew_ADDRESS: usize = 0x140002350;
/// The engine's global `operator new`: allocates `size` bytes, 16-byte aligned, from the engine
/// allocator (`MemoryAlloc`). Engine objects allocated with it are freed by their deleting
/// destructors.
pub unsafe fn OperatorNew(size: u64) -> *mut ::std::ffi::c_void {
    unsafe {
        let f: unsafe extern "system" fn(size: u64) -> *mut ::std::ffi::c_void = ::std::mem::transmute(
            OperatorNew_ADDRESS,
        );
        f(size)
    }
}
