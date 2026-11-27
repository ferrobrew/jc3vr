# Session output

Every file the mod writes during a run lands in `<sessions root>/<session stamp>/`, stamped with the injection time. Every stamp is the local wall clock as `YYYY-MM-DD_HH-MM-SS_mmm`; the milliseconds keep an uninject and reinject within one second apart.

## The sessions root

The payload reads `JC3VR_SESSIONS_DIR` from the game's environment at injection (`crates/jc3vr-core/src/session/root.rs`). A value starting with `/` is a host path, converted under Wine with `wine_get_dos_file_name` and refused on native Windows; any other value is a Windows path. Unset or refused, the root is `sessions/` in jc3vr's local data directory, beside the settings ([settings.md](settings.md)). `scripts/vr_launch.sh` sets it to the workspace's `sessions/` and forwards it into the container; setting it for a script that injects into a running game has no effect. The startup log and the debug UI's Status page name the folder in both Windows and Unix forms, and both go to the capture host, since the relay and the injector write from different sides ([capture.md](capture.md)).

## Layout

The layout is fixed in `crates/jc3vr-core/src/session/mod.rs`, and a folder is created only when its first file is written. The session holds `session.log`, `crash.log`, `markers.jsonl`, and folders for profiler captures, screenshots, grapple logs, render traces, and `recordings/<stamp>/`. A recording's folder holds whichever of `tracking.jc3trk` ([testing/recorder.md](testing/recorder.md)), the video take's files ([capture.md](capture.md#files-and-alignment)), and the Rerun export and view scan ([testing/rerun.md](testing/rerun.md)) it has; a lookback save gets a folder of its own. A kind that writes one file per capture names it `<stamp>[_<qualifier>].<ext>`, and one that writes several gets a `<stamp>/` folder. Images go through one writer (`crates/jc3vr-render/src/image_writer.rs`): `<stem>.png` for the whole frame and `<stem>_eye<N>.png` for one eye.
