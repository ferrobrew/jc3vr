//! The haptic output's shared mixer: the cue sources queue into it from any thread, and the VR
//! frame loop drains it once per frame into the controllers, noting what each hand was sent.

use std::time::Instant;

use parking_lot::Mutex;

use crate::haptics::mixer::{HapticPulse, MIX, Mixer};

/// Run `f` on the shared mixer, for a source queueing a cue or a readout reading its counters.
pub fn with_mixer<R>(f: impl FnOnce(&mut Mixer) -> R) -> R {
    f(&mut MIXER.lock())
}

/// What each hand plays from `now`, indexed by hand, for the VR frame loop to send. Notes each
/// hand's amplitude for [`last_output`].
pub fn mix(now: Instant) -> [Option<HapticPulse>; 2] {
    let out = MIXER.lock().mix(now, &MIX);
    *LAST_OUTPUT.lock() = out.map(|pulse| pulse.map(|p| p.amplitude));
    out
}

/// Forget everything the mixer holds: the controllers or the haptics are off, or the session
/// ended.
pub fn clear() {
    MIXER.lock().clear();
}

/// The amplitude each hand was last sent, indexed by hand, if the last flush sent it anything.
pub fn last_output() -> [Option<f32>; 2] {
    *LAST_OUTPUT.lock()
}

static MIXER: Mutex<Mixer> = Mutex::new(Mixer::new());

/// What [`mix`] last sent each hand.
static LAST_OUTPUT: Mutex<[Option<f32>; 2]> = Mutex::new([None; 2]);
