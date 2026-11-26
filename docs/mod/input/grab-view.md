# The grab view

The grab view draws, around each hand, what the grip and the palms' reach saw on the last tick: the reach's rays and what they hit, a line from the hand to every grip target near it, a flash where a grab was refused, and the boxes of the things near the hands that a hand might take, whether or not the reach finds them. It is a diagnostic for telling, in the headset, why a weapon on the ground, a loose object, or a car door is not taken: whether it is a candidate at all, whether a ray reaches it, whether the reach takes what the ray hit, whether the resolver offers it, and whether the subsystem that acts on the grab refused it. The `GRIP` status rows and the grip press log ([grip.md](grip.md#readouts-and-logs)) say the same in text; the view puts it where the hand is. It is on by default. The code is `crates/jc3vr-physical/src/controllers/physical_hands/grab_view/`.

## Using it

The Grab view switch in the debug UI's visualization drawer shows and hides it, and so does the button at the top of its readout under the drawer's View readouts; the drawer is reachable in the headset on the floating panel. No key toggles it. The readout sits beside the collider view's ([collider-view.md](collider-view.md)), and the two can be on together. Its one setting is `controllers.physical_hands.grab_view.enabled`, on by default, and every part is drawn while it is on. The palms' state for each hand is in the readout and the grip press log rather than on the wrist, where a coloured disc would read to a player as a control. How far from a hand a target out of reach still gets a line, how far a candidate is drawn, and how many candidates are drawn are the views' tuning's `grab_view.draw.target_radius_m`, `grab_view.candidate_radius_m`, and `grab_view.max_candidates` (`crates/jc3vr-physical/tuning/views.toml`, [tuning.md](../tuning.md)). It is a diagnostic, so it has no pause menu entry ([settings.md](../settings.md)); switching `enabled` off is how a build for players hides it. The view draws through the overlay, so it needs `controllers.overlay.enabled`, and it is taken down while a menu is up.

The readout names each hand's palm state, counts what the last tick drew and what the budget left out, the candidates by kind with the time the last read of the world took, and the refused grabs with the last one's hand and reason.

## What it draws

The overlay has no text, so every state is a colour or a shape.

The reach rays. Each acting palm casts five short rays from the back of the hand through the palm ([physical-hands.md](physical-hands.md)); a hand its body drives casts them from its body. Each is drawn from where it started to where it hit, or to its end when it hit nothing, with a dot at the hit:

| Ray | Colour | What it found |
| --- | --- | --- |
| Missed | faint grey | nothing within the reach |
| Not taken | orange | something a hand does not take: the static world, a body that is not dynamic, too heavy, a weapon not lying dropped, or what the player stands on |
| In reach | green | something a hand takes, offered to the grip |

The door handles. Each door handle within a metre of a free palm is a ring on the handle: bright green when it is within the handle reach and offered, faint green when it is past the reach, and red when it is within the reach but no body was found at it, so it is not offered ([vehicle-doors.md](vehicle-doors.md)).

The lines to the targets. From each hand's grip a line runs to every grip target the resolver saw that tick, of the ones that hand may take, to where its marker is ([grip.md, "Markers"](grip.md#markers)). A target out of reach gets a line only within 2 m, and a socket or pouch on the body gets one only once it is in reach, since its own marker already shows it. A target held by the other hand gets none. At most 24 lines per hand are drawn, the nearest first:

| State | Line and marker | Meaning |
| --- | --- | --- |
| Out of reach | dim blue-grey, ring | the hand is not within the target's reach; brighter as the hand nears |
| In reach | pale yellow, ring | the hand is within reach, but another target wins |
| Chosen | magenta, disc | the target a squeeze would take |
| Held | cyan, disc | the hand holds it |

The marker at the target's end grows from 15 mm to 50 mm as the hand comes from 2 m away to the target's reach, so an approach is visible before the hand gets there.

The refusals. When a grab chose a target and the subsystem acting on it refused (the direct pickup of a weapon in the world, the take of a weapon from a character's hands, a rack, or a locker, and the physical hands' grip on an object, a door, or a character), a red ring flashes at the target for the tuning's `grab_view.refusal_flash_s`, growing as it fades. A refusal the physical hands make flashes where the reach met the object; one the weapon handling makes flashes at the marker of the target the hand chose on that input tick.

The candidates. The weapons lying dropped in the world (gold), the loose objects light enough for a hand (white) and those too heavy (red), and the vehicles and the parts loosened from them (green) whose boxes come within the candidate radius of a hand are drawn as their boxes, with a faint line from each box's middle to the nearest hand. One box is drawn per object, the nearest first. A candidate the reach never finds, or a weapon with no gold box, is the place to look when a grab does nothing.

The held gun's grab zones. For a gun held in one hand while the other is free, the zones that decide what the free hand's grip takes on it ([reload.md, "The magazine's zone"](reload.md#the-magazines-zone)) are drawn on the gun as it rides its hand: the magazine's zone under the well as an amber axis with a ring of its radius at each end, and the handover's reach around the pistol grip as a cyan ring of its radius. The zone the free hand is in is drawn bright; while the hand is in the magazine's zone the cyan ring dims, since the handover is not offered there. While the free hand holds a pouch magazine and the gun still holds its own, the knock zone ([reload.md, "Knocking the old magazine out"](reload.md#knocking-the-old-magazine-out)) is drawn instead of nothing in the hand: the inserted magazine as a red axis with a ring at the top and a heavier ring at the base, the held magazine's tip as a dot that turns green while it moves fast enough to knock, and the zone flashing white when it knocks.

## Where the data comes from

The palms' simulation tick (`physical_hands::tick::after_aim`) records each hand's reach rays and the door handles near it while the view is on, as the reach casts them (`pickup::reach` and `doors::scan::handle_reach` take a place to record into, and record nothing when it is not given). It publishes them after it has let go of its own state, with the head position it placed them against.

The candidates are read from the physics world on the same tick, outside the physics step, a few times a second rather than every tick (the tuning's `grab_view.candidate_period_ms`): the read goes through the collider view's ([collider-view.md, "Reading the world"](collider-view.md#reading-the-world)) with the static bodies left out, around the middle of the hands, and keeps at most the tuning's `grab_view.scan_max_bodies` bodies before they are told apart. Each moving body near a hand is named by its owner, the object whose engine instance holds the body, and told apart the way the reach tells what it hits apart: a vehicle or a part of one, a weapon lying dropped, or a loose object with a mass, light enough for a hand or not. A body without an owner, a weapon that is not lying dropped, a body without a mass, and the body the player stands on are not candidates.

The zones are the ones the weapon handling's input tick resolved before the grip model ran (`reload::zones::views` and `reload::knock::view`), drawn only while they are fresh. The grab view reads them through the held-item seam (`held::reload_views`, in the body crate), which the weapon handling fills, so the physical hands do not name the weapon handling.

The refusals are noted where they are made: the direct pickup's refusal in `pickup::finish_tick`, a take's refusal in `weapons::hold::take::finish_grab`, and the pickup tick's refused grips, published after the tick.

The input tick draws it all after the grip model's own finish (`bindings::input_tick`), with the grip model's targets and resolution of the same tick, and submits it to the overlay's own slot, which lives a little past one tick.

## Anchoring

Everything is anchored at render time ([render-time-anchoring.md](../rendering/render-time-anchoring.md)). The hands' ends of the lines follow the hands as they are drawn. A target's end of its line is where its grip marker is, anchored as the marker is. The reach rays and their hits ride with the player, so they are anchored to the render-time head at their offset from the head of the tick that cast them. The door handles, the candidates' boxes, and a refusal the physical hands made lie in the world and are world anchors; the candidates are read a few times a second, so a box on something moving lags it by up to one read.

## Cost and caps

The reach records five rays and a few handles per hand per tick, only while the view is on. The read for candidates is one pass over every body slot up to the world's peak id, as the collider view's read is, a few times a second, with a capped number of bodies kept and named; the readout shows its time. The drawing is capped at the tuning's `grab_view.draw.primitive_budget` primitives a tick: the refusals, the rays, and the handles first, then the target lines, then the held gun's zones, then the candidates, each candidate's box whole or not at all, with what does not fit counted as over budget. The view's slot comes before the collider view's in the overlay, so when the overlay is full the collider view gives way first.

## Threading

Everything runs on the game thread: the palms' tick, and the input tick's game stage. The view's own state is a leaf lock, taken by the palms' tick after it has let go of its own state, by the refusals where they are made, and by the input tick's game stage; the overlay store's and the readout's leaf locks are taken briefly under it. The palm readout, which takes the palms' own state, is read before the view's lock. The debug UI reads the readout's lock on its own thread.

## Headset checklist

1. Hold an empty hand over the ground: five grey rays fan out of the palm. Lower it onto the ground: the rays that reach it turn orange, with dots where they hit.
2. Hold the hand over a crate: the rays on the crate turn green, a magenta line runs from the hand to the crate's marker, and a squeeze takes it. Try one too heavy: its box is red, the rays orange, and no line.
3. Drop a weapon and look at it: it has a gold box with a faint line to the nearer hand. Reach for it: the rays on it turn green and the line to its marker magenta. If the take is refused, a red ring flashes on it, and the readout names why.
4. Stand beside a parked car: its box is green, and as a hand comes within a metre of a door's handle, a faint green ring sits on the handle, bright once the hand is within reach. A red ring there means no body was found at the handle.
5. Move a hand toward the holster with a gun drawn in the other hand: the line to the holster appears as the hand comes into reach and turns magenta when it would be taken.
6. Hold a pistol in one hand: an amber cylinder hangs under its grip and a cyan ring surrounds the grip. Move the other hand into the cylinder: it brightens, and the cyan ring dims.
7. Turn the view off with the button: everything it drew goes, and the grip markers stay.
