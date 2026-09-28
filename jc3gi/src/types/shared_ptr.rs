#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A `boost::shared_ptr<T>`: the owned object pointer (`px`) and its reference-count control block
/// (`pn`).
pub struct SharedPtr<T> {
    /// The owned object (`px`), or null.
    pub px: *mut T,
    /// The shared reference-count control block (`pn`).
    pub pn: *mut ::std::ffi::c_void,
}
impl<T> SharedPtr<T> {}
#[repr(C, align(8))]
/// A `boost::weak_ptr<CGameObject>`: the object pointer (`px`) and the shared control block (`pn`),
/// whose weak count this pointer holds one reference on. Releasing the last weak reference destroys
/// the control block.
pub struct WeakPtrGameObject {
    /// The object (`px`), or null.
    pub px: *mut ::std::ffi::c_void,
    /// The shared reference-count control block (`pn`), or null.
    pub pn: *mut ::std::ffi::c_void,
}
fn _WeakPtrGameObject_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], WeakPtrGameObject>([0u8; 0x10]);
    }
    unreachable!()
}
impl WeakPtrGameObject {
    pub const Assign_ADDRESS: usize = 0x1404D0BC0;
    /// `boost::weak_ptr<CGameObject>::operator=`: copies `other` into `self`, taking a weak
    /// reference on the new control block and releasing the one held on the old. Assigning a null
    /// pointer releases `self`.
    pub unsafe fn Assign(
        &mut self,
        other: *const crate::types::shared_ptr::WeakPtrGameObject,
    ) -> *mut crate::types::shared_ptr::WeakPtrGameObject {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                other: *const crate::types::shared_ptr::WeakPtrGameObject,
            ) -> *mut crate::types::shared_ptr::WeakPtrGameObject = ::std::mem::transmute(
                Self::Assign_ADDRESS,
            );
            f(self as *mut Self as _, other)
        }
    }
}
impl std::convert::AsRef<WeakPtrGameObject> for WeakPtrGameObject {
    fn as_ref(&self) -> &WeakPtrGameObject {
        self
    }
}
impl std::convert::AsMut<WeakPtrGameObject> for WeakPtrGameObject {
    fn as_mut(&mut self) -> &mut WeakPtrGameObject {
        self
    }
}
#[allow(dead_code)]
impl<T> SharedPtr<T> {
    /// `true` if the owned pointer is non-null.
    pub fn exists(&self) -> bool {
        !self.px.is_null()
    }
    /// Borrow the pointee, or `None` if the pointer is null.
    ///
    /// # Safety
    /// `px` must point to a live `T` for the duration of the borrow.
    pub unsafe fn as_ref(&self) -> Option<&T> {
        if self.px.is_null() { None } else { unsafe { Some(&*self.px) } }
    }
    /// Mutably borrow the pointee, or `None` if the pointer is null.
    ///
    /// # Safety
    /// `px` must point to a live `T` with no other live borrows for the duration.
    pub unsafe fn as_mut(&mut self) -> Option<&mut T> {
        if self.px.is_null() { None } else { unsafe { Some(&mut *self.px) } }
    }
}
