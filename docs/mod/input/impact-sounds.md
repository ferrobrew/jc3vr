# Impact sounds

A hand that meets something hard enough makes the sound the game's own melee hit makes there: a slap on a wall, a punch on a crate, a blow on a soldier. The code is `payload/src/controllers/physical_hands/sounds.rs`, and the engine side is in [melee-and-damage.md, "The impact effect and sound"](../../engine/gameplay/melee-and-damage.md#the-impact-effect-and-sound).

## What plays

The player character's melee impact usage ([`AttachedEffectContainer::MELEE_IMPACT_USAGE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::attached_effects::AttachedEffectContainer)) is looked up against the struck surface's material ([`AttachedEffectContainer::GetDynamicAttachedEffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::attached_effects::AttachedEffectContainer##func-GetDynamicAttachedEffects)), exactly as the game's melee hit looks it up, and only the pair's sound is played ([`EffectAndSoundPair::PlayOneOffs`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::attached_effects::EffectAndSoundPair##func-PlayOneOffs) with the effect culled), at the contact, facing back along the hand's travel. No new assets: every sound is one the game already plays for a punch or a kick on that material. The particle effect a punch throws up is left out, since a hand's slap should not raise a cloud of dust.

Two sources raise impacts:

- A palm's touch ([physical-hands.md](physical-hands.md)), when the palm meets a surface at `sounds.min_touch_speed_mps` (1.5 m/s) or faster. The palm's cast reads the hit shape's material ([`CastRayInfo::GetHitMaterial`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::CastRayInfo##func-GetHitMaterial), only while the sounds are on), so a slap on metal, wood, or concrete sounds like a punch on it; a character plays flesh.
- A hand body's contact ([physical-hands.md, "Hits"](physical-hands.md)), when it carries `sounds.min_strike_share` (0.35) of the strongest hit pulse. A contact with a character other than the player plays flesh (the character is found again as the blows find it); any other plays the default material, since a contact does not carry the struck shape.

The game's sound events take no loudness, so an impact's strength only decides whether it sounds. A hand sounds at most once every `sounds.hand_cooldown_s` (0.25 s), and the two hands together at most once every `sounds.min_interval_s` (0.06 s): one slap, which the palm's cast and the hand body may both report, sounds once, and a hand resting against a wall does not repeat. An impact the cooldowns hold back does not restart them.

Grabbing a ledge or a door sounds when the hand arrives fast enough to count as a touch or a strike; a gentle grip makes no sound. A magazine seating, a grenade's pin, and other weapon handling are not covered: their sounds are the weapons' own animation-driven attached effects, whose usage names were not found, and playing a punch for them would be wrong.

## Settings, readout, and log

`controllers.physical_hands.sounds.enabled` (on) switches them. It, the thresholds, and the cooldowns are under "Physical hands" in the debug UI's Controllers tab, with the counts and the last sound's kind and material.

The debug UI's Status tab has `IMPACT SOUNDS` under FEEDBACK: the sounds played, the impacts the cooldowns held back, and the impacts whose material has no melee impact sound (`SILENT`).

The log has, at info level, `impact sounds: a hand made the melee impact sound` with the hand, the kind (`touch` or `strike`), the material, and the strength, for each sound played (the cooldowns bound how often), and once `impact sounds: the player's melee impact has no sound for a struck material` with the first such material.

## Threading

Both sources run on the game thread outside the physics step: the palms' tick (the player aim's post-simulation update) and the hand bodies' frame tick. The lookup reads the player's own loaded usages; the sound system locks its own state when an event starts. The cooldowns are a leaf lock.

## In the headset

1. Open the debug UI's Status tab: `IMPACT SOUNDS` reads `WAITING`. Slap a concrete wall with an open hand: a punch-on-concrete sound at the wall, `a hand made the melee impact sound` with `kind=touch` (or `strike`), and the row counts one played. Slap it again at once: nothing until a quarter second has passed, and `HELD BACK` counts up.
2. Touch the wall slowly: no sound.
3. Slap a car, a wooden crate, and the ground: each sounds like a punch on its material. Report a material that is silent (`SILENT` counts up and the log names the material).
4. Punch a soldier: a punch-on-flesh sound with the blow.
5. Turn `physical_hands.sounds.enabled` off: slaps are silent and the row reads `OFF`.
6. Slap with both hands at once: two sounds at most, never a doubled one from a single hand.

## Open questions

- Whether the player's melee impact has sounds for the materials hands meet most (concrete, metal, wood, glass); the usage is data and was not surveyed.
- Whether a hand body's strike on world geometry, which plays the default material, sounds right beside the palm's touch on the same wall, which plays the surface's own; if they disagree, the strike could take the material of the palm's last cast.
- Whether the melee impact sounds too heavy for a slap: the events take no loudness, so the only control is the thresholds.
