# The character ragdoll

A character's ragdoll is a set of Havok bodies, one per ragdoll bone, joined by ragdoll constraints. It exists for every character with a ragdoll resource, standing or not; what changes is who moves it. This doc covers how to tell a ragdolling character, how to reach its bodies, what puts a character into its ragdoll and takes it out again, and how the game marks the characters it protects. The hit reaction that knocks a character down is in [melee-and-damage.md](../gameplay/melee-and-damage.md#the-hit-reaction).

## The ragdoll instance

The character holds its ragdoll in [`m_Ragdoll`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_Ragdoll), a shared pointer to a [`RagdollInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ragdoll::RagdollInstance) (`NAnimationSystem::CRagdollInstanceImpl`). The instance keeps the physics world its bodies live in and the Havok ragdoll itself ([`hknpRagdoll`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpRagdoll)), whose [`m_bodyIds`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpRagdoll##field-m_bodyIds) lists one body id per ragdoll bone, in the order of the ragdoll's skeleton, with an invalid id for a bone without a body.

Each body gets an engine instance of its own whose owner is the ragdoll's owner, the character; that a physics query hitting a ragdoll body therefore reports the character as the object hit is unverified. The instance's other members (the bone-to-body maps, the closest-body and impulse helpers, the activation of sleeping bodies) are known by name and not bound.

## Ragdolling

While the animation drives a character, its ragdoll's displacement is controlled by its owner: the bodies follow the animated pose and are not free. [`Character::IsRagdolling`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsRagdolling) is true when the character has a ragdoll whose displacement its owner does not control: the bodies simulate as dynamic bodies, collide, and fall, and the character follows them. A knocked-down character, a thrown or tethered one, and a dead one ragdoll this way.

A dead character is not a ragdoll at once: its death animation may play first, and until it ragdolls its bodies are not free. A settled ragdoll's bodies go to sleep with the rest of the world; they stay dynamic. The instance wakes them body by body when the game needs them moving; that Havok's own velocity and impulse setters wake a sleeping ragdoll body too is unverified.

The hit reaction's ragdoll path checks ragdolling the same way: a character already ragdolling takes a hit's impulse onto its ragdoll for some damage types and otherwise ignores it.

## Into the ragdoll

A non-player character goes into its ragdoll when a hit's scaled impulse reaches its knockdown threshold: [`Character::HitReaction`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-HitReaction) queues the hit reaction's ragdoll act (`ACT_HITREACT_RAGDOLL`, unbound) and the impulse onto the ragdoll. The faction relation plays no part in it. Two things keep a character standing whatever the impulse:

- the state bit [`SB_SKIP_HITREACTION_RAGDOLL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_SKIP_HITREACTION_RAGDOLL), which sends the hit to the stumble instead;
- a damage-state flag on the character ([`m_DamageFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_DamageFlags)) that has the reaction return before doing anything.

The reaction moves the character without taking health; called without a damage message it knocks a character down without harming it. The ragdoll and the get-up are animation-graph states; what the character's AI does while it lies in its ragdoll is not established.

## Out of the ragdoll

The get-up is the animation graph's own: the `NGetUpFromRagdoll` state task runs it, entered from the ragdoll states through the graph's conditions, among them `CRagdollIsSettledCondition` (the ragdoll has come to rest), `CRagdollingTimerCondition` (the time spent ragdolling against a threshold from the graph's data), and `CBlockedGetUpCondition`. None of these is bound, and the thresholds are data the graph loads; that a ragdoll kept moving by an outside force still gets up once its timer runs out is unverified. While the get-up runs the character has [`SB_IS_GETTING_UP_FROM_RAGDOLL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/state::StateBitFlag##variant-SB_IS_GETTING_UP_FROM_RAGDOLL), and its ragdoll goes back under its owner's control. A dead character never gets up.

The ragdoll's collision listener (`CRagdollCollisionListener`) gathers its bodies' hardest contacts and reports them to the character as impacts. That these impacts harm a ragdolling character through its impact thresholds, so that a body thrown hard against a wall can die of it, is unverified.

## Protected characters

The game marks the characters a mission depends on: [`m_IsMissionCriticalCharacter`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_IsMissionCriticalCharacter), which the character rule that spawns the character copies from its data. The interaction graph and the AI's vehicle entry read it; the hit reaction does not, so a mission-critical character ragdolls like any other. Beside it sit the character's melee invulnerability ([`m_InvulnerableToMelee`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_InvulnerableToMelee)) and its invulnerability to other characters ([`m_InvulnerableToOtherAI`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_InvulnerableToOtherAI)).

A character a scripted sequence drives has [`m_InSequence`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_InSequence) set until the sequence lets it go.
