//! The haptic data and the mixer: what every vibration is made of, and how the cues reaching one
//! hand fold into the one vibration it plays. The sources and the settings live in
//! [`crate::controllers::haptics`], and the VR runtime sends what the mixer gives each hand.
//!
//! - [`vocabulary`]: the named primitives and the shape each one plays with;
//! - [`cues`]: every source, as a one-off event or a sustained level, with the primitive,
//!   strength, and category it plays;
//! - [`mixer`]: what each hand plays when several sources reach it at once;
//! - [`output`]: the one mixer the sources queue into and the VR frame loop drains.

pub(crate) mod cues;
pub(crate) mod mixer;
pub(crate) mod output;
pub(crate) mod vocabulary;
