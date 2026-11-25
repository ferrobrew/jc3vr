# Scanning a take for flashes

`tracking-synth scan-view` scans a take's video for flashes and checks each one against the recording: where the camera was against Rico's animated head at the time. A flash is a frame, or two, that is unlike the frames on both sides of it while those frames are alike. In a headset take that is nearly always the camera dipping into Rico's body for a frame or two (the harness, the backpack, a leg), which a player sees as flicker; a cut to a new view or a fast turn is not one, because the frames after it stay changed. The code is `tools/tracking-synth/src/view_scan.rs`.

## Running it

```sh
cargo run --release -p tracking-synth --target x86_64-unknown-linux-gnu -- \
    scan-view sessions/<session>/recordings/<take>/tracking.jc3trk
```

The video is the take's `video.mkv` beside the recording unless `--video <path>` names another, and the report goes to `view-scan.md` beside it unless `--out <path>` names another. The video is placed on the recording's clock through the take's `frames.jsonl` and `stream.json`; without them the two clocks are taken as one, and the tool says so. `ffmpeg` must be on `PATH`. A forty-minute take scans in about two and a half minutes and needs the memory to decode its recording (about 2.5 GB for an hour).

The capture host runs it after the Rerun export when each take ends, for a take with its video (`scripts/rerun_take.sh`, [rerun.md](rerun.md)); `JC3VR_CAPTURE_SCAN=0` skips it. Its output joins the export's in `rerun.log`.

## How it scores

`ffmpeg` decodes the video to 96 by 54 thumbnails. For each frame the tool takes the mean absolute difference of the thumbnails' colours against its neighbours: a one-frame flash scores how much less alike the frame is to the frames either side than those two are to each other, and a two-frame flash scores the pair against the frames either side of it, the better of the two counting. A frame scoring over 25 (of 255) is a flash, and flashes within a tenth of a second of each other are one, the strongest.

For each flash the tool reads the camera's current tick pose (`ControllerState::view.camera.current`), placed from the same tick as the latest pre-IK sample (the rendered camera trails it by part of a tick, a metre or more at a flight's speed), and the pre-IK head bone in the world (the `PreIk` pose sample's head, through its character transform) within a tenth of a second either side, and reports:

- the camera's lead over the head along the head's motion, the least in the window: negative is behind the head, which at speed is inside or behind Rico's back. It is not measured while the head moves slower than `view_scan::MIN_SPEED_MPS` (1 m/s), where its motion gives no direction;
- the camera's greatest distance from the head;
- the head's speed, and the mode the detector read.

A flash with the camera more than 0.12 m behind the head (`view_scan::BEHIND_M`) counts as the camera behind the head.

## The report

`view-scan.md` gives the frame count, the flash count, how many had the camera behind the head, a table of both counts by mode, and every flash with its recording time (Rerun's `recording` timeline), its video time, its score, whether it lasted one frame or two, and the camera's lead, distance, speed, and mode. The recording time is the one to scrub to in Rerun or to cut frames at; the video time is the one for `ffmpeg -ss`.

The scan does not say what filled the view: look at the frames before concluding. A flash in a menu, a loading screen, or the debug panel opening is a flash too, and the camera check says nothing there.
