//! The controller motion history now lives in [`crate::input_core::motion`]; this re-export keeps
//! the old path until the crate cut.

pub(crate) use crate::input_core::motion::*;
