# Weapon sockets

Every weapon on the body sits on one of the loadout table's four sockets, each hip and each shoulder ([two-handed-inventory.md](two-handed-inventory.md)). What a grab at a socket does is the table's decision (`loadout::grab`): an empty hand draws, and a hand holding a weapon docks it on a free socket that fits. This page is what the hands see and feel there; the rest of the hand-facing handling is in [physical-weapons.md](physical-weapons.md). The numbers are the hold tuning's `docks`, `volumes.sockets`, and `hold` sections (`crates/jc3vr-weapons/tuning/hold.toml`).

## The volumes

The sockets' volumes are seated from the table each input tick and reached by the hand as it is drawn (`weapons::docks`), so what the player sees on the body is what they grab, wherever the animation or the body rig puts the bone:

- an occupied socket has a volume on the weapon's grip ([`WeaponBase::GetGripPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-GetGripPosition)) for each hand that may draw it;
- a free socket that takes a hand's weapon has a volume where that weapon would sit: where its grip was on the bone when last seen there, or else at the bone plus the socket's offset;
- a socket that refuses a hand's weapon (taken, or too small) shows that hand a small red disc while near. A weapon without a slot docks nowhere (`hold::act::dockable`).

A hip's reach is wide enough for a hand that closes on the front of the hip rather than the pistol's grip behind it. The shoulders are reached for docking lower and further out than where the weapon sits (`volumes::reach_back_sockets`), since a hand putting a long gun away over the shoulder closes below and outside its grip; a draw is still reached on the weapon the player sees.

Seated, the drawn thighs are wherever the seat's animation puts them, so the hips hang at a waist below the player's neck by the standing spine length (`docks::seated_waist`, `docks::place_waist`). The neck is latched when the posture first reads seated, until the player stands or the cockpit frame changes (`flight_pose::epoch`), so leaning toward the knees does not carry a docked gun out through a cabin wall.

## Drawing and docking

A draw and a dock take no act: the gun is the same object throughout and slides between socket and hand over `hold.snap_s` (`hold::act`, pure parts in `hold::dock`). Since no clip plays, the draw plays the gun's draw sound and the dock its holster sound ([reload.md, "Sounds"](reload.md#sounds)).

A draw checks [`Inventory::GetAnimationAllowsDrawing`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-GetAnimationAllowsDrawing), measures the gun's placement relative to the socket's bone, and tells the table, whose reconcile wields or arms it ([two-handed-inventory.md, "The per-hand tick"](two-handed-inventory.md#the-per-hand-tick)). The gun rides the hand's attach bone at once, and the rigid grip slides that bone from the measured pose on the moving bone toward the hand (`hands::grip::slide_grip`).

A dock of the wielded weapon checks [`GetAnimationAllowsHolstering`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-GetAnimationAllowsHolstering). The gun slides to where it sat at its last draw off that socket, held from firing, and the table takes the dock when the slide ends; a weapon never drawn off that socket docks at once. Each hand has its own slide, so both hands draw and dock at once, and a dock pulses `Event::VolumeGrab` on the docking hand.

A dock or attracted travel begun by opening the grip is called off only by a fresh grip press (`hold::act::pressed_grips`), which takes the gun back. The Index's force-sensing grip releases its press while the fingers still rest on the handle, and its squeeze reads those fingers as a hold, so calling a dock off on the squeeze would drop the gun whenever a hand opens slowly.

## Attraction

A socket's reach is tight, so a grab between two sockets takes the nearer, but putting a weapon back must not need that precision: the shoulders are out of sight, and a seated player's chair stands between the hand and the shoulder blade. So every free socket that takes a hand's weapon also attracts a release that the reach missed (`weapons::docks::attract`), from about an arm's length for a shoulder and anywhere beside the thigh for a hip.

Two limits keep a deliberate drop a drop. A shoulder attracts only a hand not far in front of or below the line through Rico's shoulder joints, so a weapon tossed down by the thigh is never drawn onto the back; a hip only a hand not far in front of the line through his hip joints, deep enough for a pistol turned onto the front of the thigh. And a hand moving fast just before the release is throwing, and the weapon falls with the throw.

A long gun the other hand braces is not attracted. Of several attracting sockets, the one the hand is deepest inside, as a share of its radius, takes it; a socket the other hand's weapon is already heading for is left out, so two guns let go over one shoulder go to two sockets. The weapon travels onto the socket along the dock's slide, eased, held from firing, and docked when it arrives (`hold::act::attract`).

### The swap

A taken socket that would take the held weapon if free attracts like a free one, and is marked as accepting rather than with the red disc (`attract::swap_attractors`, `loadout::grab::swaps`). A release it draws in first ejects the docked weapon out from the body and a little up, so it falls clear (`hold::swap::eject`), and the table moves it to the world as the game's own drop does; the released weapon then docks, and the hand feels `Event::SocketSwap` instead of the dock's click. A free socket in reach always wins over a taken one.

### Feeling the attraction

A hand holding a weapon that enters an attracting socket's zone gets one tick (`attract::ZoneWatch`, `Event::DockAttraction`) saying a release there docks it; that is how the unseen shoulders are found. It ticks again only after the hand has left by a margin and come back, so a hand resting on the edge does not buzz, and the zone it reports is the socket a release would take. The tick ignores speed, so a hand swinging through ticks even though a throw from there would not dock.
