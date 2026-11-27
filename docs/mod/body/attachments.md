# Body attachment observations

The body mirror draws where the weapon sockets and the pouch props are, as markers beside the skeleton, so the interaction targets can be compared with what Rico wears. It draws them from copied observations and never reads a weapon, a source model, or a model instance on the pose-finalization thread. The skeleton, the socket observations, and the pouch render matrices each keep their own sampling phase, so the mirror compares like with like within a phase and claims nothing across phases. The code is in `jc3vr-weapons` (`controllers::weapons::docks`, `begin::socket_candidates`, `pouch_models::attachments`, and `mirror`); the body crate reaches it only through `BodyContributions::attachment_sampler` and `controllers::body::mirror::submit_pouch_render`.

## Weapon sockets

On the input tick, `docks::socket_points` observes the socket weapons and returns a copied sample (character identity, timestamp, character transform, occupancy, item identities, and each socket's pose or why it has none). `socket_candidates` derives the interaction targets from that sample and publishes the same sample for the mirror, so the markers are exactly what a hand can reach. A socket's pose comes from a visible weapon grip, a remembered bone-relative grip, a native socket bone, or an offset from one; the native inventory placement owns the bones and offsets. With both hands empty, the long-gun restriction applies before the sample is published. Object identities are diagnostic only: they neither retain an object nor authorize a grab.

The mirror rebases the copied poses through the character's current transform, which follows platform movement without another bone read. A foreign character, an expired or future sample, or an invalid transform draws nothing. Rings are amber for occupied sockets, cyan for empty offers, red for no-fit offers, and gray for neutral estimates; a missing pose has no marker.

## Pouch props

The grenade and C4 pouches use the same volume helpers as their interaction targets, and the magazine prop uses the reload provider's pouch. `pouch_models::render_frame` computes each prop's displayed matrix and, right after applying it to the owned instance, copies the same value into `attachments::RenderSample`; `mirror::pouches_rendered` draws from that copy in the same render call, without rebuilding the matrix or reading native state. It draws a green ring at the applied model matrix and a cyan ring at the interaction centre joined to it; a hidden, missing, or mismatched model shows only a red interaction ring.

The pouch markers have their own overlay source, `Source::BodyMirrorPouches`, beside the skeleton's `Source::BodyMirror`. Every render call clears the previous pouch submission before any early return, as do a stand-down and an owner change, so an expired observation can never republish an old matrix.

These providers change placement values and diagnostics only: claims, ammunition, model ownership, and native inventory writes are untouched. Their tests drive the production adapters and the mirror renderer through the same plane.
