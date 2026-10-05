//! Directing tools for staging footage from inside the headset: time control, actor safety, heat
//! control, teleport bookmarks, a spawner, a cinematic mode that hides every overlay, take
//! markers, and the key bindings that drive them without opening the debug panel.
//!
//! Everything here is applied from [`update`], which runs on the game's main thread at the top of
//! `CGame::Update` (`crate::update`), so the engine setters that assert the main thread (the heat
//! manager, the teleports) are always called from the right place. The debug tab and the hotkeys
//! only mutate [`DirectorConfig`] or queue a request; the per-frame pass reconciles the live game
//! state against the config, which is what makes the settings survive the game's own resets (a
//! respawn clearing god mode, a mission reset unlocking heat).

use serde::{Deserialize, Serialize};

use crate::config::CONFIG;

pub mod cinematic;
pub mod heat;
pub mod keymap;
pub mod markers;
pub mod safety;
pub mod spawn;
pub mod teleport;
pub mod time;

/// The directing settings, reconciled against the live game every frame by [`update`].
#[derive(Clone, Serialize, Deserialize)]
pub struct DirectorConfig {
    /// The game-clock rate: `1.0` is real time, below it is slow motion. Written into the
    /// slow-motion manager's normal game speed, so the game's own scripted slow-motion effects
    /// still blend against it. The live clock state, so it is not saved.
    #[serde(skip, default = "real_time")]
    pub time_scale: f32,
    /// The rate the slow-motion hotkey toggles [`time_scale`](Self::time_scale) to and from `1.0`.
    pub slow_motion_scale: f32,
    /// Hold the game clock. The clock's own pause: the world freezes but the render loop runs. The
    /// live clock state, so it is not saved.
    #[serde(skip)]
    pub paused: bool,
    /// Keep the player's character in god mode (no damage), re-asserted every frame so a respawn
    /// or a sequence cannot clear it.
    pub god_mode: bool,
    /// Keep the player's weapons on unlimited ammo, re-asserted every frame.
    pub unlimited_ammo: bool,
    /// Lock the heat system so the level neither rises nor decays, re-asserted every frame.
    pub heat_lock: bool,
    /// Cap the heat level at this value (`0..=5`), or `None` to leave the game's own cap alone.
    pub heat_cap: Option<i32>,
    /// Hide the floating HUD, the debug panel, and the desktop overlay together; see
    /// [`cinematic`]. A mode for the shot at hand, so it is not saved.
    #[serde(skip)]
    pub cinematic: bool,
    /// The spawner's request: what to spawn and where relative to the view.
    pub spawn: spawn::SpawnConfig,
    /// The key bindings that drive the directing tools; see [`keymap`].
    pub keymap: keymap::Keymap,
}
impl DirectorConfig {
    pub const fn new() -> Self {
        Self {
            time_scale: 1.0,
            slow_motion_scale: 0.25,
            paused: false,
            god_mode: false,
            unlimited_ammo: false,
            heat_lock: false,
            heat_cap: None,
            cinematic: false,
            spawn: spawn::SpawnConfig::new(),
            keymap: keymap::Keymap::new(),
        }
    }
}

/// The per-frame reconciliation pass. Main thread, once per real frame, before the debug UI runs
/// so a hotkey and a panel click in the same frame are applied in one place.
pub fn update() {
    let cfg = CONFIG.lock().director.clone();
    cinematic::set(cfg.cinematic);
    time::apply(&cfg);
    safety::apply(&cfg);
    heat::apply(&cfg);
    teleport::service_pending();
    spawn::service_pending(&cfg.spawn);
}

/// The default of [`DirectorConfig::time_scale`] when it is not loaded: real time.
fn real_time() -> f32 {
    1.0
}
