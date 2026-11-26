//! The view layer's engine detours, grouped by the engine system they hook: the camera, which
//! places the head pose and the eyes, and the window focus. The hook plumbing they install through
//! lives in the core crate. [`graphics_engine`] holds the dependency-free registries and cells the
//! render crate's graphics-engine hooks read and the layers above fill.

pub mod camera;
pub mod graphics_engine;
pub mod window_focus;
