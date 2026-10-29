#!/usr/bin/env bash
# The host side of the in-process recorder: listen for the payload's yuv4mpegpipe stream, encode
# it with a hardware encoder where one is available, cut it into ten-minute segments, and record
# the game's audio alongside.
#
# Usage: scripts/capture_host.sh [output dir]
#
# Start this, then start the recording from the Director tab (or its hotkey); the payload retries
# the connection every half second until this listener is up, so either order works, but frames
# rendered before the connection are dropped. Stop with Ctrl+C or by stopping the recording in
# game: the stream closes and ffmpeg finalizes the last segment.
#
# The stream carries its size and rate in its own header, so nothing here needs to match the
# in-game settings. The per-frame pose sidecar lives in the game's session folder
# (`sessions/<session>/videos/<stamp>/frames.jsonl`), not here; the segment timestamps in the file
# names and the wall clock in the sidecar are what line the two up.
#
# Environment overrides:
#   JC3VR_CAPTURE_PORT    the listen port (default 47172, the payload's default).
#   JC3VR_CAPTURE_ENCODER the ffmpeg video encoder (default: auto, see pick_encoder).
#   JC3VR_CAPTURE_AUDIO   the PulseAudio/PipeWire source to record (default: the default sink's
#                         monitor); set to "none" to record video only.
#   JC3VR_CAPTURE_QUALITY the encoder quality knob: a CQ/CRF-style value (default 18).
#   JC3VR_SEGMENT_SECONDS the segment length (default 600).

set -euo pipefail

out_dir="${1:-captures}"
port="${JC3VR_CAPTURE_PORT:-47172}"
quality="${JC3VR_CAPTURE_QUALITY:-18}"
segment_seconds="${JC3VR_SEGMENT_SECONDS:-600}"
mkdir -p "$out_dir"
stamp="$(date +%Y-%m-%d_%H-%M-%S)"

# Pick the first hardware H.264 encoder ffmpeg reports, falling back to libx264. H.264 rather than
# HEVC because every editor imports it without transcoding; at the quality set here the size
# difference does not matter for source footage.
pick_encoder() {
    local encoders
    encoders="$(ffmpeg -hide_banner -encoders 2>/dev/null)"
    for candidate in h264_nvenc h264_vaapi h264_qsv h264_amf; do
        if grep -q " $candidate " <<<"$encoders"; then
            echo "$candidate"
            return
        fi
    done
    echo libx264
}
encoder="${JC3VR_CAPTURE_ENCODER:-$(pick_encoder)}"

# Encoder-specific arguments. The colour tags mark the stream as BT.709 limited range, which is
# what the payload's pack shader produces.
video_args=(-color_primaries bt709 -color_trc bt709 -colorspace bt709 -color_range tv)
device_args=()
case "$encoder" in
    h264_nvenc)
        video_args+=(-c:v h264_nvenc -preset p5 -tune hq -rc vbr -cq "$quality" -b:v 0)
        ;;
    h264_vaapi)
        # VAAPI wants the frames uploaded to the device; the filter does that from the CPU frames.
        video_args+=(-vf 'format=nv12,hwupload' -c:v h264_vaapi -qp "$quality")
        device_args=(-vaapi_device "${JC3VR_VAAPI_DEVICE:-/dev/dri/renderD128}")
        ;;
    h264_qsv)
        video_args+=(-c:v h264_qsv -global_quality "$quality")
        ;;
    h264_amf)
        video_args+=(-c:v h264_amf -quality quality -rc cqp -qp_i "$quality" -qp_p "$quality")
        ;;
    *)
        video_args+=(-c:v libx264 -preset veryfast -crf "$quality")
        ;;
esac

# The audio source: the default sink's monitor unless overridden. `pactl` speaks to PipeWire's
# PulseAudio shim as well as to PulseAudio itself.
audio_source="${JC3VR_CAPTURE_AUDIO:-}"
if [ -z "$audio_source" ]; then
    default_sink="$(pactl get-default-sink 2>/dev/null || true)"
    if [ -n "$default_sink" ]; then
        audio_source="${default_sink}.monitor"
    else
        audio_source=none
    fi
fi
audio_in=()
audio_map=()
audio_args=()
if [ "$audio_source" != none ]; then
    audio_in=(-use_wallclock_as_timestamps 1 -f pulse -thread_queue_size 4096 -i "$audio_source")
    audio_map=(-map 1:a)
    audio_args=(-c:a aac -b:a 256k)
fi

echo "capture: listening on tcp port $port, encoder $encoder, audio ${audio_source}, segments of ${segment_seconds}s in $out_dir" >&2

# The video input is the listening socket. `-use_wallclock_as_timestamps` on both inputs lets the
# muxer line the audio up against the moment each frame arrived, and the y4m header's rate keeps
# the video itself constant-rate. `-fps_mode passthrough` keeps one output frame per input frame:
# the payload already emits exactly one frame per display refresh (repeating on a late readback),
# so nothing should be dropped or duplicated here.
exec ffmpeg -hide_banner -loglevel warning -stats \
    "${device_args[@]}" \
    -use_wallclock_as_timestamps 1 -thread_queue_size 64 \
    -f yuv4mpegpipe -i "tcp://127.0.0.1:${port}?listen=1" \
    "${audio_in[@]}" \
    -map 0:v "${audio_map[@]}" \
    -fps_mode passthrough \
    "${video_args[@]}" \
    "${audio_args[@]}" \
    -f segment -segment_time "$segment_seconds" -reset_timestamps 1 -segment_format matroska \
    -strftime 0 \
    "${out_dir}/jc3vr_${stamp}_%03d.mkv"
