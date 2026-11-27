//! Registers the vehicle handling with the body: the wingsuit's take-off from the ground, ticked by
//! the roomscale's frame half ([`BodyContributions::take_off`]). The vehicle rail's refusal of the
//! automatic entry into a vehicle's stunt area below the player
//! ([`blocks_auto_attach`](crate::controllers::vehicles::rail::blocks_auto_attach)) rides in the
//! gameplay crate's refusal table, which declares the refusals' order. The rail's carry and the
//! vehicle controls' targets are the physical crate's contributions, which the crate's own
//! registration fills ([`crate::register()`]). The handles' kick by each shot of the mounted gun the
//! hands aim ([`on_shot`](crate::controllers::vehicles::mounted_recoil::on_shot)) is the weapon
//! handling's contribution, which the gameplay crate's install registers.

use jc3vr_body::contributions::BodyContributions;

use crate::controllers::vehicles::take_off;

/// Fill the vehicle handling's field of `body`, beside the mounted-gun source
/// ([`crate::controllers::vehicles::register_mounted`]).
pub fn register(body: &mut BodyContributions) {
    body.take_off = Some(take_off::tick);
}
