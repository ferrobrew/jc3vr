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

- `world/rico`: Rico's root from the root's tick, with his measured velocity (`velocity`) and the tick's wanted velocity (`wanted`) as arrows, and `world/rico_trail` his path through the stretch.
- `world/cockpit`: the cockpit frame placed in the world. Under it are the head (`head`, with axes and a gaze arrow), each hand's grip and aim poses with axes (`left/grip`, `left/aim`), the aim ray (`left/aim/ray`, its length the ray's hit when the aim pose casts it, green on an object, yellow on the world, and grey at two metres for a miss or a gun sight's ray), the hand joints when the runtime gave them, and the snatch's line (`left/aim/snatch`) while a snatch is out.
- `world/cockpit/room`: the room frame, the cockpit with its floor at zero, in which the grounded rig is drawn: the neck and waist pivots and the pelvis (`rig/pivots`), the spine from the pelvis through the waist to the neck (`rig/spine`), the lean ring at the neck's height around the pelvis (`rig/lean`, green while the neck is inside it and orange while a step is demanded), the step demand (`rig/demand`), and the body's heading (`rig/heading`). `root_anchor` is the root's place in the cockpit, the cockpit shift.
- `world/aim/left` and `world/aim/right`: each hand's aim target, `world/grapple` the grapple's aim point (green for a tetherable hit, yellow for a valid one, red for an invalid one), and `world/wrists` the drawn wrists, as the late hand correction placed them.
- `world/throws/<n>`: each throw's release point (the hand's drawn wrist at the release) and its velocity per real second.

Velocity arrows show a quarter-second's travel. The `cockpit` tab shows the same tracked poses unplaced, as the runtime gave them.

The recording does not hold the view's world transform, so the cockpit's placement is an estimate (`rerun_export::registration`). The camera is the anchor the root carries plus the cockpit position less the cockpit shift, turned by the view's heading and scaled by the world scale ([body/rig.md](../body/rig.md#who-moves-the-root)); the recording holds the root, the shift, the scale, and the floor, but not the heading. The heading is fitted, over a quarter of a second at a time and never across a recenter, as the yaw that best turns the grips about the root's place in the cockpit onto the drawn wrists about the root, with each hand's wrist offset from its grip estimated from the fit. Without a root tick (in a vehicle) the wrists' centroid anchors it, and with neither the last placement holds. On the 2 November take it is good to about five degrees, checked against the aim targets, and the drawn wrists sit a median of 13 cm from the registered grips (`plots/registration/wrist_error_m`), so `world/wrists` shows the error where it matters.

### Plots

Under `plots/`, in tabs by subject: the frame time and the controller publication's age; the registration's wrist error; each hand's grip speed, trigger, and squeeze; the head's height over the floor (STAGE) and Rico's height over the ground and over the nearer of the ground and the water; Rico's measured speed, horizontal and vertical, the tick's wanted speed, and the root's unexplained move; the rig's bend, step demand, yaw error, neck offset, lean reach, crouch, neck height, standing neck height, and simulated pelvis speed; the room-scale gap, residual, push-back, eye drop, and fade; and each arm's IK weight and reach and the late correction's displacement.

### Events and states

Under `events/`, as text log entries with levels:

- `events/grip`: each grip acquisition's stage (the press and what the resolver chose, the request's result, and the first drive), each grip release with its cause (a lost hold as a warning), and each gun release with its route and its speed or refusal.
- `events/snatch`: each snatch phase change with its target and yank, and its outcome and reason (a failure as a warning).
- `events/throw`: each throw, with its speed per game and real second and its source.
- `events/recenter`: each recenter with its kind, cause, and turn (a follow bank at debug level).
- `events/body`, `events/wingsuit`: the body's animation state changes and the wingsuit gesture's phases, at debug level.
- `events/session`: the session state's changes and input focus.
- `events/marker`: take markers. `events/errors`: the input subsystem's and the raycast's standing errors as they start and clear. `events/root`: a root move the tick's wanted velocity does not explain by more than a quarter of a metre.

Under `states/`, as lanes in the state timeline: the action set, the session state, the controller mode with its refinements, the posture, the root's owner and support, the body's rule state and hand-over case, the rig's step phase, the wingsuit gesture, and each hand's snatch phase.

## What the file does not hold

The export draws what the format records; a later format version would make these exact rather than estimated or missing:

- The view's world transform (the camera, or the view frame's heading), which the cockpit's placement is fitted for.
- The snatch's line and hook: the snatch phases carry the target's kind and distance on arming and firing, but not the hook's world point, the line's anchor, or the target's identity, so the line is drawn along the hand's aim.
- A throw's release position: a throw records the requested motion but not where the item left, so the release point is the hand's drawn wrist.
- The body's pose: the root's position but not Rico's world heading, his pelvis or neck bones, or the skeleton the sockets ride; and the rig's own outputs (pelvis, yaw, step phase), which the export evaluates offline with today's solver rather than reading what the game ran.
- The world origin of each gun sight's ray, so only the aim pose's ray is drawn from the hand.
- The grip acquisition's chosen target's world position, and the held object's world pose while held.
