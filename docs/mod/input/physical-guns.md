# Physical guns

A gun in a hand is a body of its own in the physics world, joined to the hand bodies that hold it ([physical-hands.md](physical-hands.md#hand-bodies)). Its weight hangs from the hands, it collides along its whole length, and a two-handed hold is two joints.

The code is split three ways: the gun bodies are the hand bodies' own (`crates/jc3vr-physical/src/controllers/physical_hands/physics/guns/`); the guns in the hands, the queued shots, and each step's reports are a store in the held-item facts (`crates/jc3vr-body/src/controllers/held/guns/`); and the weapon handling publishes into that store from `crates/jc3vr-weapons/src/controllers/weapons/gun_bodies/`. The tuning is `hand_bodies.guns` in `crates/jc3vr-physical/tuning/physics.toml`. The engine side is [havok-bodies.md](../../engine/physics/havok-bodies.md).

Gun bodies act on foot, on the ground and falling, including during grapple traversal. Where they do not (a parked hand body in a seat, a swim, the wingsuit, or the parachute; no body made yet; a heavy weapon with its own body in the world), the kinematic weight, synthetic recoil, and wall probe act on the hand instead ([weapon-handling.md](weapon-handling.md), [physical-weapons.md, "Guns against walls"](physical-weapons.md#guns-against-walls)).

## Which body

The gun's own body ([`WeaponData::m_PfxInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_PfxInstance)) is in the world only while the weapon lies dropped or is mounted, the weapon's state changes add and remove it, and the game can destroy it at any time, while the world does not end a constraint when one of its bodies is destroyed ([havok-bodies.md](../../engine/physics/havok-bodies.md#joining-bodies-with-a-fixed-constraint)). So the gun body is the mod's own, made as the hand bodies are, with the gun's own shape ([`hknpBody::m_shape`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody##field-m_shape), referenced by the new body) and its joints join only bodies the mod owns.

The game's body mass is the pickup's, heavy enough that the gun would swing the hand, so the mass is clamped between `min_mass_kg` and the class's `max_mass_by_class_kg`, the dual-wield pair's two guns share it, and a gun with no finite mass takes `fallback_mass_kg` (`guns::math::gun_mass`). It sits on the weapons layer in the player's collision group: it meets the world, props, characters, ragdolls, and vehicles, but not the player, the hands, or the aim's ray, so it never blocks a shot.

## The seat and the joints

The input tick publishes each hand's gun (`gun_bodies::read_holds`) only while the rigid grip has pinned it lately and no slide moves it. The next pre-simulation step checks that the gun's own body still belongs to it and is out of the world, and takes the gun's seat on the hand from its last placement ([`WeaponData::m_WorldMatrixT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_WorldMatrixT1)) in the frame of the controller's palm. It waits while the seat is further out than `max_grip_seat_m` (`max_carry_reach_m` for a gun sliding onto the grip), or while the hand body is further than `settled_shift_m` or `settled_turn_rad` from its palm, because a fixed joint keeps whatever error it is made with. Then it makes the body on its seat and joins it with a fixed constraint.

### The brace

While the other hand holds the foregrip or another support region ([below](#grabbing-a-gun-anywhere)), the step places that palm's body on the selected point of the gun body and joins it with a second fixed constraint. The brace keeps the palm frame it was made with and is remade only when the wanted frame moves past `brace.distance_m` or `brace.angle_rad` from it, and no sooner than `brace.min_interval_s`, so aim jitter never remakes it but a slide along the handguard does.

The holding hand, gun, and bracing hand move as one rigid body: each step gives the gun the holding hand's rigid velocity, then the bracing hand the gun's. Only the holding hand's drive steers, toward the two-handed pose; two drives each closing their whole error through one rigid gun would pull it between them on alternate steps. The drawn bracing hand takes the holding hand's render offset (`physics::render::apply_offsets`), so it stays on the gun.

The brace breaks when the tracked palm, measured before the two-handed pose moves it onto the gun, is further than `brace.break_distance_m` from where the brace holds it for `brace.break_steps` steps; the foregrip then lets go with `left the gun` (`held::guns::take_support_break`).

### Keeping the gun on its seat

Each step gives the gun body the hand's rigid velocity at its centre of mass, so the joint holds it on its seat rather than catching up a step late; what it meets still stops it. A body past the drawn clamps (`drawn_max_shift_m`, `drawn_max_turn_deg`) is steered back within the step (`guns::math::clamp_velocity`), and one past `stray_distance_m` or `stray_turn_deg` (pressed into a wall), or whose hand body was placed rather than driven (added, reset, snap turn), is put back on its seat with its bracing hand.

## Drawing the gun and the hands

The hands are drawn at their bodies ([physical-hands.md, "Drawing the hand at its body"](physical-hands.md#drawing-the-hand-at-its-body)) with no turn deadband on a gun hand. The rigid grip (`crates/jc3vr-body/src/controllers/hands/grip.rs`) pins the gun's attach bone at the canonical grip and carries it by the body's clamped deviation from its seat, so the drawn gun, its muzzle effects and shot origin, the sight line, and the reload's wells all agree with the body. The support and weight composition (`gun_bodies::apply`) supplies both the palm targets and the drawn reference hands, so the weight is not applied twice; the `keep_physical` stage puts the physical gun hands back after the synthetic recoil and wall probe.

## Weight, collision, and strikes

The hand driving the gun carries its mass, so a heavier gun lags a fast swing a little more; the droop and trail of a one-handed long gun are the class weight in the target, which the body follows. Pushed into a wall, it stops and the lagging hand pushes the player away, as a hand does.

The gun's contacts count as its holding hand's ([physical-hands.md, "Hits"](physical-hands.md#hits)). Swung into a character it is a blow with the momentum of the arm and gun together, so a rifle butt hits harder than a fist; one swing is one blow for hand and gun.

## Shots

Each shot kicks the gun body: an impulse of the class's `recoil_impulse_ns` back along the fire point's axis and `muzzle_rise` of it upward, at the muzzle, queued from the per-shot seam (`weapons::fire`, `held::guns::queue_shot`) and given at the next step.

A gun pointed at the player hits them. The game's bullets skip the bodies their holder names ([`WeaponBase::GetHolderInformation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-GetHolderInformation)), so under the controller scheme `weapons::self_hits` drops the local player's character from that list for its bullet weapons' shots ([`BulletWeaponBase::Fire_Projectile`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::BulletWeaponBase##func-Fire_Projectile)); the hit takes the game's damage path at full strength ([melee-and-damage.md](../../engine/gameplay/melee-and-damage.md#factions)).

## The lifecycle

The gun body is made, kept, and destroyed only in the pre-simulation step, on the game thread outside the physics step, and its joints always go before either body they join (`guns::step::destroy_slot`: the brace, the holder's joint, then the body). It goes when the gun leaves the hand, stops being published, changes between grip and support hold, or its own body re-enters the world. Every path that stops a hand's body first destroys its gun and brace (`guns::step::drop_hand`).

A heavy weapon the table carries is holstered on taking ([two-handed-inventory.md, "Heavy weapons"](two-handed-inventory.md#heavy-weapons)), so it gets a gun body; one the game wields itself keeps its own body in the world and is refused (`GunError::OwnBodyInWorld`).

## Grabbing a gun anywhere

A lone hand always holds a gun by its pistol grip, ready to shoot; any other hold is support for a gun the other hand holds by the grip. A second hand braces a long gun (the two-handed, special, and heavy slots) wherever it closes on it; a pistol keeps its grip. The code is `crates/jc3vr-weapons/src/controllers/weapons/regions/`, tuned by `hold.toml`'s `regions`.

The regions are worked out in the gun's own frame from the first gun's model bounding box ([`ModelInstance::GetModelSpaceBoundingBox`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstance##func-GetModelSpaceBoundingBox)), its fire point, its other-hand grip point ([`WeaponBase::GetGripPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-GetGripPosition)), and the wells table's magazine well and action handle ([reload.md](reload.md)). A hand within `regions.reach_m` of the box is in the nearest part with a point of its own (the pistol grip, the action, a long gun's magazine), or else in a region along the bore (the stock, the handguard, the barrel), closing on the point nearest the hand along it (`regions::classify`).

The hand on the grip holds and fires the gun. A second hand elsewhere braces it there: the foregrip hold takes that point, so the two-handed pose, the brace joint, and the finger pose ([grip.md, "Grip poses"](grip.md#grip-poses)) all put the hand where it closed. A brace ahead of the grip steers the gun as the foregrip does; a hand on the stock holds it without turning it. With `grip_to_hold` on, the holding hand letting go passes the gun to the bracing hand, which slides onto the grip over `regions.grip_slide_ms` (`regions::grip_from_brace`, `held::slide_offset`) and ends aligned to shoot. A lone hand taking a free gun anywhere takes it by the grip.

A trigger press on a gun the hand braces, or one the mod's fire path holds back (untracked, a mode without firing, a slide, a reload, draw, or holster, or the reload's fire hold), gives a dry-fire click ([haptics.md](haptics.md)) and one log line naming the reason, decided in one place (`weapons::finish::refused_fire`).

## Unverified

The game's masses for held guns' bodies (the fallbacks are guesses); whether the solver holds the three-body chain steady; whether the break distance is right; and whether the model boxes fit closely enough that a scope does not shift the regions.
