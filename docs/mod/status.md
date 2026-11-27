# Status rows

The status rows say whether each of the mod's systems is live, for confirming a headset session at a glance. The code is `crates/jc3vr-ui/src/status/`; the debug UI's Status feature ([debug-ui.md](debug-ui.md#navigation)) shows every row and logs them as a summary on request.

Every row's line is a state word followed, after a comma, by a short reason or the key numbers, and sometimes a longer note:

- `Ok`: live and doing what it should.
- `Off`: switched off, or not used in the current setup.
- `Waiting`: on, but waiting for a session, the player, or a measurement.
- `Failed`: on and broken.

The rows are grouped in sections, listed top to bottom in `status::SECTIONS`: Rendering, Input, Capture and tools, HUD, Gun in hand, Weapons in hand, Inventory, Explosives and magazines, Aiming, Hand bodies, Grip, Hands in the world, Feedback, Fingers, Grapple, Vehicles, Locomotion, and Procedural body. What a row reads is documented on its constant and in the subsystem's own doc.

## One spelling

Each section module owns its rows as `status::Row` constants (`status::feedback::HAPTIC_CUES`, for example), and a row's formatter is the one place its status is turned into text. A feature page that has a row shows it as its headline through `ui::status_row::headlines`, with the same state word, detail, and note as the Status feature, and keeps only the lines that add detail the row has no room for, so the two never disagree.

The Status feature reads every row each frame it is drawn and lists the rows that need attention first. Its `Log the summary` button logs every row once at info, as one line starting `status:`, each row as `label: value` with its note in parentheses, and sections separated by `|`.

## Reading a row from any thread

A row may be read on any thread, including in the middle of a frame while the game thread holds the VR runtime lock ([threading.md](threading.md)), so each reads only what its module publishes for any thread: atomics, or a leaf lock nothing holds while it waits, one at a time, and never the runtime state. Some rows read the configuration, so a caller holds no configuration lock while it reads them. Where a subsystem's own state sits behind a gate or a lock held across its tick, it publishes a copy for the rows: the procedural torso's status is copied after its gate lets go of its own state (`body::procedural::published_status`), so a reader never waits on a gate in progress, and the finger curls publish their last tick apart from their own state (`controllers::fingers::status`).
