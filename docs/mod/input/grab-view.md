# The grab view

The grab view draws, around each hand, what the grip and the palms' reach saw on the last tick: the reach's rays and what they hit, a line from the hand to every grip target near it, a flash where a grab was refused, and the boxes of the things near the hands that a hand might take. It is a diagnostic for telling, in the headset, why a weapon on the ground, a loose object, or a car door is not taken: whether it is a candidate at all, whether a ray reaches it, whether the reach takes what the ray hit, whether the resolver offers it, and whether the subsystem acting on the grab refused it. The `Grip` status rows and the grip press log ([grip.md](grip.md#readouts-and-logs)) say the same in text. The code is `crates/jc3vr-physical/src/controllers/physical_hands/grab_view/`, and its sibling for the physics world's bodies is the collider view ([collider-view.md](collider-view.md)).

## Using it

The view is on by default (`controllers.physical_hands.grab_view.enabled`). The Grab view switch in the debug UI's visualization drawer, reachable on the floating panel, shows and hides it, as does the button at the top of its readout under the Physical hands page's Diagnostics. It is a diagnostic, so it has no pause menu entry ([settings.md](../settings.md)). It draws through the overlay, so it needs `controllers.overlay.enabled`, and it is taken down while a menu is up. Its reach and budgets are the `grab_view` section of `crates/jc3vr-physical/tuning/views.toml` ([tuning.md](../tuning.md)).

The readout names each hand's palm state, counts what the last tick drew and what the budget left out, the candidates by kind with the time the last read of the world took, and the refused grabs with the last one's hand and reason. The palm state stays in the readout rather than on the wrist, where a coloured disc would read to a player as a control.

## What it draws

The overlay has no text, so every state is a colour or a shape.

The reach rays: each acting palm's five rays ([physical-hands.md](physical-hands.md#picking-things-up)), cast from its body when its body drives it, drawn from where each started to its hit or its end, with a dot at the hit:

| Ray | Colour | What it found |
| --- | --- | --- |
| Missed | faint grey | nothing within the reach |
| Not taken | orange | something a hand does not take: the static world, a body that is not dynamic, too heavy, a weapon not lying dropped, or what the player stands on |
| In reach | green | something a hand takes, offered to the grip |

The door handles near a free palm, as rings: bright green within the handle reach and offered, faint green past it, and red within it but with no body found at the handle, so not offered ([vehicle-doors.md](vehicle-doors.md)).

The lines to the targets: from each hand's grip, a line to every grip target the resolver saw that tick that the hand may take, ending at its marker ([grip.md, "Markers"](grip.md#markers)). A target out of reach gets a line only within `grab_view.draw.target_radius_m`, a socket or pouch only once it is in reach, and a target the other hand holds none. At most `grab_view.draw.max_target_lines` lines per hand are drawn, the nearest first. The marker grows from `grab_view.draw.min_target_mark_m` to `grab_view.draw.max_target_mark_m` as the hand closes on the target's reach:

| State | Line and marker | Meaning |
| --- | --- | --- |
| Out of reach | dim blue-grey, ring | the hand is not within the target's reach; brighter as the hand nears |
| In reach | pale yellow, ring | the hand is within reach, but another target wins |
| Chosen | magenta, disc | the target a squeeze would take |
| Held | cyan, disc | the hand holds it |

The refusals: when a grab chose a target and the subsystem acting on it refused (the direct pickup of a dropped weapon, the take of a weapon from a character's hands, a rack, or a locker, and the physical hands' grip on an object, a door, or a character), a red ring flashes at the target for `grab_view.refusal_flash_s`, growing as it fades. A refusal the physical hands make flashes where the reach met the object; one the weapon handling makes flashes at the chosen target's marker.

The candidates: the weapons lying dropped (gold), the loose objects light enough for a hand (white) or too heavy (red), and the vehicles and their loosened parts (green), whose boxes come within `grab_view.candidate_radius_m` of a hand, drawn as their boxes with a faint line to the nearest hand, at most `grab_view.max_candidates`. A candidate the reach never finds, or a weapon with no gold box, is the place to look when a grab does nothing.

The held gun's grab zones: for a gun held in one hand while the other is free, the zones that decide what the free hand takes on it ([reload.md, "The magazine's zone"](reload.md#the-magazines-zone)) are drawn on the gun: the magazine's zone as an amber axis with a ring of its radius at each end, and the handover's reach around the pistol grip as a cyan ring, the zone the free hand is in drawn bright. While the free hand holds a pouch magazine and the gun still holds its own, the knock zone ([reload.md, "Knocking the old magazine out"](reload.md#knocking-the-old-magazine-out)) is drawn instead: the inserted magazine as a red axis, the held magazine's tip as a dot that turns green while it moves fast enough to knock, and a white flash when it knocks.

## Where the data comes from

The palms' tick (`physical_hands::tick::after_aim`, run on the game thread) records each hand's reach rays and the door handles near it while the view is on (`pickup::reach` and `doors::scan::handle_reach` take a place to record into, and record nothing without one), and publishes them after it has let go of its own state, with the head position it placed them against.

The candidates are read on the same tick, every `grab_view.candidate_period_ms` rather than every tick, through the collider view's read of the world ([collider-view.md, "Reading the world"](collider-view.md#reading-the-world)) with the static bodies left out, keeping at most `grab_view.scan_max_bodies` bodies around the middle of the hands. Each body is named by its owner and told apart as the reach tells its hits apart; a body without an owner, a weapon not lying dropped, a body without a mass, and the body the player stands on are not candidates.

The zones are the ones the weapon handling's input tick resolved before the grip model ran, read through the held-item seam (`held::reload_views`, in the body crate), which the weapon handling fills, so the physical hands do not name the weapon handling. The refusals are noted where they are made: `pickup::finish_tick`, the weapon handling's `hold::take::finish_grab`, and the pickup tick's refused grips.

The input tick's game stage draws it all after the grip model's own finish (`physical_hands::grab_view::submit`, called from the bindings' input tick), with that tick's targets and resolution, into the overlay's own slot, which lives `grab_view.ttl_ms`.

## Anchoring

Everything is anchored at render time ([render-time-anchoring.md](../rendering/render-time-anchoring.md)). The hands' ends of the lines follow the hands as they are drawn, and a target's end is anchored as its marker is. The reach rays ride with the player, so they are anchored to the render-time head at their offset from the head of the tick that cast them. The door handles, the candidates' boxes, and a physical-hands refusal lie in the world and are world anchors; since the candidates are read a few times a second, a box on something moving lags it by up to one read.

## Cost and threading

The reach records five rays and a few handles per hand per tick, only while the view is on. The candidate read is one pass over the world's body slots, a few times a second, with its time in the readout. The drawing is capped at `grab_view.draw.primitive_budget` primitives a tick, in order: the refusals, rays, and handles, then the target lines, the gun's zones, and the candidates, each candidate's box whole or not at all, with the rest counted as over budget. The view's overlay slot comes before the collider view's, so when the overlay is full the collider view gives way first.

Everything runs on the game thread. The view's own state is a leaf lock taken by the palms' tick after it has let go of its own state, by the refusals, and by the input tick; the overlay store's and the readout's leaf locks are taken briefly under it, and the palm readout is read before it. The debug UI reads the readout's lock on its own thread.
