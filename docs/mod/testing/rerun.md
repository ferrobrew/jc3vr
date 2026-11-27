# Viewing a recording in Rerun

`tracking-synth export-rerun` writes a tracking recording, or a stretch of it, as a [Rerun](https://rerun.io) recording (`.rrd`), so a session can be scrubbed in the Rerun viewer with the take's video beside the 3D scene, the plots, and the event log. The code is `tools/tracking-synth/src/rerun_export/`; the recording format it reads is described in [recorder.md](recorder.md).

## Running it

```sh
cargo run --release -p tracking-synth --target x86_64-unknown-linux-gnu -- \
    export-rerun --from 1860 --to 1980 \
    --video sessions/<session>/recordings/<take>/video.mkv \
    sessions/<session>/recordings/<take>/tracking.jc3trk -o take.rrd
rerun take.rrd
```

The capture host exports every take whole, with its video, when the take ends: `scripts/rerun_take.sh <take folder>` writes `take.rrd` into the take's folder, and can be run by hand on any take ([capture.md](../capture.md#the-relay)). After the export it scans the video for flashes ([view-scan.md](view-scan.md)).

`--from` and `--to` take the same times as `dump`. An hour-long take writes about half a gigabyte without its video, so a stretch of a few minutes is the usual way, and decoding the recording takes about 2.5 GB of memory whatever the stretch. `--video` adds the take's video, placed on the recording's clock through the take's `frames.jsonl` and `stream.json`; `--video-height` sets the cut's height. `--height`, `--eye-height`, `--tuning`, and `--weights` are `rig-eval`'s, for the offline rig ([rig.md](../body/rig.md#offline-evaluation)).

Several stretches can be merged into one recording on one timeline: `--recording-id <id>` writes under a given recording id, and `--no-blueprint` leaves out the default layout, so `rerun rrd merge` can join the exports, and an overlay from another tool under the same id, without competing layouts. What differs between stretches (the video cut, Rico's trail, and the `about` note) is logged at the stretch's start rather than statically, so each stretch keeps its own.

Rerun reads video only from MP4 and a take is recorded to MKV, so the tool cuts the stretch with `ffmpeg` (on `PATH`) and encodes it as H.264 without B-frames and with a keyframe every second, so the cut starts at the exact frame and scrubs back cheaply. A long stretch is cut into ten-minute segments, each its own video asset, because the viewer shows no video for an asset of an hour's take. The cut is copied into the `.rrd` and deleted.

### The viewer

The SDK is pinned to the Rerun release nixpkgs ships (`tools/tracking-synth/Cargo.toml`), and the viewer must be that version or newer:

```sh
nix shell nixpkgs#rerun          # or: cargo install rerun-cli --version =<pinned> --locked
rerun take.rrd
rerun rrd verify take.rrd        # checks that the file loads, without a window
```

The native viewer decodes H.264 through `ffmpeg` on `PATH`. The export is behind the `rerun` cargo feature of `tracking-synth`, on by default; `--no-default-features` builds the tool without it.

## What it shows

Everything is on the `recording` timeline, in seconds on the recording's clock. A `wall_clock` timeline is added when the take's sidecar sits beside the recording, or otherwise when the recording has a take marker to anchor it. The default layout (`rerun_export/blueprint.rs`) has the 3D scene and the plots on the left, the video, the event log, the state lanes, and the export's notes (`about`) on the right, and the coarse movement lane across the bottom.

### The 3D scene

`world` is the game's world, Y up, less an origin near the stretch's first root position (given in `about`), so the viewer works near zero. Velocity arrows show a quarter-second's travel. Under it:

- `rico` and `rico_trail`: Rico's root with his measured and wanted velocities and his heading, and his path through the stretch.
- `pose/pre_ik` (grey) and `pose/final` (orange): Rico's body bones before the HumanIK solves and as they rendered, under the sample's character transform, with the main pass's live HumanIK position targets under `pre_ik/ik_targets`.
- `cockpit`: the cockpit frame placed in the world (below), with the head, each hand's grip and aim poses, the aim ray to its hit (green on an object, yellow on the world, grey for a miss), and the hand joints. `cockpit/room` is the room frame with its floor at zero, in which the offline rig is drawn under `rig` (its pivots, spine, lean ring, step demand, and heading) and the rig the game ran under `rig_recorded` (cyan), so the two can be compared in place.
- `camera` and `camera_ticks`: the camera the frame rendered from, and the two tick poses the engine blended for it.
- `aim`, `grapple`, and `wrists`: each hand's aim target, the grapple's aim point, and the drawn wrists as the late hand correction placed them.
- `grapple_line` and `grapple_sling`: the wire while it is out, and the snatch's held line.
- `grip_targets`, `throws`, `held`, and `forces`: the point each grip chose, each throw's release point and velocity per real second, what each hand holds with the hold's report and lag, and the velocity change each of the mod's writes and impulses asked for (magenta for an impulse, blue for a velocity write).
- `bodies`: the dynamic bodies near Rico, as the broad-phase box each joined with (or a point when its introduction is outside the stretch), coloured by kind, the held ones pale yellow.
- `vehicle` and `vehicle_aim`: the vehicle at its rendered transform, and a manned mounted gun's aim point.

The `cockpit` tab shows the tracked poses unplaced, as the runtime gave them.

### Placing the cockpit

A version 8 recording holds where the cockpit sits in the world each game frame (the controller state's `view`), and the export places `world/cockpit` from it: a cockpit point is the recorded world head plus the view's rotation applied to the point's offset from the cockpit head, scaled by the world scale. The recorded head is the eye midpoint the camera renders from, which the hands compose from ([render-time-anchoring.md](../rendering/render-time-anchoring.md)).

An older recording is placed by an estimate (`rerun_export/registration.rs`): the camera is the anchor the root carries plus the cockpit position less the cockpit shift ([rig.md](../body/rig.md#who-moves-the-root)), turned by a heading the recording does not hold, which is fitted a quarter of a second at a time, never across a recenter, as the yaw that best turns the grips onto the drawn wrists about the root. Without a root tick the wrists' centroid anchors it, and with neither the last placement holds. `states/cockpit_placement` names what placed each frame and `about` counts them. Each wrist sits at an offset from its grip fixed in the controller's frame, estimated from the placements, and `plots/registration/wrist_error_m` is the distance between a placed grip's wrist and the drawn wrist: with the recorded view it measures the late hand correction, and with the estimate it also holds the fit's error.

### Plots, events, and states

`plots/` holds the scalars in tabs by subject: the frame time and the breakdown's slots, the controller publication's age, the hands' grip speed, trigger, squeeze, and haptics, the head's and Rico's heights, Rico's speeds and the root's unexplained move, the offline and recorded rigs, the room scale, the arms' IK, the camera's blend fraction and how far the rendered camera is from the blend and from the recorded head, the holds and the grapple, and the vehicle.

`events/` holds the events as text log entries with levels, by subject: grips and gun releases, snatches, throws, recenters, the body and wingsuit states, the session state, take markers, standing errors, unexplained root moves, the mod's refusals by source (`events/refusals/<source>`, as warnings), forces, injected actions, haptics, settings (with the settings' text as a `settings` document), the rig's configuration, bodies joining, and the timing. The chattier ones log at debug level so they can be filtered out.

`states/` holds lanes in the state timeline: the action set, the session state, the controller mode, the posture, the root's owner and support, the body's rule state and hand-over case, both rigs' step phases, the wingsuit gesture, the snatch phases, the cockpit's placement, the grapple's states, and the vehicle and seat. `states/move` is the coarse lane, the controller mode or the vehicle's class, with its labels and colours fixed by `MOVE_STATES` in `rerun_export/character.rs` so a mode reads the same colour in every take.

## What the file does not hold

A recording from before version 8 has none of the view, the camera, the grapple's line, the vehicle, the poses, the recorded rig, the bodies, or the notes, so its export places the cockpit by the estimate, draws the snatch's line along the hand's aim and a throw's release at the drawn wrist, and draws no skeleton, bodies, or recorded rig. Even from version 8 the format leaves out the world origin of a gun sight's ray (only the aim pose's ray is drawn), a body's shape as it turns (the box drawn is the axis-aligned extent it joined with), and the HumanIK targets of the solve's other passes.
