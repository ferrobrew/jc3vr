//! Synthetic tracking recordings: `.jc3trk` files written from scripted head and controller paths
//! rather than recorded in a headset, so the body rig can be tested by a replay without anyone
//! wearing one. See `docs/mod/testing/recorder.md`.
//!
//! - [`track`] is a keyframed scalar eased between its keys.
//! - [`script`] describes what a synthetic player does as tracks of the body and the controllers.
//! - [`scripts`] holds the built-in scripts, one per synthetic scenario.
//! - [`rig`] poses a 1.8 m player's body from the tracks and reads the head and grips off it.
//! - [`inputs`] turns the controller tracks into the raw inputs and digital state of an Index
//!   controller.
//! - [`schema`] reads the payload's semantic action schema, which the semantic states index.
//! - [`synth`] samples a script into a recording, frame by frame.

pub mod inputs;
pub mod rig;
pub mod schema;
pub mod script;
pub mod scripts;
pub mod synth;
pub mod track;

use crate::{
    schema::Schema,
    script::Script,
    synth::{Options, SynthError},
};

/// The bytes of `script`'s recording, sampled with `options` against the payload's schema.
pub fn generate(script: &Script, options: Options) -> Result<Vec<u8>, SynthError> {
    let schema = Schema::payload()?;
    Ok(synth::encode(&synth::recording(script, options, &schema)?))
}

#[cfg(test)]
mod tests;
