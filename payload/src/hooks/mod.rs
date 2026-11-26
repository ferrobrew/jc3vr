//! The payload's detours, grouped by the engine system they hook. The hook plumbing they install
//! through (the installation, the removal, the shared `Patcher`, and the registered steps), the
//! gameplay predicate, and the online timing detours live in the core crate and are re-exported
//! here.

pub(crate) use jc3vr_core::hooks::{
    in_gameplay, install, online, patcher, register_post_enable, register_post_unpatch,
    register_pre_unpatch, suspend, uninstall,
};

pub mod animation;
pub mod camera;
pub mod character;
pub mod clock;
pub mod draw_count;
pub(crate) mod equipment;
pub mod game;
pub mod graphics_engine;
pub mod input;
#[cfg(feature = "profiler")]
pub mod profiler;
pub mod sim;
pub mod ui;
pub mod window_focus;
pub mod wndproc;
