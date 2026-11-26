//! The per-tick facts a subsystem publishes for the subsystems below it to read in place of its
//! live state, one module per fact. Each fact is written before its readers run in the same tick,
//! or its readers take the previous tick's.

pub mod airframe;
pub mod climb;
pub mod driving;
pub mod line;
pub mod lock;
pub mod mounted;
pub mod seat;
