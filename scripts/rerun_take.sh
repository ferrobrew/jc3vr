#!/usr/bin/env bash
# Export a take's tracking recording, with its video, as a Rerun recording (`take.rrd`) beside it.
#
# Usage: scripts/rerun_take.sh <take folder>
#
# The take folder is `<session>/recordings/<stamp>/`. The payload stops the video and the tracking
# writer together, so the tracking file can still be growing when the video ends; the script waits
# until the file's size has held for a few seconds (the format has no end marker, and a reader stops
# at the last complete record). The video is added when the take's `video.mkv`, `frames.jsonl`, and
# `stream.json` are all present. The export's output goes to `rerun.log` in the take folder.
# docs/mod/testing/rerun.md describes the export.
#
# The capture relay (scripts/capture_host.sh) runs this when each take ends, unless
# JC3VR_CAPTURE_RERUN is 0.
#
# Environment overrides:
#   JC3VR_RERUN_SETTLE_S   how long the tracking file's size must hold (default 3).
#   JC3VR_RERUN_WAIT_S     how long to wait for the file to appear and settle (default 120).

set -euo pipefail

if [ $# -ne 1 ]; then
    echo "usage: $0 <take folder>" >&2
    exit 2
fi

DIR="$(cd "$(dirname "$0")/.." && pwd)"
take="$(cd "$1" && pwd)"
tracking="$take/tracking.jc3trk"
settle_s="${JC3VR_RERUN_SETTLE_S:-3}"
wait_s="${JC3VR_RERUN_WAIT_S:-120}"
log="$take/rerun.log"

exec >>"$log" 2>&1
echo "rerun: $(date -Is): exporting $take"

deadline=$((SECONDS + wait_s))
last_size=-1
held_since=$SECONDS
while :; do
    if [ -f "$tracking" ]; then
        size=$(stat -c %s "$tracking")
        if [ "$size" != "$last_size" ]; then
            last_size=$size
            held_since=$SECONDS
        elif [ $((SECONDS - held_since)) -ge "$settle_s" ]; then
            break
        fi
    fi
    if [ $SECONDS -ge $deadline ]; then
        if [ -f "$tracking" ]; then
            echo "rerun: the tracking file was still growing after ${wait_s}s; exporting what it holds"
            break
        fi
        echo "rerun: no tracking recording at $tracking after ${wait_s}s; nothing to export"
        exit 1
    fi
    sleep 1
done

video=()
if [ -f "$take/video.mkv" ] && [ -f "$take/frames.jsonl" ] && [ -f "$take/stream.json" ]; then
    video=(--video "$take/video.mkv")
else
    echo "rerun: exporting without video (the take lacks video.mkv, frames.jsonl, or stream.json)"
fi

cd "$DIR"
if cargo run --quiet --release -p tracking-synth --target x86_64-unknown-linux-gnu -- \
    export-rerun "${video[@]}" "$tracking" -o "$take/take.rrd.tmp"; then
    mv "$take/take.rrd.tmp" "$take/take.rrd"
    echo "rerun: $(date -Is): wrote $take/take.rrd"
else
    status=$?
    rm -f "$take/take.rrd.tmp"
    echo "rerun: $(date -Is): the export failed with status $status"
    exit "$status"
fi
