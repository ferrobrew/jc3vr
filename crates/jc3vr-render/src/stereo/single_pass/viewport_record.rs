//! What the collapse knows about the immediate context's viewports, readable on every draw without a
//! lock.
//!
//! Three things: the last scene-sized viewport the engine bound (the full width the eye halves are
//! cut from), the last viewport the engine bound of any size (so a reduced-resolution target is split
//! by its own size), and the pair the two viewport slots hold now. The first two are read by every
//! draw the collapse routes, the last lets [`crate::stereo::single_pass::ensure_collapse_viewport`]
//! skip a bind that would change nothing, which is most of them: a pass's patched draws all want the
//! same split.
//!
//! The draw path runs one draw after another on the engine's draw thread with its context mutex
//! held, so the record is written rarely (an engine viewport bind, or a bind that changes the slots)
//! and read on every draw. A sequence lock makes the read a handful of plain loads; a read-modify-
//! write (a lock or a counter) on every draw costs more than it looks, since it waits for the stores
//! the runtime has just made to drain. Writers are serialised by a mutex of their own.
//!
//! The bound pair is only as good as the guarantee that every bind is seen. Every `RSSetViewports`
//! on the immediate context passes through `rs_set_viewports_detour`, which records what it binds,
//! and the mod's own binds through the trampoline record theirs. What could still change the slots
//! unseen is a state reset (`ClearState`), which nothing calls inside a dispatch; the pair is
//! forgotten at every pass boundary all the same, so a missed change cannot outlive the pass it
//! happened in.

use std::sync::atomic::{AtomicU32, Ordering, fence};

use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::D3D11_VIEWPORT;

/// The last scene-sized viewport the engine bound, if any.
pub(crate) fn scene_viewport() -> Option<D3D11_VIEWPORT> {
    read().scene
}

/// The viewport the collapse splits: the engine's latest bind when the split follows the bound
/// target, falling back to the scene's, otherwise the scene's.
pub(crate) fn base_viewport(follows_target: bool) -> Option<D3D11_VIEWPORT> {
    let record = read();
    if follows_target {
        record.engine.or(record.scene)
    } else {
        record.scene
    }
}

/// The base viewport (as [`base_viewport`]) and the pair the slots are known to hold, in one read.
pub(crate) fn base_and_bound(
    follows_target: bool,
) -> (Option<D3D11_VIEWPORT>, Option<[D3D11_VIEWPORT; 2]>) {
    let record = read();
    let base = if follows_target {
        record.engine.or(record.scene)
    } else {
        record.scene
    };
    (base, record.bound)
}

/// Record an engine viewport bind: always the engine's latest, and the scene's when it is
/// scene-sized. `bound` is the pair the bind leaves in the slots.
pub(crate) fn record_engine_bind(
    viewport: D3D11_VIEWPORT,
    scene_sized: bool,
    bound: [D3D11_VIEWPORT; 2],
) {
    write(|record| {
        record.engine = Some(viewport);
        if scene_sized {
            record.scene = Some(viewport);
        }
        record.bound = Some(bound);
    });
}

/// Record what the two slots hold after a bind: `Some` for a known pair, `None` when the bind left
/// them in a state the record does not model (one slot, or more than two).
pub(crate) fn record_bound(bound: Option<[D3D11_VIEWPORT; 2]>) {
    if read().bound == bound {
        return;
    }
    write(|record| record.bound = bound);
}

/// Forget the bound pair, so the next routed draw binds its viewports whatever they were.
pub(crate) fn forget_bound() {
    record_bound(None);
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Record {
    scene: Option<D3D11_VIEWPORT>,
    engine: Option<D3D11_VIEWPORT>,
    bound: Option<[D3D11_VIEWPORT; 2]>,
}

/// The validity word, then six words per viewport: the scene's, the engine's, and the bound pair.
const WORDS: usize = 1 + 6 + 6 + 12;
const SCENE_VALID: u32 = 1;
const ENGINE_VALID: u32 = 2;
const BOUND_VALID: u32 = 4;

/// Even while the record is stable, odd while a writer is storing it.
static SEQUENCE: AtomicU32 = AtomicU32::new(0);
static DATA: [AtomicU32; WORDS] = [const { AtomicU32::new(0) }; WORDS];
static WRITER: Mutex<()> = Mutex::new(());

fn read() -> Record {
    loop {
        let before = SEQUENCE.load(Ordering::Acquire);
        if before & 1 == 1 {
            std::hint::spin_loop();
            continue;
        }
        let mut words = [0u32; WORDS];
        for (word, data) in words.iter_mut().zip(&DATA) {
            *word = data.load(Ordering::Relaxed);
        }
        fence(Ordering::Acquire);
        if SEQUENCE.load(Ordering::Relaxed) == before {
            return decode(&words);
        }
    }
}

fn write(change: impl FnOnce(&mut Record)) {
    let _writer = WRITER.lock();
    let mut words = [0u32; WORDS];
    for (word, data) in words.iter_mut().zip(&DATA) {
        *word = data.load(Ordering::Relaxed);
    }
    let mut record = decode(&words);
    change(&mut record);
    let words = encode(&record);
    let sequence = SEQUENCE.load(Ordering::Relaxed);
    SEQUENCE.store(sequence.wrapping_add(1), Ordering::Relaxed);
    fence(Ordering::Release);
    for (word, data) in words.iter().zip(&DATA) {
        data.store(*word, Ordering::Relaxed);
    }
    SEQUENCE.store(sequence.wrapping_add(2), Ordering::Release);
}

fn encode(record: &Record) -> [u32; WORDS] {
    let mut words = [0u32; WORDS];
    let mut valid = 0;
    if let Some(scene) = record.scene {
        valid |= SCENE_VALID;
        put(&mut words[1..7], scene);
    }
    if let Some(engine) = record.engine {
        valid |= ENGINE_VALID;
        put(&mut words[7..13], engine);
    }
    if let Some([slot0, slot1]) = record.bound {
        valid |= BOUND_VALID;
        put(&mut words[13..19], slot0);
        put(&mut words[19..25], slot1);
    }
    words[0] = valid;
    words
}

fn decode(words: &[u32; WORDS]) -> Record {
    let valid = words[0];
    Record {
        scene: (valid & SCENE_VALID != 0).then(|| take(&words[1..7])),
        engine: (valid & ENGINE_VALID != 0).then(|| take(&words[7..13])),
        bound: (valid & BOUND_VALID != 0).then(|| [take(&words[13..19]), take(&words[19..25])]),
    }
}

fn put(words: &mut [u32], viewport: D3D11_VIEWPORT) {
    let fields = [
        viewport.TopLeftX,
        viewport.TopLeftY,
        viewport.Width,
        viewport.Height,
        viewport.MinDepth,
        viewport.MaxDepth,
    ];
    for (word, field) in words.iter_mut().zip(fields) {
        *word = field.to_bits();
    }
}

fn take(words: &[u32]) -> D3D11_VIEWPORT {
    let field = |i: usize| f32::from_bits(words[i]);
    D3D11_VIEWPORT {
        TopLeftX: field(0),
        TopLeftY: field(1),
        Width: field(2),
        Height: field(3),
        MinDepth: field(4),
        MaxDepth: field(5),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport(x: f32, width: f32) -> D3D11_VIEWPORT {
        D3D11_VIEWPORT {
            TopLeftX: x,
            TopLeftY: 0.0,
            Width: width,
            Height: 100.0,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        }
    }

    #[test]
    fn a_record_survives_its_encoding() {
        let record = Record {
            scene: Some(viewport(0.0, 200.0)),
            engine: None,
            bound: Some([viewport(0.0, 100.0), viewport(100.0, 100.0)]),
        };
        assert!(decode(&encode(&record)) == record);
        assert!(decode(&encode(&Record::default())) == Record::default());
    }
}
