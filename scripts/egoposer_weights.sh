#!/bin/sh
# Convert EgoPoser's released checkpoint into the safetensors file the body estimator loads, and
# stage it beside the built payload.
#
# The checkpoint is a PyTorch state dict (`egoposer.pth`, the base model, from the authors' Google
# Drive folder linked in https://github.com/eth-siplab/EgoPoser). It is trained on AMASS, whose
# licence is non-commercial, so it is never committed and never downloaded by this script: fetch it
# by hand (or with `uvx gdown --folder <the folder's link>`), then run
#
#   scripts/egoposer_weights.sh path/to/egoposer.pth
#
# The conversion keeps PyTorch's own parameter names and stores every tensor as 32-bit floats; the
# payload rounds the weight matrices to 16 bits as it uploads them. The result is cached as
# `.egoposer/egoposer.safetensors` and copied next to `jc3vr_payload.dll` in every built target
# profile, where the payload looks for it by default (`docs/mod/body/body-estimator.md`). PyTorch
# comes from `uv`, in a throwaway environment, from PyTorch's CPU-only index.
set -eu

if [ $# -ne 1 ] || [ ! -f "$1" ]; then
    echo "usage: scripts/egoposer_weights.sh path/to/egoposer.pth" >&2
    exit 2
fi
command -v uv >/dev/null || {
    echo "egoposer_weights: uv is not on PATH; it provides PyTorch for the conversion" >&2
    exit 1
}

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)
cache="$repo/.egoposer"
out="$cache/egoposer.safetensors"
pth=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
mkdir -p "$cache"

converter=$(mktemp "${TMPDIR:-/tmp}/egoposer_to_safetensors.XXXXXX.py")
trap 'rm -f "$converter"' EXIT
cat > "$converter" <<'PY'
"""Convert an EgoPoser .pth state dict to safetensors, keeping PyTorch's parameter names."""
import sys

import torch
from safetensors.torch import save_file

source, target = sys.argv[1], sys.argv[2]
state = torch.load(source, map_location="cpu", weights_only=True)
if not isinstance(state, dict) or not all(torch.is_tensor(v) for v in state.values()):
    sys.exit(f"egoposer_weights: {source} is not a state dict of tensors")
state = {name: tensor.contiguous().float() for name, tensor in state.items()}
save_file(state, target, metadata={"format": "pt"})
print(f"egoposer_weights: wrote {len(state)} tensors to {target}", file=sys.stderr)
PY

echo "egoposer_weights: converting $pth" >&2
uv run --quiet --no-project --python 3.12 \
    --index pytorch-cpu=https://download.pytorch.org/whl/cpu \
    --with torch --with safetensors --with numpy \
    python -I "$converter" "$pth" "$out"

staged=0
for profile in debug release; do
    dir="$repo/target/x86_64-pc-windows-msvc/$profile"
    [ -d "$dir" ] || continue
    cp "$out" "$dir/egoposer.safetensors"
    echo "egoposer_weights: staged -> $dir/egoposer.safetensors" >&2
    staged=1
done
if [ "$staged" = 0 ]; then
    echo "egoposer_weights: no built target dir yet; the weights are cached at $out" >&2
fi
