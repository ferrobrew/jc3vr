#!/bin/sh
# Fetch JPEXS FFDec (the Flash decompiler) into `vendor/ffdec/`.
#
# FFDec reads the game's Scaleform movies (`.gfx`, a SWF with a renamed signature), decompiles their
# ActionScript 3, and exports their art. It is a Java program and is not in nixpkgs, so this script
# provides it from a pinned, checksum-verified GitHub release. The download and the extracted tree
# live under `vendor/ffdec/`, which is git-ignored; only the first run needs the network. Run it
# through `scripts/ffdec.sh`, which fetches on demand.
#
#   scripts/fetch_ffdec.sh    # download (if needed), verify, and extract
set -eu

# Pinned FFDec release: the platform-independent zip (the jar plus its libraries). Bump both
# together, from https://github.com/jindrapetrik/jpexs-decompiler/releases.
VERSION=26.3.0
SHA256=35f4930eb7c380afe66f2117f90b006deac0631473ad7500bb39c78f68645ecd
URL="https://github.com/jindrapetrik/jpexs-decompiler/releases/download/version${VERSION}/ffdec_${VERSION}.zip"

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)
dest="$repo/vendor/ffdec"
zip="$dest/ffdec_${VERSION}.zip"
tree="$dest/$VERSION"
stamp="$tree/.extracted"

mkdir -p "$dest"

# Download once, verified against the pinned hash. Re-fetch if the cached zip is missing or its hash
# no longer matches (a partial or corrupt download).
if [ ! -f "$zip" ] || ! printf '%s  %s\n' "$SHA256" "$zip" | sha256sum -c - >/dev/null 2>&1; then
    echo "fetch_ffdec: downloading FFDec $VERSION" >&2
    curl -fSL --retry 3 -o "$zip.part" "$URL"
    printf '%s  %s\n' "$SHA256" "$zip.part" | sha256sum -c - >/dev/null || {
        echo "fetch_ffdec: checksum mismatch for $zip (expected $SHA256)" >&2
        rm -f "$zip.part"
        exit 1
    }
    mv "$zip.part" "$zip"
    rm -rf "$tree"
fi

# Extract once. The stamp is written last, so an interrupted extraction is redone on the next run.
# There is no `unzip` in the dev shell, so use Python, which the other tooling already relies on.
if [ ! -f "$stamp" ]; then
    rm -rf "$tree"
    python3 - "$zip" "$tree" "$VERSION" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as z:
    if "ffdec.jar" not in z.namelist():
        sys.exit(f"fetch_ffdec: ffdec.jar not found in {sys.argv[1]}; "
                 f"the release layout may have changed in version {sys.argv[3]}")
    z.extractall(sys.argv[2])
PY
    touch "$stamp"
    echo "fetch_ffdec: extracted -> $tree" >&2
fi

# Point `current` at the pinned version, so the wrapper does not have to know it.
ln -sfn "$VERSION" "$dest/current"
