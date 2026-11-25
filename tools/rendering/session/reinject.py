"""Eject the payload from a live private session's game and inject the freshly built one, through
the injector's `--reload` inside the game's container, then wait for the new payload to come up."""

import json
import logging
import os
import re
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

from tools.rendering.session.errors import SessionError

LOG = logging.getLogger("tools.rendering.session.reinject")
ROOT = Path(__file__).resolve().parents[3]
# The line the payload logs once the game is in gameplay with the VR session rendering; the
# session launcher waits for the same line.
READY_LINE = "camera height above Rico"
# How many injector runs one re-injection may take (see the sharing violation below).
INJECT_ATTEMPTS = 4
# The line a payload logs once the shader identity census's pixel-shader detour pins it: such a
# payload tears down on an eject and then refuses to unmap, which leaves the game without the mod.
PINNED_LINE = "the pixel-shader detour was enabled, which pins the payload"
# The line a payload logs once the texture census's detours pin it, for the same reason.
CENSUS_PINNED_LINE = "the texture detours were enabled, which pins the payload"


@dataclass(frozen=True)
class Reinjection:
    """What a re-injection did and how long each part took."""
    injector_seconds: float
    ready_seconds: float
    session_log: Path
    output: str


def build(release: bool = False, timeout: float = 900.0) -> float:
    """Build the payload and the injector incrementally; returns the seconds it took."""
    flag = " --release" if release else ""
    command = ("cargo xwin build --xwin-cache-dir .xwin --target x86_64-pc-windows-msvc "
               f"-p jc3vr_payload -p jc3vr_injector{flag}")
    started = time.monotonic()
    result = subprocess.run(["nix-shell", str(ROOT / "shell.nix"), "--run", command], cwd=ROOT,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=timeout,
                            text=True, errors="replace")
    if result.returncode:
        raise SessionError(f"build: cargo xwin build exited with {result.returncode}:\n"
                           f"{result.stdout[-4000:]}")
    return time.monotonic() - started


def reinject(runner_path: Path, release: bool = False, timeout: float = 120.0) -> Reinjection:
    """Re-inject the payload into the game of the session `runner_path` describes, and wait until
    the new payload logs gameplay readiness in a new session log."""
    state = _load(runner_path)
    sessions = Path(state["sessions_dir"])
    before = set(sessions.glob("*/session.log"))
    if before:
        current = max(before, key=lambda path: path.parent.name)
        text = current.read_text(errors="replace")
        for line, census in ((PINNED_LINE, "the shader identity census"), (CENSUS_PINNED_LINE, "the texture census")):
            if line in text:
                raise SessionError(
                    f"reinject: the loaded payload ran {census}, which pins it ({current}); "
                    "relaunch the session instead (python3 -m tools.rendering.session.matched_run N --build-only ...)")
    steam = Path(os.environ.get("STEAM_ROOT", "~/.steam/steam")).expanduser().resolve()
    proton = Path(os.environ.get("PROTON_DIR", str(steam / "steamapps/common/Proton - Experimental"))).resolve()
    client = steam / "steamapps/common/SteamLinuxRuntime_sniper/pressure-vessel/bin/steam-runtime-launch-client"
    profile = "release" if release else "debug"
    injector = ROOT / f"target/x86_64-pc-windows-msvc/{profile}/jc3vr_injector.exe"
    for path in (proton / "proton", client, injector, injector.with_name("jc3vr_payload.dll")):
        if not path.exists():
            raise SessionError(f"reinject: required path does not exist: {path}")
    env = os.environ.copy()
    for key in ("WAYLAND_DISPLAY", "WINEPREFIX", "STEAM_COMPAT_DATA_PATH", "XR_RUNTIME_JSON"):
        env.pop(key, None)
    env["XDG_RUNTIME_DIR"] = state["runtime_dir"]
    env["DBUS_SESSION_BUS_ADDRESS"] = f"unix:path={state['runtime_dir']}/bus"
    env.update(_display(Path(state["display_file"])))
    base = ["steam-run", str(client), f"--bus-name={state['bus_name']}", "--",
            str(proton / "proton"), "runinprefix", str(injector), "--no-capture-host"]
    started = time.monotonic()
    action = ["--reload"]
    for attempt in range(1, INJECT_ATTEMPTS + 1):
        try:
            result = subprocess.run([*base, *action], env=env, cwd=ROOT, stdin=subprocess.DEVNULL,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=timeout,
                                    text=True, errors="replace")
        except subprocess.TimeoutExpired as error:
            raise SessionError(f"reinject: the injector did not finish within {timeout:.0f} s") from error
        if result.returncode == 0:
            break
        # Wine can keep the ejected image's file open until the injector that ejected it exits, so
        # the copy of the new build fails; a fresh injector finds nothing loaded and injects.
        if "Sharing violation" not in result.stdout or attempt == INJECT_ATTEMPTS:
            raise SessionError(f"reinject: the injector exited with {result.returncode}:\n"
                               f"{result.stdout[-4000:]}")
        LOG.info("reinject: the ejected payload's file was still in use; injecting again")
        action = []
    injector_seconds = time.monotonic() - started
    LOG.info("reinject: injector finished in %.1f s", injector_seconds)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        for log in set(sessions.glob("*/session.log")) - before:
            if READY_LINE in log.read_text(errors="replace"):
                return Reinjection(injector_seconds, time.monotonic() - started, log, result.stdout)
        time.sleep(0.05)
    raise SessionError(f"reinject: the new payload did not log gameplay readiness within {timeout:.0f} s")


def _load(path: Path) -> dict:
    try:
        state = json.loads(Path(path).read_text())
    except (OSError, ValueError) as error:
        raise SessionError(f"reinject: could not read runner state {path}: {error}") from error
    for key in ("bus_name", "runtime_dir", "display_file", "sessions_dir"):
        if not isinstance(state.get(key), str) or not state[key]:
            raise SessionError(f"reinject: runner state has no {key}")
    return state


def _display(path: Path) -> dict:
    try:
        display = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise SessionError(f"reinject: cannot read the private display: {error}") from error
    if not re.fullmatch(r":\d+(?:\.\d+)?", display.get("DISPLAY", "")):
        raise SessionError("reinject: the session reported no local private DISPLAY")
    return {key: value for key, value in display.items() if key in ("DISPLAY", "XAUTHORITY")}
