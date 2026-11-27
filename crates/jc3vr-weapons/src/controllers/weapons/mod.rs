//! Physical weapon handling: the plan of record's "Holsters", "Two-handed weapons", "Reloads",
//! "Grenades", and "Grip semantics" (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! - [`volumes`]: the sockets' landmarks and the pouches (the grenade pouch, the reload pouch, and
//!   the C4 pouch), placed on the character's skeleton;
//! - [`pouch_models`]: the game's models of what the pouches hold, drawn on them;
//! - [`docks`]: the sockets' volumes, seated on the weapons the loadout table has docked and where a
//!   held weapon would dock;
//! - [`foregrip`]: the two-handed weapon's foregrip and the two-handed pose;
//! - [`recoil`]: the recoil offset between the controller and the gun, with its return spring;
//! - [`heft`]: the one-handed weight of a long gun: sag, lag, sway, a stronger kick, and a wider
//!   spread, gone while it is braced on the foregrip;
//! - [`fire`]: the per-shot seam that kicks the recoil and marks the shot's hand;
//! - [`grip_watch`]: a diagnostic watch on how each held gun sits in its hand;
//! - [`gun_bodies`]: the held guns the hand bodies make bodies of, as the weapon handling publishes
//!   them, the shots that kick them, and what each step reports back;
//! - [`dual_wield`]: each dual-wielded pistol drawn by its own hand and fired by its own hand's
//!   trigger;
//! - [`hold`]: the weapons in the hands as the loadout table has them, the attach bones that put
//!   them there, the draws, docks, and drops at the sockets, and the direct pickup;
//! - [`regions`]: a long gun's parts, derived from its model's box and its known points, and a
//!   gun taken, braced, or carried by whichever part the hand closes on;
//! - [`inventory`]: every weapon the player owns, holds, or let go of nearby, and the ones it
//!   keeps from the game's despawn;
//! - [`camera_kick`]: the game's camera recoil, suppressed while the guns aim by hand;
//! - [`grenade`]: the physical grenade throw;
//! - [`opening`]: when a hand holding a grenade or a charge lets it go: when its fingers open;
//! - [`c4`]: the C4 throw from the chest pouch and the wrist detonator;
//! - [`in_hand`]: where a held model sits in the hand, and the render-time placement of the
//!   explosives a hand holds (the primed and the caught grenade, and the primed charge);
//! - [`reload`]: the true reload: eject, load from the belt pouch, and chamber by hand;
//! - [`sounds`]: the sounds at the gun for the hands' own work: seats, action cycles, draws, and
//!   holsters;
//! - [`empty`]: the game's automatic switch away from an empty weapon suppressed, and the dry fire
//!   pulsed;
//! - [`unarmed_fire`]: the fire input with nothing in the hands kept from drawing a weapon;
//! - [`scope`]: scoped weapons: the game's precision aim suppressed, and the lens on the scope;
//! - [`wheel`]: the game's weapon selection cross, opened with a held input and steered by the
//!   hand;
//! - [`config`]: the weapon-handling settings.
//!
//! The weapon handling runs in the input tick's game stage
//! (`controllers::bindings::stage`), in two halves around the bindings' tables
//! (`controllers::bindings::game_tick`):
//!
//! 1. [`begin_tick`] reads the wielded weapon and provides the weapon handling's grip targets
//!    ([`jc3vr_body::controllers::grab`]): the sockets and pouches on the body, reached by the player's
//!    own grip before the body-scale remap while they ride the player's body, each pouch with a
//!    reach margin a hand that misses it by a little still takes it within; the held gun's parts,
//!    reached by the hand on the gun; the weapons in the world without a body (in characters'
//!    hands, on racks, in lockers); and the grenades and magazines in the world. A braced
//!    foregrip is offered to its hand as held.
//! 2. [`finish_tick`] acts on the grabs the grip model hands it: a socket grab draws or docks as
//!    the loadout table decides ([`crate::controllers::loadout::grab`]), a grab of the held gun's
//!    grip by the free hand hands the gun over, a foregrip grab braces the gun, a grenade-pouch
//!    grab draws a safe grenade that a deliberate tracked throw arms, a C4-pouch grab primes a
//!    charge, and a reload-pouch grab takes a round for the reload. Opening the grip on a gun docks
//!    it on the nearest socket under the hand that takes it, or lets it fall
//!    ([`jc3vr_body::controllers::grab::resolve::dock_target`]). It runs the throws, the wrist
//!    detonator, and the reload, and holds the fire inputs back while a reload plays (the
//!    action-runs hand tier) and while the reload holds the gun from firing.
//!
//! A gun is held while its grip is: nothing here draws or docks except a grab and its release.
//! The markers on the targets are the grip model's ([`jc3vr_body::controllers::grab::markers`]).
//! Everything acts only while the bindings inject, which is only under the controller scheme with
//! a controller active.

pub mod ads;
mod begin;
pub mod c4;
mod camera_kick;
pub mod capabilities;
pub mod collide;
pub mod config;
pub mod contents;
mod context;
pub mod docks;
pub mod dual_wield;
pub mod empty;
pub mod engine;
mod finish;
pub mod fire;
pub mod foregrip;
pub mod grenade;
pub mod grip_sources;
pub(crate) mod grenade_moves;
pub mod grip_watch;
pub mod gun_bodies;
pub mod handling_tuning;
pub mod heft;
mod held_source;
pub mod hold;
pub mod hud_feed;
pub mod in_hand;
pub mod inventory;
pub(crate) mod mirror;
pub mod opening;
pub(crate) mod permission;
pub mod pouch_models;
pub(crate) mod pouch_return;
pub mod recoil;
pub mod regions;
pub mod register_body;
pub mod reload;
pub mod scope;
mod self_hits;
pub(crate) mod sounds;
mod stand_down;
mod state;
mod types;
pub mod unarmed_fire;
pub mod volumes;
pub mod wheel;

pub use begin::begin_tick;
// Re-exported so the weapon-handling submodules that need it (the reload) can reach it at the
// weapon handling's own path.
use begin::weighted_hands;
pub use finish::finish_tick;
use re_utilities::hook_library::HookLibrary;
pub use stand_down::{StandDown, StandDownScope, stand_down};
pub use state::status;
pub use types::{TickStart, WeaponsStatus};

/// The weapon handling's hooks, aggregated by `controllers::hook_library`.
pub fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(fire::hook_library())
        .with_hook_library(self_hits::hook_library())
        .with_hook_library(camera_kick::hook_library())
        .with_hook_library(grenade::hook_library())
        .with_hook_library(c4::hook_library())
        .with_hook_library(in_hand::hook_library())
        .with_hook_library(scope::hook_library())
        .with_hook_library(hold::hook_library())
        .with_hook_library(hold::rack::offer::hook_library())
        .with_hook_library(dual_wield::single::hook_library())
        .with_hook_library(inventory::hook_library())
        .with_hook_library(empty::hook_library())
        .with_hook_library(heft::hook_library())
        .with_hook_library(reload::hook_library())
        .with_hook_library(wheel::hook_library())
}
