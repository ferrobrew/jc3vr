//! Input-area detours, mirroring `jc3gi::input`: the look detour, where the bindings' writes land
//! on the input tick, and the composition of the body crate's input detours around it.

use jc3vr_body::hooks::input::{air, locomotion, parachute, swim};
use re_utilities::hook_library::HookLibrary;

pub mod look;

pub fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(air::hook_library())
        .with_hook_library(locomotion::hook_library())
        .with_hook_library(look::hook_library())
        .with_hook_library(parachute::hook_library())
        .with_hook_library(swim::hook_library())
}
