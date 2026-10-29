# Session output

Every file the mod writes during a run lands in one folder, `sessions/<session stamp>/`, beside the payload DLL (for a release build, `target/x86_64-pc-windows-msvc/release/sessions/`). The session stamp is the injection time. Every stamp, the session's and each capture's, is the local wall clock as `YYYY-MM-DD_HH-MM-SS_mmm`; the milliseconds keep an uninject and reinject within one second apart.

What lands where is fixed per kind of output, in `payload/src/session.rs`. Folder names are plural, and file names do not repeat the kind their folder names. A folder is created only when its first file is written, so a session holds only the folders its run used.

| Path in the session | What writes it | Read by |
| --- | --- | --- |
| `session.log` | the tracing log | |
| `crash.log` | the crash handler, appended through a raw handle so a record survives a broken logging path | |
| `markers.jsonl` | take markers made while no video recording runs, one JSON object per line ([capture.md](capture.md)) | |
| `profiles/<stamp>.json` | an F9 profiler capture ([profiler.md](performance/profiler.md)) | `tools/analyze_profile.py`, ui.perfetto.dev |
| `screenshots/<stamp>.png` and `<stamp>.json` | an F12 screenshot and its single-pass sidecar | |
| `grapple-logs/<stamp>.csv` | a grapple telemetry capture ([grapple-comfort.md](body/grapple-comfort.md)) | |
| `recordings/<stamp>.jc3trk` | a tracking recording ([testing/recorder.md](testing/recorder.md)) | `tracking-synth dump`, the replay |
| `recordings/<stamp>_lookback.jc3trk` | a save of the tracking lookback | `tracking-synth dump`, the replay |
| `scenarios/<stamp>_<scenario file>.toml` | a scenario's summary, stamped with the scenario's start | |
| `shader-dumps/<stamp>.txt` | the vertex-shader name census, after a shader reload with the census on | |
| `traces/<stamp>/` | a render trace: `trace.ndjson` and, with the per-eye screenshot probe, `frameNNN_eye<N>.png` ([render-trace.md](rendering/render-trace.md)) | `tools/analyze_trace_frames.py` |
| `videos/<stamp>/` | a video recording's sidecar: `stream.json` and `frames.jsonl` ([capture.md](capture.md)) | `tools/capture/reframe.py` |

A kind that writes one file per capture names it `<stamp>[_<qualifier>].<ext>`; a kind that writes several gets a `<stamp>/` folder whose files are named by their role. Images are written by one writer, `payload/src/image_writer.rs`, on its own thread, and follow one rule wherever they land: `<stem>.png` for an image of the whole frame (both eyes side by side in stereo) and `<stem>_eye<N>.png` for one eye, with any sidecar beside it as `<stem>.json`.
