#!/bin/sh
# Run a Windows executable under wine on the real GPU, through the DXVK that Proton ships: the same
# Direct3D 11 implementation the game runs on under Proton.
#
#   dxvk_run.sh <program.exe> [args...]
#
# Wine's own Direct3D (wined3d) runs Direct3D 11 compute shaders, but it is not what the game
# uses, and its timings say nothing about the game's. This copies the program next to Proton's
# DXVK `d3d11.dll` and `dxgi.dll` in a scratch directory under `target/` and runs it there in the
# shared prefix (see `wine_prefix.sh`), with overrides that select the DXVK DLLs. The prefix itself
# is left untouched, so every other tool keeps wine's Direct3D.
#
# It also serves as a cargo runner, so a test binary runs on the real GPU unchanged:
#
#   CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER="$PWD/scripts/dxvk_run.sh" \
#       ./scripts/xwin_test.sh -p body-estimator
#
# Overridable via env: STEAM_ROOT, PROTON_DIR (as `proton_run.sh`), and DXVK_LOG_LEVEL (default
# `none`).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)
. "$here/wine_prefix.sh"
jc3vr_ensure_wine_prefix

steam="${STEAM_ROOT:-$HOME/.steam/steam}"
proton="${PROTON_DIR:-$steam/steamapps/common/Proton - Experimental}"
dxvk="$proton/files/lib/wine/dxvk/x86_64-windows"
if [ ! -f "$dxvk/d3d11.dll" ] || [ ! -f "$dxvk/dxgi.dll" ]; then
    echo "dxvk_run: no DXVK at $dxvk; set PROTON_DIR to a Proton install" >&2
    exit 1
fi
if [ "$#" -lt 1 ]; then
    echo "usage: dxvk_run.sh <program.exe> [args...]" >&2
    exit 2
fi

exe=$(realpath "$1")
shift
run="$repo/target/dxvk-run"
mkdir -p "$run"
# `install` rather than `cp`: Steam's copies are read-only, and `cp` would carry that over and then
# refuse to overwrite them on the next run.
install -m 0644 "$dxvk/d3d11.dll" "$dxvk/dxgi.dll" "$run/"
install -m 0755 "$exe" "$run/"
cd "$run"
WINEDLLOVERRIDES="d3d11,dxgi=n;$WINEDLLOVERRIDES" DXVK_LOG_LEVEL="${DXVK_LOG_LEVEL:-none}" \
    exec wine "./$(basename "$exe")" "$@"
