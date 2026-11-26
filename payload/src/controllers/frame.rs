//! The published controller frame now lives in [`crate::input_core::frame`]; this re-export keeps
//! the old path until the crate cut.

pub(crate) use crate::input_core::frame::*;
