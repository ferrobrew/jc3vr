//! Directing tools for staging footage from inside the headset: time control, actor safety, heat
//! control, teleport bookmarks, a spawner, a clean view that hides the overlays, take markers, and the key bindings that drive them without opening the debug panel.
//!
//! The spawner itself, with the spawn definition table, is [`jc3vr_weapons::controllers::spawn`],
//! because the inventory's gives use the table too; the director queues its requests and services
//! them.
//!
//! Everything here is applied from [`update`], which runs on the game's main thread at the top of
//! `CGame::Update` (the payload's frame driver), so the engine setters that assert the main thread
//! (the heat manager, the teleports) are always called from the right place. The debug tab and the
//! hotkeys only mutate [`DirectorConfig`] or queue a request; the per-frame pass reconciles the
//! live game state against the config, which is what makes the settings survive the game's own
//! resets (a respawn clearing god mode, a mission reset unlocking heat).

use jc3vr_capture::testing::visual::PAUSED;
use jc3vr_core::{
    clean_view,
    clean_view::CLEAN_VIEW_CONFIG,
    config::{ConfigSlot, SlotOptions},
    slot_children,
};
use jc3vr_view::recorder::TIME_SCALE;
use jc3vr_weapons::controllers::spawn;
use serde::{Deserialize, Serialize};

pub mod heat;
pub mod keymap;
pub mod markers;
pub mod safety;
pub mod teleport;
pub mod time;

/// The directing tools' settings, at `director` in the settings file; the payload binds the slot at
/// startup. The lower layers read three of its fields through slots of their own.
pub static DIRECTOR_CONFIG: ConfigSlot<DirectorConfig> = ConfigSlot::with(
    "director",
    SlotOptions::new().with_children(slot_children!(DIRECTOR_CONFIG {
        TIME_SCALE => time_scale,
        PAUSED => paused,
        CLEAN_VIEW_CONFIG => clean_view,
    })),
);

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
    /// The clean view's switch and scope; see [`clean_view`]. Neither is saved.
    #[serde(skip)]
    pub clean_view: clean_view::CleanViewConfig,
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
            clean_view: clean_view::CleanViewConfig::new(),
            spawn: spawn::SpawnConfig::new(),
            keymap: keymap::Keymap::new(),
        }
    }
}

impl Default for DirectorConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The per-frame reconciliation pass. Main thread, once per real frame, before the debug UI runs
/// so a hotkey and a panel click in the same frame are applied in one place.
pub fn update() {
    let cfg = DIRECTOR_CONFIG.lock().clone();
    clean_view::update(&cfg.clean_view);
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
