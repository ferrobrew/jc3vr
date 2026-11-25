//! The game's dual-wield weapon as the mod holds it. The engine side is in
//! `docs/engine/gameplay/weapon-equip-and-drop.md`, "The dual-wield weapon".
//!
//! The game builds every weapon of the dual-wield slot as one object with a gun per hand. Each is
//! made a pistol of its own ([`single`]): an item of the loadout table's like any other weapon,
//! drawn, docked, held, fired, and reloaded by whichever hand the table has it in, on whichever hip
//! the table has it docked on. A pair left as the game's, while the pistols are not made separate,
//! is one item held in one hand with its second gun in the other
//! ([`crate::controllers::weapons::hold::layout::placed_bone`]), and fires from the game's own fire
//! input, which alternates its guns.

pub mod single;
