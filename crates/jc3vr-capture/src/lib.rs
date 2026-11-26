//! The payload's capture layer: the tooling that reads the rendered frame back for the offline
//! comparisons, naming nothing above it.
//!
//! The crate sits above the core, input, view, and render crates (`docs/architecture.md`, "Crate
//! layering"). It defines no detours: it reaches the render hooks only through the probe tables the
//! render crate keeps for it, which it fills at install, and which the render hooks call only while
//! a capture or benchmark request has armed them. This crate owns:
//!
//! - [`screenshot::harness`]: the capture harness (the per-eye and stage captures, the draw census,
//!   the snapshot comparisons, the water GPU probe, and the sun probe), and the registrations of its
//!   probes into the single-pass draw path and the graphics-engine pipeline.
//! - [`testing`]: the visual stage's camera, clock, and render overrides, and the benchmark
//!   samplers. The request parsing and the stage driver live in the tooling layer above.

pub mod screenshot;
pub mod testing;

#[cfg(test)]
mod test_config;
