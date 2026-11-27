# Impacts

What a hand or a held object meeting something does beyond the physics: the impact sound, and for a held object its strike haptics and its blow on a character. A hand body's own blows on characters and its strike haptics are in [physical-hands.md, "Hits"](physical-hands.md#hits). The code is `physical_hands/sounds.rs` and `physical_hands/impacts/` in `crates/jc3vr-physical/src/controllers/`; the engine side is in [melee-and-damage.md](../../engine/gameplay/melee-and-damage.md). Both are always on under the controller scheme.

## Impact sounds

A hand that meets something hard enough makes the sound the game's own melee hit makes there. The player character's melee impact usage ([`AttachedEffectContainer::MELEE_IMPACT_USAGE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::attached_effects::AttachedEffectContainer)) is looked up against the struck material ([`AttachedEffectContainer::GetDynamicAttachedEffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::attached_effects::AttachedEffectContainer##func-GetDynamicAttachedEffects)) as the game's melee hit looks it up ([melee-and-damage.md, "The impact effect and sound"](../../engine/gameplay/melee-and-damage.md#the-impact-effect-and-sound)), and only the pair's sound is played ([`EffectAndSoundPair::PlayOneOffs`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::attached_effects::EffectAndSoundPair##func-PlayOneOffs) with the effect culled), at the contact, facing back along the hand's travel. The particle effect is left out, since a slap should not raise a cloud of dust. Every sound is one the game already plays for a punch or a kick on that material.

Three sources raise impacts:

- A palm's touch, at `sounds.min_touch_speed_mps` or faster. The palm's cast reads the hit shape's material ([`CastRayInfo::GetHitMaterial`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::CastRayInfo##func-GetHitMaterial)), so a slap on metal sounds like a punch on metal; a character plays flesh.
- A hand body's contact that carries `sounds.min_strike_share` of the strongest hit pulse. A contact with a character plays flesh; any other plays the default material, since a contact does not carry the struck shape.
- A held object's strike ([below](#held-objects)), with the same flesh-or-default choice.

The game's sound events take no loudness, so strength only decides whether an impact sounds. A hand sounds at most once every `sounds.hand_cooldown_s`, and the two hands together at most once every `sounds.min_interval_s`, so one slap that the palm's cast and the hand body both report sounds once, and a hand resting on a wall does not repeat. An impact the cooldowns hold back does not restart them. The thresholds are the `sounds` section of `crates/jc3vr-physical/tuning/hands.toml`. Weapon handling (a magazine seating, a grenade's pin) is not covered: those sounds are the weapons' own animation-driven attached effects, and a punch would be wrong for them. Breaking glass plays the glass melee impact with its effect ([breaking-glass.md](breaking-glass.md#feedback)).

Every source runs on the game thread outside the physics step: the palms' tick, the hand bodies' frame tick, and the held objects' delivery. The cooldowns are a leaf lock.

The `Impact sounds` status row, under FEEDBACK, counts the sounds played, the impacts the cooldowns held back, and the impacts whose material has no melee impact sound (`SILENT`), with the last sound's kind and material. The log has, at info level, `impact sounds: a hand made the melee impact sound` with the hand, the kind (`touch` or `strike`), the material, and the strength, and once `impact sounds: the player's melee impact has no sound for a struck material` with the first such material.

## Held objects

While a hand holds a body, carried or through a joint, the body's real solver contacts drive its strike haptics, its sound, and its blow on a character. A detached door held as a dynamic body uses the same path; tearing doors off is the doors' own ([vehicle-doors.md](vehicle-doors.md)).

### Observing the contacts

The observer detours [`HavokCollisionListener::onContactImpulseEvent`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::HavokCollisionListener##func-onContactImpulseEvent), records the contact, and calls the original dispatcher. It replaces no game-owned wrapper table and keeps no callback in a game object. The solver raises the event only for bodies whose contact-event flag is set, so the observer leases that one flag on the held body. It also detours [`hknpWorld::enableBodyFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##func-enableBodyFlags) and [`hknpWorld::disableBodyFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##func-disableBodyFlags): a native change to the flag during a hold updates the state restored on release and leaves the leased bit set, while unrelated flags and the caller's cache-rebuild mode pass through. Taking and returning the lease change only that flag, rebuilding collision caches only when it changes. Release, stale input, the controllers or pickup being disabled, leaving gameplay, and shutdown end the lease, and restoring checks the world, body, owner, and instance identity before changing a flag.

The pre-simulation callback samples the held body's motion after the hand drive. A contact's strike speed is the linear velocity plus the angular velocity crossed with the contact's offset from the centre of mass, at the manifold point with the largest impulse, along the normal toward the other body. Resting, tangential, separating, and initial-placement contacts give no feedback, nor do contacts with bodies in the player's system group or between two held bodies. Contacts with Rico's torso capsules are instead noted for the hold ([physical-hands.md, "The player's own body"](physical-hands.md#the-players-own-body)).

### Delivering feedback

Contacts are queued under the observer's lock with a timestamp and each holding hand's hold generation, and delivered on the game thread after the step. Delivery needs a fresh publication of what the hands hold, a recent contact (`impacts.contact_freshness_ms` in `characters.toml`), both participants proven live by identity, and the same hold generation for each receiving hand. A release or a gap in publications invalidates the hand's old contacts, even when it takes the same body again, and a shared contact reaches only hands that still hold the body. Native calls, sounds, and haptics run outside the lock.

Feedback is bounded by mass, speed, solver impulse, and a per-hand interval (the `impacts.feedback` section). It plays `Event::HandStrike` on each holding hand, scaled, and one impact sound, so a shared hold pulses both hands but sounds once.

### Striking characters

A character struck by a held prop takes the mod's calibrated strike (`blows::held_strike`, calibrated against the character's own thresholds as a hand's blow is, [physical-hands.md, "Hits"](physical-hands.md#hits)) whenever the swing is intentional and closing (the `impacts.strike` section bounds its momentum, and `impacts.max_struck_pairs` remembers recent pairs so one swing strikes once). The character's capsule turns every contact into a native hit-reaction call, but that reaction does nothing visible for a hand-held prop, so deferring to it would leave nearly every blow without effect. The observer detours [`PhysicsGameObject::DoReactToHit`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsGameObject##func-DoReactToHit) to see which participants the native reaction acted on, and logs `held impacts: the native hit reaction acted on the struck character as well` when it did, so a session shows any blow taken twice.

When the native reaction did not act on the held object, the held object takes impact damage from the strike (the `impacts.object_damage` section, as a [`DMG_TYPE_IMPACT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/damage::DamageFlags##flag-DMG_TYPE_IMPACT) message), so a crate swung into a soldier can break. Object-on-object contacts are left to the game, since they also feed the native damage controller's listener. Nothing else is added: no second impulse, no change to materials, bullet casts, penetration, collision filters, or tethers, and collision damage is not relabelled as a player melee attack. A held object is not made bulletproof; its protection is the game's own.

The Physical hands feature's readout has a `Held impacts` line counting the bodies listened to, the contacts, the feedback delivered, suppressed, and dropped.

## Outstanding checks

- Whether the player's melee impact has sounds for the materials hands meet most (concrete, metal, wood, glass), and whether a hand body's strike on the world, which plays the default material, sounds right beside the palm's touch on the same wall.
- Holding a prop with C4 attached, and a held prop on a grapple tether under spring load or reeling.
