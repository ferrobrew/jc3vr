# Session output

Every file the mod writes during a run lands in one folder, `<sessions root>/<session stamp>/`. The session stamp is the injection time. Every stamp, the session's and each capture's, is the local wall clock as `YYYY-MM-DD_HH-MM-SS_mmm`; the milliseconds keep an uninject and reinject within one second apart.

## The sessions root

The payload reads `JC3VR_SESSIONS_DIR` from the game process's environment at injection (`payload/src/session/root.rs`):

- A value starting with `/` is a host (Unix) path. Under wine or Proton the payload converts it to the Windows path it opens with wine's `wine_get_dos_file_name`, resolved from `kernel32` at run time; on native Windows, where that export does not exist, the value is refused.
- Any other value is a Windows path, taken as it is.
- Unset, blank, or refused, the root is `sessions/` in the local data directory that `directories::ProjectDirs` names for jc3vr: `%LOCALAPPDATA%\philpax\jc3vr\data\sessions` on Windows, and the same inside the game's prefix under Proton. The settings live beside it ([settings.md](settings.md)).

`scripts/vr_launch.sh` sets the variable, unless it is already set, to `sessions/` in the workspace (the repository root, found from the script's own location; ignored by git), creates the folder, and names it in `JC3BOOT_FORWARD_ENV` so jc3boot passes it into the container. The pressure-vessel container shares `$HOME`, so the path is the same on both sides. The variable has to be in the game's environment: setting it for `scripts/proton_run.sh` or `scripts/run.ps1`, which inject into a running game, has no effect. On native Windows the root is the local data directory unless the game was started with the variable set.

When the root is not a Unix path already, the payload asks wine for its Unix form with `wine_get_unix_file_name`; on native Windows there is none. The startup log names the session folder in both forms and where the root came from, with a warning when a set variable was refused, and the debug UI's Status page shows the same below the rows. The Unix form is what the capture host is told to write a recording's video into ([capture.md](capture.md)).

## Layout

What lands where is fixed per kind of output, in `payload/src/session.rs`. Folder names are plural, and file names do not repeat the kind their folder names. A folder is created only when its first file is written, so a session holds only the folders its run used.

| Path in the session | What writes it | Read by |
| --- | --- | --- |
| `session.log` | the tracing log | |
| `crash.log` | the crash handler, appended through a raw handle so a record survives a broken logging path | |
| `markers.jsonl` | take markers made while no video recording runs, one JSON object per line ([capture.md](capture.md)) | |
| `profiles/<stamp>.json` | an F9 profiler capture ([profiler.md](performance/profiler.md)) | `tools/analyze_profile.py`, ui.perfetto.dev |
| `screenshots/<stamp>.png` and `<stamp>.json` | an F12 screenshot and its single-pass sidecar | |
| `grapple-logs/<stamp>.csv` | a grapple telemetry capture ([grapple-comfort.md](body/grapple-comfort.md)) | |
| `recordings/<stamp>/` | a recording, holding whichever halves it has (below) | |
| `scenarios/<stamp>_<scenario file>.toml` | a scenario's summary, stamped with the scenario's start | |
| `shader-dumps/<stamp>.txt` | the vertex-shader name census, after a shader reload with the census on | |
| `traces/<stamp>/` | a render trace: `trace.ndjson` and, with the per-eye screenshot probe, `frameNNN_eye<N>.png` ([render-trace.md](rendering/render-trace.md)) | `tools/analyze_trace_frames.py` |

A recording's folder holds:

| File | What writes it | Read by |
| --- | --- | --- |
| `tracking.jc3trk` | a tracking recording ([testing/recorder.md](testing/recorder.md)) | `tracking-synth dump`, the replay |
| `lookback.jc3trk` | a save of the tracking lookback, in a folder of its own | `tracking-synth dump`, the replay |
| `stream.json` and `frames.jsonl` | a video recording's stream parameters and per-frame sidecar ([capture.md](capture.md)) | `tools/capture/reframe.py` |
| `video_NNN.mkv` | the capture host's segments of the video, written there by the host (not the game) when the session folder's Unix form is known | `tools/capture/reframe.py`, an editor |

A standalone tracking recording is a folder with only `tracking.jc3trk`, a standalone video recording one without it, and an F1 session recording has both halves.

A kind that writes one file per capture names it `<stamp>[_<qualifier>].<ext>`; a kind that writes several gets a `<stamp>/` folder whose files are named by their role. Images are written by one writer, `payload/src/image_writer.rs`, on its own thread, and follow one rule wherever they land: `<stem>.png` for an image of the whole frame (both eyes side by side in stereo) and `<stem>_eye<N>.png` for one eye, with any sidecar beside it as `<stem>.json`.
