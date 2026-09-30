#!/bin/sh
# Run JPEXS FFDec headless, fetching it first if needed (see `scripts/fetch_ffdec.sh`).
#
# Arguments are passed straight to FFDec's command line. Run it inside the dev shell, which provides
# the JDK. For example, to decompile a movie's ActionScript into a git-ignored directory:
#
#   nix-shell --run 'scripts/ffdec.sh -export script target/ffdec/pause path/to/pause.gfx'
#
# Never write decompiled game code or extracted game art into a tracked path.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)

"$here/fetch_ffdec.sh"

command -v java >/dev/null 2>&1 || {
    echo "ffdec: no \`java\` on PATH; run this inside \`nix-shell\`" >&2
    exit 1
}

# `java.awt.headless` keeps FFDec off the display when it only exports.
exec java -Djava.awt.headless=true -jar "$repo/vendor/ffdec/current/ffdec.jar" "$@"
