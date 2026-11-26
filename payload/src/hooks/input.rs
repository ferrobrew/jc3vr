//! The input tick detours live in the gameplay crate and are re-exported here; the old path keeps
//! working until the final cleanup pass.

pub(crate) use jc3vr_gameplay::hooks::input::*;
