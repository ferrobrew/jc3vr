//! Physical constants and shared test fixtures that more than one crate's code or tests use.

/// Standard gravity, metres per second squared.
pub const GRAVITY_MPS2: f32 = 9.81;

/// The sim tick, seconds, of the seated recording whose hand paths the grapple's yank, snatch, and
/// pull tests replay: they sample one path per tick, or every other tick at twice this.
pub const SEATED_TICK_S: f32 = 0.0266;
