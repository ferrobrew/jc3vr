//! Input-area detours, mirroring `jc3gi::input`. The look detour, which lands the bindings' writes
//! on the input tick, lives in the gameplay crate's `hooks::input`, which composes these detours'
//! libraries around it in their established order.

pub mod config;
pub use config::MovementConfig;

pub mod air;
pub mod locomotion;
pub mod parachute;
pub mod swim;
pub mod wingsuit;
