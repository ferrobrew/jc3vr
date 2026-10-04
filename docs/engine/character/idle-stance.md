# The on-foot idle and how its stance is chosen

Rico's on-foot idle is one rule state in the base graph, `S_IDLE`, whose clip is chosen by animation modifiers rather than by separate states. This doc covers which clip plays, what sets the modifiers that choose it, and how the aim state and the aim-focus states relate to it. Animation names come from Rico's shipped base graph and its animation set (`animations/jc_statemachine/main_characters/rico/rico_base.afsmb` and `rico_base.asb`, from `patch_win64/game1`). Related: [aim-state.md](../gameplay/aim-state.md) ("Relaxed and aimed holds"), [animation-layers.md](animation-layers.md) (the layers the modifiers reach), and [humanik.md](humanik.md).

## The idle's clip variants

In the animation set, the `S_IDLE` state's node is `ANIM_IDLE`. Its own clip is the unarmed relaxed idle (`locomotion/idle/rico_loco_idle_noncombat.ban`, a full-body clip), and it lists one variant node per combination of active modifiers. The variants that matter on foot, by the lower-body clip they play:

| Active modifiers | Lower body | Upper body |
|---|---|---|
| none of the below | the unarmed relaxed idle (full body) | the same clip |
| a weapon's held modifier (`WEAPON_MODIFIER_ASSAULT`, `_REV`, `_RPG`) | `rico_loco_idle_rifle_noaim_lower` | the weapon class's held arms and head-torso, and its hand pose |
| a weapon's aim modifier (`WEAPON_MODIFIER_AIM_ASSAULT`, `_AIM_REV`, `_AIM_RPG`) | `rico_loco_idle_rifle_aim0_lower` | the aimed arms or upper body |
| the explosive's modifier (`WEAPON_MODIFIER_C4`) | `rico_loco_idle_rifle_aim0_lower` | the explosive priming arms |
| `GRAPPLE_MODIFIER`, alone or with an equipped modifier | `rico_loco_idle_rifle_aim0_lower` | the grapple arm |
| `STUNT_COMBAT_MODIFIER` or `STUNT_NONCOMBAT_MODIFIER` | the stunt idles | the stunt idles |

So every armed idle is a combat stance: the held stance for a wielded rifle, pistol, or launcher, and the aimed stance while aiming, holding a grapple tether, or holding an explosive. The only square, relaxed idle in the base graph is the unarmed clip. Whether the held stance's lower body is the bladed one-foot-forward pose has not been checked against the clip data, which was not decoded.

`S_IDLE_AIM_FOCUS` is not the aimed idle for ordinary weapons: its node, `ANIM_IDLE_AIM_FOCUS`, plays the minigun's idle and lists no variants. The aim-focus family's idle for the other weapons is `S_IDLE` with an aim modifier.

## What sets the modifiers

[`Character::SetCharacterModifiers`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-SetCharacterModifiers) runs once per tick from `CCharacter::UpdatePassPreStateMachines_Parallel`, after the aim input and before the state tasks, and sets every modifier frame-bound ([`AnimationModifierType::FRAME_BOUND`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::rule_system::AnimationModifierType##variant-FRAME_BOUND)), so each tick's set is rebuilt from that tick's state:

- `GRAPPLE_MODIFIER` on every rule system while the hook is in its aiming state ([`GrapplingHook::IsHookAimingState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-IsHookAimingState), a live tether).
- For the wielded weapon (the planted explosive while planting; otherwise the selected slot's weapon while it is wielded, dual-wielded, or mounted; otherwise the weapon being switched to or drawn), through [`Character::SetModifier`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-SetModifier): the weapon's [`WeaponBase::m_AimAnimationModifier`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_AimAnimationModifier) when [`Character::IsTryingToAim`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsTryingToAim) holds and the hook is not aiming, its [`WeaponBase::m_AnimationModifier`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_AnimationModifier) otherwise, and always its [`WeaponBase::m_EquippedAnimationModifier`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_EquippedAnimationModifier).
- For characters other than the player, a faction or ghost modifier.
- `RELAX_MODIFIER` while the avatar's mental state is [`MentalState::E_RELAXED`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::avatar::MentalState##variant-E_RELAXED). `ANIM_IDLE` lists no variant for it.
- A stunt modifier, combat with a weapon and non-combat without, while attached to a vehicle or while stunt mode is forced.

`Character::SetModifier` forwards to [`AnimationRuleSystem::SetModifier`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::rule_system::AnimationRuleSystem##func-SetModifier) for every rule system of the animated model ([`AnimatedModel::m_RuleSystems`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##field-m_RuleSystems), the body's first), which sets the type's bit in the rule system's modifier map. For the player, the weapon's two calls are the only `Character::SetModifier` calls `SetCharacterModifiers` makes; the grapple, relaxed, and stunt modifiers go to the rule systems directly.

`IsTryingToAim` is true while the aim flags report a weapon aim ([`AimState::m_AimingWeapon`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AimState##flag-m_AimingWeapon)), while the weapon's fire input is past its trigger sensitivity, or while a force-fire timer, count, or input buffer is live ([aim-state.md, "Related queries"](../gameplay/aim-state.md#related-queries)). The weapon's aimed stance therefore follows the trigger and the aim flag, not which state family the lower body is in.

## Reaching the idle

The idle state is entered by the stop acts the on-foot movement tasks queue when the stick is released. The movement task's stop branch queues the aim-relative stop turns while the aim flags report aiming and the plain stops otherwise; with a weapon wielded both queue a stop-turn act. In `S_IDLE` the idle task ([`NStateTask_InputLocoIdleTask::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::locomotion::NStateTask_InputLocoIdleTask##func-Update)) queues nothing while the character stands still with the aim flags clear, outside precision aim, and with no heavy weapon: it only runs the idle variation countdown. With the aim flags set it queues the reset rotate-on-spot acts that turn the body toward the aim, and with a heavy weapon it queues the aim-relative on-spot acts. Which states the stop acts pass through on the way to `S_IDLE` was not traced, because the graph's transition wiring is hashed.

So the stance the player sees while standing is decided by the modifiers alone: the held stance for a wielded weapon, the aimed stance while the trigger is pulled or a tether is held, and the unarmed relaxed clip only with nothing wielded. In normal play, the game reaches the relaxed clip by holstering automatically once the mental state turns relaxed ([aim-state.md, "The mental state"](../gameplay/aim-state.md#the-mental-state)).
