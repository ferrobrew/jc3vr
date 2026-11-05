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

`--from` and `--to` take the same times as `dump`: seconds on the recording's clock, or `xr:<ns>` for a predicted display time. Without them the whole recording is exported; an hour-long take writes about half a gigabyte without its video, so a stretch of a few minutes is the usual way. `--video` adds the take's video, which needs the take's `frames.jsonl` and `stream.json` beside the recording to place it on the recording's clock. `--video-height <px>` sets the cut's height (720 by default, never above the source's). `--height`, `--eye-height`, and `--tuning` are `rig-eval`'s, for the offline rig ([body/rig.md](../body/rig.md#offline-evaluation)).

A two-minute stretch of an hour's take, with its video, takes about 25 seconds and writes about 90 MB, most of it video; decoding the recording takes about 2.5 GB of memory whatever the stretch.

Rerun reads video only from MP4, and a take is recorded to MKV, so the tool cuts the stretch with `ffmpeg` (which must be on `PATH`) and encodes it afresh as H.264 at constant quality, without B-frames and with a keyframe every second, so the cut starts at the exact frame and scrubs back cheaply. The cut is copied into the `.rrd` and deleted.

### The viewer

The SDK is pinned to Rerun 0.40.0, the version nixpkgs ships, and the viewer must be that version or newer:

```sh
nix shell nixpkgs#rerun          # or: cargo install rerun-cli --version =0.40.0 --locked
rerun take.rrd
rerun rrd verify take.rrd        # checks that the file loads, without a window
```

The native viewer decodes H.264 through `ffmpeg` on `PATH`. The export is behind the `rerun` cargo feature of `tracking-synth`, on by default; `--no-default-features` builds the tool without it and its dependencies.

## What it shows

Everything is on the `recording` timeline, in seconds on the recording's clock. A `wall_clock` timeline is added when the take's sidecar sits beside the recording (each frame's wall-clock stamp), or otherwise when the recording has a take marker (which anchors it, running at real time). The default layout has the 3D scene and the plots on the left, and the video, the event log, the state lanes, and the export's notes (`about`) on the right.

### The 3D scene

`world` is the game's world, Y up, less an origin (the root's first position in the stretch, rounded to the metre, given in `about`), so the viewer works near zero.

- `world/rico`: Rico's root from the root's tick, with his measured velocity (`velocity`) and the tick's wanted velocity (`wanted`) as arrows, his heading (`heading`, the final pose's character transform's -Z, flattened), and `world/rico_trail` his path through the stretch.
- `world/pose/pre_ik` and `world/pose/final`: Rico's body bones from the pose samples, before the HumanIK solves (grey) and as they rendered (orange), each a skeleton of lines from parent to child (`bones`) with its joints (`joints`, labelled by bone name), under an entity whose transform is the sample's character transform, so that a bone sits in the character's model space. The pre-IK stage also holds the HumanIK position targets live for the solve (`pre_ik/ik_targets`, labelled by effector, reach, and pull); the rotation targets are not drawn. The final stage carries the character frame's axes.
- `world/cockpit`: the cockpit frame placed in the world. Under it are the head (`head`, with axes and a gaze arrow), each hand's grip and aim poses with axes (`left/grip`, `left/aim`), the aim ray (`left/aim/ray`, its length the ray's hit when the aim pose casts it, green on an object, yellow on the world, and grey at two metres for a miss or a gun sight's ray), and the hand joints when the runtime gave them. In a recording without the grapple's line (before version 8), the snatch's line runs along the hand's aim to the target's distance (`left/aim/snatch`) while a snatch is out.
- `world/cockpit/room`: the room frame, the cockpit with its floor at zero, in which the offline rig is drawn under `rig`: the neck and waist pivots and the pelvis (`rig/pivots`), the spine from the pelvis through the waist to the neck (`rig/spine`), the lean ring at the neck's height around the pelvis (`rig/lean`, green while the neck is inside it and orange while a step is demanded), the step demand (`rig/demand`), and the body's heading (`rig/heading`). `rig_recorded` draws the same for the rig the game ran (cyan), from the recording's rig samples, shifted by the difference between the sample's floor and the frame's, so the two can be compared in place. `root_anchor` is the root's place in the cockpit, the cockpit shift.
- `world/camera`: the camera the frame rendered from, with axes, and `world/camera_ticks` the two tick poses the engine blended for it, with the step between them.
- `world/aim/left` and `world/aim/right`: each hand's aim target, `world/grapple` the grapple's aim point (green for a tetherable hit, yellow for a valid one, red for an invalid one), and `world/wrists` the drawn wrists, as the late hand correction placed them.
- `world/grapple_line`: the grapple's wire from the device's end to the hook's end while a wire is out, green while the hook's end is fixed and yellow while it is not, labelled with the hook's and the wire's states. `world/grapple_sling` is the snatch's held line, from its anchor to the target's centre.
- `world/grip_targets/left` and `world/grip_targets/right`: the world point a grip acquisition chose, from the acquisition until the hand lets go.
- `world/throws/<n>`: each throw's release point and its velocity per real second. The release point is where the throw says the item left, or, without that, the hand's drawn wrist at the release.
- `world/bodies`: the dynamic bodies near Rico after each physics step, as boxes of the broad-phase extents a body joined the set with (`boxes`) or as points where its introduction is not in the stretch (`points`), coloured by kind and labelled with the body's id and kind, the held ones in pale yellow, with the moving ones' velocities (`velocity`). A body at rest stays drawn though the samples leave it out.
- `world/held/left` and `world/held/right`: what each hand holds, the body's position and velocity (`body`, `velocity`), and the hold's report, the grab point, where the hand would have it, and the body's centre (`report`), with a line from the grab point to the target (`lag`). It clears when the hand lets go.
- `world/forces`: the velocity writes and impulses the mod applied, drawn as the velocity change each asked for: an impulse (magenta) at its point, divided by the body's mass when the force records it, and a velocity write (blue) at the body's latest known position. An arrow stays a quarter of a second.
- `world/vehicle`: the vehicle the player rides, at its rendered transform with axes, and `world/vehicle_aim` a manned mounted gun's aim point.

Velocity arrows show a quarter-second's travel. The `cockpit` tab shows the same tracked poses unplaced, as the runtime gave them.

A recording from version 8 holds where the cockpit sits in the world for each game frame, and the export places `world/cockpit` from it: a cockpit point is the published world head plus the view's rotation applied to the point's offset from the cockpit head, scaled by the world scale. A frame whose controller state lacks the view, which is every frame before version 8, is placed by an estimate (`rerun_export::registration`). The camera is the anchor the root carries plus the cockpit position less the cockpit shift, turned by the view's heading and scaled by the world scale ([body/rig.md](../body/rig.md#who-moves-the-root)); the older recordings hold the root, the shift, the scale, and the floor, but not the heading. The heading is fitted, over a quarter of a second at a time and never across a recenter, as the yaw that best turns the grips about the root's place in the cockpit onto the drawn wrists about the root. Without a root tick (in a vehicle) the wrists' centroid anchors it, and with neither the last placement holds. On the 2 November take it is good to about five degrees, checked against the aim targets. `states/cockpit_placement` names what placed each frame (`view`, `root`, `wrists`, or `held`), and `about` counts them.

Each hand's wrist sits a little off its grip, at an offset fixed in the controller's frame, which is estimated from the placements (the recorded and the root-anchored ones). `plots/registration/wrist_error_m` is the distance between a placed grip's wrist and the drawn wrist: with the recorded view it measures the late hand correction, and with the estimate it also holds the fit's error (a median of 13 cm on the 2 November take), so `world/wrists` shows the error where it matters.

### Plots

Under `plots/`, in tabs by subject: the frame time, the controller publication's age, and, from the recorded frame state, the last game frame's interval and its sim ticks; the registration's wrist error; each hand's grip speed, trigger, squeeze, and last haptic amplitude; the head's height over the floor (STAGE) and Rico's height over the ground and over the nearer of the ground and the water; Rico's measured speed, horizontal and vertical, the tick's wanted speed, and the root's unexplained move; the offline rig's bend, step demand, yaw error, neck offset, lean reach, crouch, neck height, standing neck height, and simulated pelvis speed, and the recorded rig's as `plots/rig/recorded_*` (with its gap, and without a pelvis speed); the room-scale gap, residual, push-back, eye drop, and fade; each arm's IK weight and reach and the late correction's displacement; the camera's blend fraction, the distance between its tick poses, how far the rendered camera is from the blend of its tick poses by that fraction, and how far it is from the published head (`plots/camera/`); the frame-time breakdown's mean and longest frame and each slot's own time per frame (`plots/timing/`); each hold's lag, load, and twist, and the grapple's wire length, its longest, the snatch sling's length, tension, and most tension, and the grapple lock's length, stretch, tension, and most tension (`plots/held/`, `plots/grapple/`); and the vehicle's last step and its steering (`plots/vehicle/`).

### Events and states

Under `events/`, as text log entries with levels:

- `events/grip`: each grip acquisition's stage (the press and what the resolver chose, the request's result, and the first drive), each grip release with its cause (a lost hold as a warning), and each gun release with its route and its speed or refusal.
- `events/snatch`: each snatch phase change with its target and yank, and its outcome and reason (a failure as a warning).
- `events/throw`: each throw, with its speed per game and real second and its source.
- `events/recenter`: each recenter with its kind, cause, and turn (a follow bank at debug level).
- `events/body`, `events/wingsuit`: the body's animation state changes and the wingsuit gesture's phases, at debug level.
- `events/session`: the session state's changes and input focus.
- `events/marker`: take markers. `events/errors`: the input subsystem's and the raycast's standing errors as they start and clear. `events/root`: a root move the tick's wanted velocity does not explain by more than a quarter of a metre.
- `events/refusals/<source>`: the mod's refusals and other diagnostic moments, as warnings, by the subsystem that reported them.
- `events/forces`: each force the mod applied, with its source line (an impulse at info level, a velocity write at debug level).
- `events/injected`: the actions the mod injected on each input tick, at debug level so they can be filtered out.
- `events/haptics`: each haptic cue. `events/settings`: each settings note; the settings' text is the `settings` document, shown in its own tab. `events/rig`: the rig's calibration and tuning (a repeat at debug level). `events/bodies`: a body joining the near set, at debug level. `events/timing`: each frame-time breakdown, at debug level.

Under `states/`, as lanes in the state timeline: the action set, the session state, the controller mode with its refinements, the posture, the root's owner and support, the body's rule state and hand-over case, the offline rig's step phase and the recorded rig's (`rig_step_recorded`), the wingsuit gesture, each hand's snatch phase, what placed the cockpit, and, in a recording that has them, the grapple's hook and wire states and the vehicle with the player's seat and what each hand holds.

## What the file does not hold

A recording from before version 8 has none of the view, the camera, the grapple's line, the vehicle, the frame state, the poses, the recorded rig, the bodies, or the notes, so its export draws what it did before: the cockpit placed by the estimate, the snatch's line along the hand's aim, a throw's release at the drawn wrist, and no skeleton, bodies, or recorded rig. Even from version 8, the format leaves out:

- The world origin of each gun sight's ray, so only the aim pose's ray is drawn from the hand.
- A body's shape as it is: a body sample has its position and orientation, and its introduction the broad-phase box it joined with, axis-aligned in the world at that moment, so the box drawn is that extent and does not turn with the body.
- The HumanIK targets of the solve's other passes; only the main pass's live targets are recorded, with the pre-IK pose.
