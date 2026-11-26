//! The render-time re-pose of the player's body as the character render blocks see it: how a
//! block's palette stores its matrices, the edit a draw applies to the palette copy, and the
//! provider that settles the edit for each draw of the body.
//!
//! The render blocks ([`crate::hooks::graphics_engine::render_block`]) only apply the edit. What it
//! re-poses, and from what tracking, is the provider's, which registers itself at startup; until it
//! does, the body draws with the engine's palette.

use std::sync::OnceLock;

use glam::Mat4;
use jc3gi::graphics_engine::render_block::Matrix3x4;

/// How a block's palette stores a 3x4 affine matrix (see the layout detection in
/// [`crate::hooks::graphics_engine::render_block`]).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PaletteLayout {
    /// Four 3-float columns: the images of the X, Y, and Z axes, then the translation.
    Columns,
    /// Three 4-float rows of `[R | t]`.
    Rows,
}

/// A draw's edit of the body's palette.
pub(crate) trait BodyPaletteEdit {
    /// Whether the edit changes `bone`'s entry.
    fn touches(&self, bone: usize) -> bool;
    /// How many entries of the engine's palette the edit reads, from the first.
    fn source_len(&self) -> usize;
    /// The neck's edit, which carries the point the hidden head collapses to.
    fn neck(&self) -> Mat4;
    /// Pre-multiply the edited entries of `entries`, which spans the batch's referenced bones,
    /// reading the engine's palette `source`.
    fn apply(&self, entries: &mut [Matrix3x4], source: &[Matrix3x4], layout: PaletteLayout);
    /// Sample the drawn arms from the edited `entries`, for the diagnostics.
    fn sample_drawn_arms(&self, entries: &[Matrix3x4], layout: PaletteLayout);
}

/// The provider of the body's edits. It holds each draw's edit for the drawing thread, between
/// [`Self::begin`] and [`Self::end`]: character draws run on several render worker threads at once.
pub(crate) trait PlayerDrawEdits: Sync {
    /// Settle the edit for this thread's draw of the body whose instance matrix is `instance`, at
    /// the camera's interpolation fraction `dtf`. Whether there is one.
    fn begin(&self, instance: Mat4, dtf: f32) -> bool;
    /// Drop this thread's edit.
    fn end(&self);
    /// Run `f` with this thread's edit, `None` when it holds none.
    fn with(&self, f: &mut dyn FnMut(Option<&dyn BodyPaletteEdit>));
}

/// Register the provider of the body's edits. The first registration stands.
pub(crate) fn register_player_draw_edits(edits: &'static dyn PlayerDrawEdits) {
    if PROVIDER.set(edits).is_err() {
        tracing::warn!(
            "render block: a player draw edit provider is already registered; the new one is ignored"
        );
    }
}

pub(crate) fn player_draw_edits() -> Option<&'static dyn PlayerDrawEdits> {
    PROVIDER.get().copied()
}

static PROVIDER: OnceLock<&'static dyn PlayerDrawEdits> = OnceLock::new();
