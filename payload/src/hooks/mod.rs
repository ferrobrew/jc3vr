//! The payload's detours, grouped by the engine system they hook. The hook plumbing they install
//! through (the installation, the removal, the shared `Patcher`, and the registered steps), the
//! gameplay predicate, and the online timing detours live in the core crate. The engine-pipeline
//! detours (the graphics engine, the camera, the clock, the draw counts, the window focus, and the
//! profiler's engine scopes) live in the render crate, and the gameplay detours (the character,
//! the animation graph, the input tick, the equipment, and the sim step) in the gameplay crate.
//! Both crates' modules are re-exported through shim files at their old paths. The UI detours live
//! in the HUD crate and are re-exported through a shim file at [`ui`].

pub(crate) use jc3vr_core::hooks::{install, online, uninstall};

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
