#!/usr/bin/env bash
# The host side of the session recorder: listen for the payload's takes and record the game's mirror
# window with gpu-screen-recorder, with the default output's audio, into each take's folder.
#
# Usage: scripts/capture_host.sh [fallback output dir] [-- extra gpu-screen-recorder args]
#
# Start this, then start a recording in game (F1, or Tools > Video); the payload retries the
# connection every half second until this listener is up, so either order works. Each take finds the
# game's window under niri, records its region on screen to `video.mkv` in the take's folder
# (`<session>/recordings/<stamp>/`, beside `frames.jsonl`), and writes `stream.json` there with the
# region and the first frame's wall-clock time. Stopping the recording in game finalizes the file
# and the listener waits for the next take. Ctrl+C stops the listener, finalizing the take in
# progress. A take whose folder the payload cannot name (the payload on native Windows) goes to a
# stamped folder under the fallback output dir, `captures/` by default.
#
# The window must stay where it is and uncovered for the whole take: the region is fixed when the
# take starts, and whatever is on screen there is what is recorded.
#
# Environment overrides:
#   JC3VR_CAPTURE_PORT    the listen port (default 47172, the payload's default).
#   JC3VR_CAPTURE_FPS     the recording frame rate (default 60).
#   JC3VR_CAPTURE_CODEC   the video codec: hevc (default), av1, or h264. AV1 needs an encoder
#                         that has it (gpu-screen-recorder --info lists the GPU's codecs).
#   JC3VR_CAPTURE_BITRATE the constant video bitrate in kbps (default 10000, about 1.1 GB per
#                         fifteen minutes at 1080p60); 0 records at constant quality instead.
#   JC3VR_CAPTURE_QUALITY gpu-screen-recorder's quality preset for constant quality (default high).
#   JC3VR_CAPTURE_AUDIO   a gpu-screen-recorder audio input (default default_output); set to "none"
#                         to record video only.
#   JC3VR_CAPTURE_WINDOW  a niri window id to record instead of looking the game's window up.
#   JC3VR_CAPTURE_REGION  a WxH+X+Y region to record without asking niri, for other compositors.

set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"
out_dir="captures"
if [ $# -gt 0 ] && [ "$1" != "--" ]; then
    out_dir="$1"
    shift
fi

args=(
    --port "${JC3VR_CAPTURE_PORT:-47172}"
    --fallback-dir "$out_dir"
    --fps "${JC3VR_CAPTURE_FPS:-60}"
    --codec "${JC3VR_CAPTURE_CODEC:-hevc}"
    --bitrate-kbps "${JC3VR_CAPTURE_BITRATE:-10000}"
    --quality "${JC3VR_CAPTURE_QUALITY:-high}"
    --audio "${JC3VR_CAPTURE_AUDIO:-default_output}"
)
if [ -n "${JC3VR_CAPTURE_WINDOW:-}" ]; then
    args+=(--window-id "$JC3VR_CAPTURE_WINDOW")
elif [ -n "${JC3VR_CAPTURE_REGION:-}" ]; then
    args+=(--region "$JC3VR_CAPTURE_REGION")
fi

exec python3 -I "$DIR/tools/capture/relay.py" "${args[@]}" "$@"
