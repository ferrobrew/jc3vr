#!/bin/sh
# Fetch an LGPL ffmpeg build for Windows and stage it beside the built injector.
#
# The injector stays running after it injects and encodes the payload's in-process recordings with
# an `ffmpeg.exe` beside it (see docs/mod/capture.md). This script provides that executable from a
# pinned, checksum-verified BtbN FFmpeg-Builds release: the LGPL variant, which carries the
# hardware H.264 encoders (NVENC, AMF, Quick Sync, Media Foundation) and OpenH264 but not libx264,
# which is GPL. The build's LICENSE.txt is staged with it as ffmpeg-LICENSE.txt. The download is
# cached under `.ffmpeg/`; only the first run needs the network. A Windows tester without a POSIX
# shell uses scripts/fetch_ffmpeg.ps1, which fetches the same build.
#
#   scripts/fetch_ffmpeg.sh    # stage into every built target/.../{debug,release} dir
#
# Set JC3VR_FFMPEG_ZIP to a local copy of the zip to skip the download (it is still verified).
set -eu

# Pinned BtbN release. BtbN keeps each month's last autobuild, so pin one of those; bump all three
# together from https://github.com/BtbN/FFmpeg-Builds/releases (the `win64-lgpl-<branch>.zip`
# asset, not `-shared` and not `gpl`).
RELEASE=autobuild-2026-10-31-12-59
BUILD=ffmpeg-n8.1.3-27-g8dca1586f4-win64-lgpl-8.1
SHA256=d4c2a42673917b4a97104a3e9f082729a9730796b98664e37c0b7eba6c6c613d
URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/${RELEASE}/${BUILD}.zip"

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)
cache="$repo/.ffmpeg"
zip="$cache/${BUILD}.zip"
exe="$cache/ffmpeg.exe"
license="$cache/ffmpeg-LICENSE.txt"

mkdir -p "$cache"

if [ -n "${JC3VR_FFMPEG_ZIP:-}" ]; then
    cp "$JC3VR_FFMPEG_ZIP" "$zip.part"
    mv "$zip.part" "$zip"
fi

# Download once, verified against the pinned hash. Re-fetch if the cached zip is missing or its hash
# no longer matches (a partial or corrupt download).
if [ ! -f "$zip" ] || ! printf '%s  %s\n' "$SHA256" "$zip" | sha256sum -c - >/dev/null 2>&1; then
    echo "fetch_ffmpeg: downloading $BUILD" >&2
    curl -fSL --retry 3 -o "$zip.part" "$URL" || {
        echo "fetch_ffmpeg: the download failed; if $RELEASE has been pruned, pin a newer month-end build" >&2
        rm -f "$zip.part"
        exit 1
    }
    printf '%s  %s\n' "$SHA256" "$zip.part" | sha256sum -c - >/dev/null || {
        echo "fetch_ffmpeg: checksum mismatch for $zip (expected $SHA256)" >&2
        rm -f "$zip.part"
        exit 1
    }
    mv "$zip.part" "$zip"
fi

# Extract the executable and the licence. There is no `unzip` in the dev shell, so use Python,
# which the other tooling already relies on.
python3 -I - "$zip" "$BUILD" "$exe" "$license" <<'PY'
import sys, zipfile
archive, build, exe, license = sys.argv[1:5]
with zipfile.ZipFile(archive) as z:
    for member, dest in ((f"{build}/bin/ffmpeg.exe", exe), (f"{build}/LICENSE.txt", license)):
        try:
            data = z.read(member)
        except KeyError:
            sys.exit(f"fetch_ffmpeg: {member!r} not found in {archive}; the build's layout may have changed")
        with open(dest, "wb") as f:
            f.write(data)
PY

# Stage beside the injector in every built target profile.
staged=0
for profile in debug release; do
    dir="$repo/target/x86_64-pc-windows-msvc/$profile"
    [ -d "$dir" ] || continue
    cp "$exe" "$dir/ffmpeg.exe"
    cp "$license" "$dir/ffmpeg-LICENSE.txt"
    echo "fetch_ffmpeg: staged -> $dir/ffmpeg.exe" >&2
    staged=1
done
if [ "$staged" = 0 ]; then
    echo "fetch_ffmpeg: no built target dir yet; ffmpeg is cached at $exe" >&2
fi
