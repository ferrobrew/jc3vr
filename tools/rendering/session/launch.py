#!/usr/bin/env python3

import contextlib
import fcntl
import json
import logging
import math
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

from tools.rendering.capture.probe import check_probe_options
from tools.rendering.capture.validation import read_object, validate_capture
from tools.rendering.protocol import (
    Capture,
    Command,
    ComparisonReport,
    Mode,
    NativeCapture,
    Outcome,
    PhaseCapture,
    ProtocolError,
    Request,
    RunOutcome,
    RunReport,
    ScreenshotAck,
    SmokeResult,
)
from tools.rendering.session.channel import RequestChannel
from tools.rendering.session.errors import SessionError
from tools.rendering.session.options import Options
from tools.rendering.session.processes import ProcessError, processes, steam_client
from tools.rendering.viewer.server import DEFAULT_POINTER, publish_run

LOG = logging.getLogger("tools.rendering.session.launch")
ROOT = Path(__file__).resolve().parents[3]
MANIFEST = Path("/run/current-system/sw/share/openxr/1/openxr_monado.json")
RUNNER_POINTER = ROOT / "target/visual-harness/runner.json"



class RenderingSession(RequestChannel):
    def __init__(self, options=None, smoke=None):
        self.options = options or Options()
        self.smoke_check = smoke
        self.token = uuid.uuid4().hex
        self.bus_name = f"com.jc3vr.RenderingSession.r{self.token}"
        self.processes = []
        self.logs = []
        self.runtime = None
        self.work = None
        self.output = None
        self.env = None
        self.launch = None
        self.monado = None
        self.bus = None
        self.deadline = None
        self.menu_deadline = None
        self.last_menu = 0.0
        self.gameplay_ready = False
        self._closed = False
        self._lock = None
        self.result = RunReport()
        self.cleanup_errors = self.result.cleanup_errors
        self.config_dir = None
        self.pending_request: Request | None = None

    def check(self):
        for name in ("timeout", "startup_timeout", "build_timeout", "copy_timeout", "menu_interval", "menu_timeout"):
            value = getattr(self.options, name)
            if not math.isfinite(value) or value <= 0:
                raise SessionError(f"session: {name} must be finite and positive")
        if type(self.options.samples) is not int or self.options.samples < 3:
            raise SessionError("session: samples must be an integer of at least three")
        if type(self.options.warmup_frames) is not int or not 1 <= self.options.warmup_frames <= 300:
            raise SessionError("session: warmup_frames must be an integer from 1 through 300")
        check_probe_options(self.options)
        execution = (self.options.suite, self.options.orientation_sweep,
                     self.options.motion_sweep, self.options.smoke,
                     self.options.single_pass_probe)
        if sum(execution) > 1 or (self.options.launch_only and any(execution)):
            raise SessionError("session: --suite, --orientation-sweep, --motion-sweep, --single-pass-probe, --smoke, and --launch-only are mutually exclusive")
        if self.options.keep_alive and any(execution):
            raise SessionError("session: --keep-alive is mutually exclusive with an execution mode")
        self.reject_existing()
        self.require_steam_client()
        if self.options.output is not None:
            output = self.options.output.expanduser().resolve()
            if output.exists() and (not output.is_dir() or any(output.iterdir())):
                raise SessionError(f"session: output must be an empty directory: {output}")
        commands = ("cp", "gamescope", "steam-run", "setpriv", "dbus-daemon", "nix-shell")
        for command in commands + (("uv",) if self.options.suite or self.options.orientation_sweep or self.options.motion_sweep else ()):
            if not shutil.which(command):
                raise SessionError(f"session: missing executable {command}")
        self.steam = Path(os.environ.get("STEAM_ROOT", "~/.steam/steam")).expanduser().resolve()
        self.appid = os.environ.get("JC3_APPID", "225540")
        self.library = self._library()
        self.source_prefix = self.library / "steamapps/compatdata" / self.appid
        self.game = Path(os.environ.get("JC3_EXE", str(self.library / "steamapps/common/Just Cause 3/JustCause3.exe"))).resolve()
        self.proton = Path(os.environ.get("PROTON_DIR", str(self.steam / "steamapps/common/Proton - Experimental"))).resolve()
        self.sniper = self.steam / "steamapps/common/SteamLinuxRuntime_sniper/_v2-entry-point"
        self.client = self.sniper.parent / "pressure-vessel/bin/steam-runtime-launch-client"
        self.boot = Path(os.environ.get("JC3BOOT_DIR", str(ROOT.parent / "jc3boot"))).resolve()
        profile = "release" if self.options.release else "debug"
        self.boot_injector = self.boot / f"target/x86_64-pc-windows-msvc/{profile}/jc3boot_injector.exe"
        self.injector = ROOT / f"target/x86_64-pc-windows-msvc/{profile}/jc3vr_injector.exe"
        try:
            library = Path(json.loads(MANIFEST.read_text())["runtime"]["library_path"])
        except (OSError, KeyError, ValueError, TypeError) as error:
            raise SessionError(f"session: cannot read Monado manifest {MANIFEST}: {error}") from error
        if not library.is_absolute():
            library = MANIFEST.resolve().parent / library
        self.monado_service = library.resolve().parent.parent / "bin/monado-service"
        for path in (self.source_prefix / "pfx", self.game, self.proton / "proton",
                     self.sniper, self.client, self.monado_service, library):
            if not path.exists():
                raise SessionError(f"session: required path does not exist: {path}")
        self.vr_override = self._xrizer()
        self.gpu = self._gpu()
        self.xdotool = self.options.xdotool or shutil.which("xdotool")
        if not self.xdotool:
            candidates = sorted(Path("/nix/store").glob("*-xdotool-*/bin/xdotool"))
            self.xdotool = str(candidates[-1]) if candidates else None
        if not self.xdotool or not os.access(self.xdotool, os.X_OK):
            raise SessionError("session: xdotool is unavailable; pass --xdotool or install it with nix-build '<nixpkgs>' -A xdotool --no-out-link")
        if self.options.no_build:
            self._check_artifacts()
        LOG.info("checks pass: GPU %s, prefix %s, Monado %s", self.gpu, self.source_prefix, self.monado_service)
        return self

    def start(self):
        if self.env is not None or self._closed:
            raise SessionError("session: runner cannot be started twice")
        self.check()
        try:
            self._acquire_lock()
            self.reject_existing()
            base = ROOT / "target/visual-harness"
            base.mkdir(parents=True, exist_ok=True)
            self.work = Path(tempfile.mkdtemp(prefix="run-", dir=base))
            self.output = (self.options.output or self.work / "artifacts").expanduser().resolve()
            if self.output.exists() and (not self.output.is_dir() or any(self.output.iterdir())):
                self.output = None
                raise SessionError("session: output became nonempty before launch")
            self.output.mkdir(parents=True, exist_ok=True)
            self._write_report()
            publish_run(self.output, DEFAULT_POINTER)
            runtime_base = Path(os.environ.get("XDG_RUNTIME_DIR", "/tmp"))
            self.runtime = Path(tempfile.mkdtemp(prefix="jc3vr-", dir=runtime_base))
            os.chmod(self.runtime, 0o700)
            self.prefix = self.work / "compatdata"
            self.config_dir = self.output / "config"
            self.sessions_dir = self.output / "sessions"
            self.display_file = self.output / "display.json"
            for path in (self.config_dir, self.sessions_dir):
                path.mkdir(exist_ok=True)
            self._seed_settings()
            self._base_env()
            if not self.options.no_build:
                self._build()
            self._check_artifacts()
            self._command(["cp", "-a", "--reflink=auto", str(self.source_prefix), str(self.prefix)],
                          "prefix-copy", self.options.copy_timeout)
            self._isolate_profile_links()
            self.reject_existing()
            self.deadline = time.monotonic() + self.options.timeout
            self.bus = self._spawn(["dbus-daemon", "--session", "--nofork", "--nopidfile",
                                    f"--address={self.env['DBUS_SESSION_BUS_ADDRESS']}"], "bus")
            self._wait_socket(self.runtime / "bus", self.options.startup_timeout)
            self.monado = self._spawn([str(self.monado_service)], "monado", stdin=subprocess.PIPE)
            self._wait_socket(self.runtime / "monado_comp_ipc", self.options.startup_timeout)
            self.launch = self._spawn(self._launch_command(), "gamescope")
            self._wait_for(lambda: self.display_file.exists(), "the private gamescope display",
                           self.options.startup_timeout)
            self.menu_deadline = time.monotonic() + self.options.menu_timeout
            self._write_state()
            LOG.info("private session started; artifacts: %s", self.output)
            return self
        except BaseException:
            self.close()
            raise

    def run(self, callback=None):
        try:
            if self.options.keep_alive and callback is not None:
                raise SessionError("session: keep-alive does not run an execution callback")
            self.start()
            self.result.status = RunOutcome.STARTED
            self._write_report()
            if self.options.keep_alive:
                self.inject_until_ready()
                self.deadline = math.inf
                self._write_report()
            if callback is not None:
                result = callback(self)
                self.result.complete(result)
                return result
            self.serve()
            return None
        except BaseException as error:
            self.result.fail(error)
            raise
        finally:
            self.close()
            self._write_report()

    def serve(self):
        while True:
            self.assert_running()
            self.pump_menu()
            time.sleep(0.2)

    def smoke(self) -> SmokeResult:
        if self.smoke_check is None:
            raise SessionError("session: no smoke check is configured")
        return self.smoke_check(self)

    def screenshot(self, capture_id: str, mode: Mode | None = None, generation: int | None = None, motion_capture: PhaseCapture | None = None) -> NativeCapture | Capture:
        result = self.request(Request(Command.SCREENSHOT, capture_id, motion_capture=motion_capture))
        if not isinstance(result, ScreenshotAck):
            raise ProtocolError("visual protocol: expected a screenshot acknowledgement")
        capture = validate_capture(self.config_dir / "captures", capture_id, result.capture,
                                   mode=mode, generation=generation, motion_capture=motion_capture)
        self.mark_gameplay_ready()
        return capture

    def settle(self, seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            self.assert_running()
            time.sleep(min(0.2, max(0, end - time.monotonic())))
        self.assert_running()

    def compare(self, manifest):
        output = self.output / "comparison"
        env = dict(self.env if self.env is not None else os.environ)
        for name in ("PYTHONPATH", "PYTHONHOME"):
            env.pop(name, None)
        outcome = self._command(["uv", "run", "--no-project", "--with", "numpy", "--with", "pillow",
                                 "python", "-m", "tools.rendering", "compare", str(manifest),
                                 "--output", str(output)], "visual-compare", 120, env=env, check=False)
        report = ComparisonReport.from_wire(read_object(output / "report.json"))
        if outcome.returncode != report.exit_code:
            raise SessionError(f"session: comparison {report.status} disagrees with process status {outcome.returncode}; see {output}")
        if report.status != Outcome.PASS:
            LOG.warning("session: comparison %s; see %s", report.status, output)
        return report

    def _write_report(self):
        if self.output is None:
            return
        path = self.output / "result.json"
        temporary = path.with_suffix(".tmp")
        temporary.write_text(json.dumps(self.result.to_wire(), indent=2) + "\n")
        temporary.replace(path)

    def assert_running(self):
        if self.deadline is not None and time.monotonic() >= self.deadline:
            raise SessionError("session: the run timeout expired")
        for name, process in (("gamescope", self.launch), ("Monado", self.monado), ("D-Bus", self.bus)):
            if process is not None and process.poll() is not None:
                raise SessionError(f"session: {name} exited with status {process.returncode}; see {self.output}")

    def display_env(self):
        try:
            display = json.loads(self.display_file.read_text())
        except (OSError, ValueError) as error:
            raise SessionError(f"session: cannot read the private display: {error}") from error
        if not re.fullmatch(r":\d+(?:\.\d+)?", display.get("DISPLAY", "")):
            raise SessionError("session: the child did not report a local private DISPLAY")
        env = self.env.copy()
        env.update(display)
        return env

    def mark_gameplay_ready(self):
        self.gameplay_ready = True

    def pump_menu(self):
        self.assert_running()
        now = time.monotonic()
        if self.gameplay_ready or self.menu_deadline is None or now >= self.menu_deadline:
            return False
        if now - self.last_menu < self.options.menu_interval:
            return False
        self.last_menu = now
        env = self.display_env()
        command_timeout = 0.5
        hold_seconds = 0.3
        end = min(now + 2, self.menu_deadline, self.deadline if self.deadline is not None else math.inf)

        def command(arguments, name):
            remaining = end - time.monotonic()
            if self.gameplay_ready or remaining <= 0:
                return None
            return self._command([self.xdotool, *arguments], name, min(command_timeout, remaining), env=env, check=False)
        windows = set()
        for selector in ("--class", "--name"):
            result = command(["search", "--onlyvisible", selector, "[Jj]ust.?[Cc]ause"], "menu-search")
            if result is None:
                return False
            if result.returncode not in (0, 1):
                raise SessionError(f"session: xdotool search exited with {result.returncode}")
            windows.update(line for line in result.stdout.splitlines() if line.isdecimal())
        if not windows:
            return False
        window = str(int(max(windows, key=int)))
        result = command(["windowfocus", window], "menu-focus")
        if result is None or result.returncode:
            return False
        result = command(["getwindowfocus", "-f"], "menu-verify-focus")
        if result is None or result.returncode or result.stdout.strip() != window:
            return False
        if self.gameplay_ready or end - time.monotonic() < 2 * command_timeout + hold_seconds:
            return False

        def key(action):
            try:
                subprocess.run([self.xdotool, action, "Return"], env=env, timeout=command_timeout,
                               stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=True)
            except (OSError, subprocess.SubprocessError) as error:
                raise SessionError(f"session: menu {action}: {error}") from error

        try:
            key("keydown")
            # The XR frame loop can sample input less often than a short xdotool key press lasts.
            time.sleep(hold_seconds)
        finally:
            pending = sys.exception()
            try:
                # Release even after interruption or deadline expiry; never inherit the host display.
                key("keyup")
            except SessionError as error:
                if pending is None:
                    raise
                pending.add_note(str(error))
                LOG.error("%s", error)
        LOG.info("sent held Return to the private game window %s", window)
        return True

    def run_in_container(self, command, timeout=20, check=True):
        self.assert_running()
        return self._command(["steam-run", str(self.client), f"--bus-name={self.bus_name}", "--", *map(str, command)],
                             "container-command", timeout, env=self.display_env(), check=check)

    def inject(self, timeout=30, check=True):
        return self.run_in_container([self.proton / "proton", "runinprefix", self.injector,
                                      "--no-capture-host"], timeout=timeout, check=check)

    def inject_until_ready(self):
        end = time.monotonic() + self.options.startup_timeout
        if self.deadline is not None:
            end = min(end, self.deadline)
        while True:
            self.assert_running()
            outcome = self.inject(timeout=min(30, max(0.1, end - time.monotonic())), check=False)
            if outcome.returncode == 0:
                return outcome
            text = outcome.stdout.lower()
            retryable = "no justcause3.exe process found" in text or any(
                phrase in text for phrase in ("serviceunknown", "namehasnoowner", "no owner for name",
                                              "unable to connect to any of the specified bus names")
            )
            if not retryable:
                raise SessionError(f"session: injection failed and is not safe to retry: {outcome.stdout[-2000:]}")
            if time.monotonic() >= end:
                raise SessionError("session: timed out waiting for the game before injection")
            self.pump_menu()
            time.sleep(1)

    def close(self):
        if self._closed:
            return
        self._closed = True
        try:
            self._terminate_owned()
        finally:
            self._remove_state_pointer()
            for process in self.processes:
                if process.stdin is not None:
                    process.stdin.close()
                with contextlib.suppress(subprocess.TimeoutExpired):
                    process.wait(timeout=0.2)
            for stream in self.logs:
                stream.close()
            paths = () if self.cleanup_errors else (getattr(self, "prefix", None), self.runtime)
            for path in paths:
                if path is not None and path.exists():
                    try:
                        shutil.rmtree(path)
                    except OSError as error:
                        self.cleanup_errors.append(f"cannot remove {path}: {error}")
                        LOG.error("cleanup cannot remove %s: %s", path, error)
            if self._lock is not None:
                self._lock.close()
                self._lock = None

    def __enter__(self):
        return self.start()

    def __exit__(self, *_):
        self.close()

    @staticmethod
    def reject_existing():
        try:
            if found := [f"{process.pid} ({process.comm})" for process in processes() if process.conflicts]:
                raise SessionError(f"session: refusing to coexist with a game or Monado service: {', '.join(found)}")
        except ProcessError as error:
            raise SessionError(str(error)) from error

    @staticmethod
    def require_steam_client():
        try:
            client = steam_client()
        except ProcessError as error:
            raise SessionError(str(error)) from error
        if client is None:
            raise SessionError("session: Steam client is not running; start Steam and sign in before launching a session")

    def _library(self):
        if os.environ.get("JC3_LIBRARY"):
            return Path(os.environ["JC3_LIBRARY"]).expanduser().resolve()
        try:
            data = (self.steam / "steamapps/libraryfolders.vdf").read_text()
        except FileNotFoundError:
            return self.steam
        for match in re.finditer(r'"path"\s+"((?:\\.|[^"\\])*)"', data):
            path = Path(match.group(1).replace("\\\\", "\\").replace('\\"', '"'))
            if (path / f"steamapps/appmanifest_{self.appid}.acf").exists():
                return path.resolve()
        return self.steam

    def _xrizer(self):
        if os.environ.get("VR_OVERRIDE"):
            path = Path(os.environ["VR_OVERRIDE"])
        else:
            config = Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config")))
            try:
                paths = json.loads((config / "openvr/openvrpaths.vrpath").read_text())["runtime"]
                path = next(Path(value) for value in paths if "xrizer" in value)
            except (OSError, ValueError, KeyError, TypeError, StopIteration) as error:
                raise SessionError("session: xrizer is unavailable; set VR_OVERRIDE") from error
        if not path.is_dir():
            raise SessionError(f"session: xrizer runtime does not exist: {path}")
        return path.resolve()

    def _gpu(self):
        gpus = []
        for card in sorted(Path("/sys/class/drm").glob("card[0-9]*")):
            if not re.fullmatch(r"card\d+", card.name):
                continue
            try:
                vendor = (card / "device/vendor").read_text().strip().removeprefix("0x")
                device = (card / "device/device").read_text().strip().removeprefix("0x")
            except OSError:
                continue
            gpus.append(f"{vendor}:{device}")
        selected = self.options.gpu or next((gpu for gpu in gpus if gpu.startswith("10de:")), None)
        selected = selected or (gpus[0] if len(gpus) == 1 else None)
        if selected is None or not re.fullmatch(r"[\da-fA-F]{4}:[\da-fA-F]{4}", selected):
            raise SessionError("session: select a GPU using --gpu vendor:device")
        selected = selected.lower()
        if selected not in gpus:
            raise SessionError(f"session: requested GPU {selected} is not in sysfs: {gpus}")
        if gpus.count(selected) > 1:
            raise SessionError(f"session: GPU ID {selected} is ambiguous")
        return selected

    def _check_artifacts(self):
        for path in (self.boot_injector, self.boot_injector.with_name("jc3boot_payload.dll"),
                     self.injector, self.injector.with_name("jc3vr_payload.dll")):
            if not path.is_file():
                raise SessionError(f"session: missing build artifact: {path}")

    def _acquire_lock(self):
        path = Path(tempfile.gettempdir()) / f"jc3vr-visual-harness-{os.getuid()}.lock"
        fd = os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
        self._lock = os.fdopen(fd, "w")
        try:
            fcntl.flock(self._lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            self._lock.close()
            self._lock = None
            raise SessionError("session: another private runner holds the launch lock") from error

    def _seed_settings(self):
        eye_size = self.options.eye_size
        override = "" if eye_size is None else f"recommended_size_override = [{eye_size[0]}, {eye_size[1]}]\n"
        (self.config_dir / "settings.toml").write_text(
            f"version = 1\n[vr]\nmirror = false\nresolution_scale = {self.options.resolution_scale!r}\n"
            f"{override}[controllers]\nenabled = false\n[foveation]\nenabled = false\n"
        )

    def _base_env(self):
        self.env = os.environ.copy()
        self.env["JC3VR_HOST_DISPLAY"] = self.env.get("DISPLAY", "")
        for key in ("WAYLAND_DISPLAY", "WINEPREFIX", "STEAM_COMPAT_DATA_PATH",
                    "XR_RUNTIME_JSON", "XRT_COMPOSITOR_FORCE_GPU_INDEX", "XRT_COMPOSITOR_FORCE_CLIENT_GPU_INDEX",
                    "VK_ICD_FILENAMES", "VK_DRIVER_FILES", "DRI_PRIME", "PROTON_VR_RUNTIME"):
            self.env.pop(key, None)
        alsa = self.runtime / "asound.conf"
        alsa.write_text("pcm.!default { type null }\nctl.!default { type hw card 999 }\n")
        private_config = self.runtime / "config"
        private_config.mkdir()
        openxr_config = private_config / "openxr/1"
        openxr_config.mkdir(parents=True)
        (openxr_config / "active_runtime.json").symlink_to(MANIFEST.resolve())
        openvr_config = private_config / "openvr"
        openvr_config.mkdir()
        (openvr_config / "openvrpaths.vrpath").write_text(json.dumps({
            "version": 1, "jsonid": "vrpathreg", "runtime": [str(self.vr_override)],
        }) + "\n")
        self.env.update({
            "JC3VR_HARNESS_TOKEN": self.token,
            "XDG_RUNTIME_DIR": str(self.runtime), "XDG_CONFIG_HOME": str(private_config),
            "DBUS_SESSION_BUS_ADDRESS": f"unix:path={self.runtime}/bus",
            "SIMULATED_ENABLE": "1", "XRT_COMPOSITOR_NULL": "1",
            "XRT_COMPOSITOR_NULL_FPS": str(self.options.compositor_fps),
            "P_OVERRIDE_ACTIVE_CONFIG": "none", "VR_OVERRIDE": str(self.vr_override),
            "PRESSURE_VESSEL_IMPORT_OPENXR_1_RUNTIMES": "1",
            "PRESSURE_VESSEL_FILESYSTEMS_RW": f"{self.runtime}:{self.work}:{self.output}",
            "STEAM_COMPAT_CLIENT_INSTALL_PATH": str(self.steam),
            "STEAM_COMPAT_DATA_PATH": str(self.prefix),
            "STEAM_COMPAT_APP_ID": self.appid, "SteamAppId": self.appid, "SteamGameId": self.appid,
            "JC3_BUS_NAME": self.bus_name,
            "JC3VR_CONFIG_DIR": str(self.config_dir), "JC3VR_SESSIONS_DIR": str(self.sessions_dir),
            "PULSE_SERVER": f"unix:{self.runtime}/no-pulse", "ALSA_CONFIG_PATH": str(alsa),
            "PIPEWIRE_REMOTE": "jc3vr-no-pipewire",
        })

    def _build(self):
        flag = " --release" if self.options.release else ""
        for root, prefix in ((self.boot, "jc3boot"), (ROOT, "jc3vr")):
            command = ("cargo xwin build --xwin-cache-dir .xwin --target x86_64-pc-windows-msvc "
                       f"-p {prefix}_payload -p {prefix}_injector{flag}")
            self._command(["nix-shell", str(root / "shell.nix"), "--run", command],
                          f"build-{prefix}", self.options.build_timeout, cwd=root)

    def _isolate_profile_links(self):
        users = self.prefix / "pfx/drive_c/users"
        for path in sorted(users.rglob("*")):
            if not path.is_symlink():
                continue
            target = path.resolve()
            if target.is_relative_to(self.prefix.resolve()):
                continue
            save = target / "Square Enix/Just Cause 3"
            path.unlink()
            path.mkdir()
            if save.is_dir():
                destination = path / "Square Enix"
                destination.mkdir()
                self._command(["cp", "-a", "--reflink=auto", str(save), str(destination)],
                              "save-copy", self.options.copy_timeout)

    def _launch_command(self):
        allowed = {
            "XDG_RUNTIME_DIR", "XDG_CONFIG_HOME", "DBUS_SESSION_BUS_ADDRESS",
            "SIMULATED_ENABLE", "XRT_COMPOSITOR_NULL", "XRT_COMPOSITOR_NULL_FPS",
            "P_OVERRIDE_ACTIVE_CONFIG", "VR_OVERRIDE", "JC3VR_HARNESS_TOKEN", "JC3_BUS_NAME",
            "JC3VR_CONFIG_DIR", "JC3VR_SESSIONS_DIR", "PULSE_SERVER", "ALSA_CONFIG_PATH", "PIPEWIRE_REMOTE",
            "STEAM_COMPAT_CLIENT_INSTALL_PATH", "STEAM_COMPAT_DATA_PATH", "STEAM_COMPAT_APP_ID",
            "SteamAppId", "SteamGameId", "PRESSURE_VESSEL_IMPORT_OPENXR_1_RUNTIMES",
            "PRESSURE_VESSEL_FILESYSTEMS_RW", "MESA_VK_DEVICE_SELECT", "DXVK_FILTER_DEVICE_UUID",
            "DXVK_FILTER_DEVICE_NAME", "DXVK_FILTER_DEVICE_VENDOR_ID", "DXVK_FILTER_DEVICE_DEVICE_ID",
            "VKD3D_FILTER_DEVICE_NAME", "VKD3D_VULKAN_DEVICE", "__NV_PRIME_RENDER_OFFLOAD",
            "__VK_LAYER_NV_optimus", "VK_DRIVER_FILES", "VK_ICD_FILENAMES",
        }
        forwarded = [f"{key}={value}" for key, value in self.env.items() if key in allowed]
        chain = ["setpriv", "--inh-caps=-all", "--ambient-caps=-all", "steam-run", "env", *forwarded,
                 str(self.sniper), "--verb=waitforexitandrun", "--", "steam-runtime-launcher-service",
                 f"--bus-name={self.bus_name}", "--hint", "--", str(self.proton / "proton"),
                 "waitforexitandrun", str(self.boot_injector), "--spawn", str(self.game)]
        return ["gamescope", "--backend", "headless", "--prefer-vk-device", self.gpu,
                "-W", "1280", "-H", "720", "-w", "1280", "-h", "720", "-r", "30", "--",
                sys.executable, "-m", "tools.rendering", "launch", "_child", str(self.display_file), *chain]

    def _write_state(self):
        state = {"bus_name": self.bus_name, "gpu": self.gpu, "runtime_dir": str(self.runtime),
                 "prefix": str(self.prefix), "config_dir": str(self.config_dir),
                 "sessions_dir": str(self.sessions_dir), "display_file": str(self.display_file),
                 "token": self.token}
        text = json.dumps(state, indent=2) + "\n"
        path = self.output / "runner.json"
        temporary = path.with_suffix(".tmp")
        temporary.write_text(text)
        temporary.replace(path)
        try:
            RUNNER_POINTER.parent.mkdir(parents=True, exist_ok=True)
            temporary = RUNNER_POINTER.with_suffix(".tmp")
            temporary.write_text(text)
            temporary.replace(RUNNER_POINTER)
        except OSError as error:
            LOG.warning("cannot write the session runner pointer %s: %s", RUNNER_POINTER, error)

    def _remove_state_pointer(self):
        try:
            value = json.loads(RUNNER_POINTER.read_text())
        except (OSError, ValueError):
            return
        if value.get("token") != self.token:
            return
        with contextlib.suppress(OSError):
            RUNNER_POINTER.unlink()

    def _spawn(self, command, name, stdin=subprocess.DEVNULL, env=None, cwd=None):
        stream = (self.output / f"{name}.log").open("ab")
        self.logs.append(stream)
        LOG.info("starting %s", name)
        process = subprocess.Popen(command, env=env or self.env, cwd=cwd or ROOT,
                                   stdin=stdin, stdout=stream, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        self.processes.append(process)
        return process

    def _command(self, command, name, timeout, env=None, check=True, cwd=None):
        if self.deadline is not None:
            timeout = min(timeout, self.deadline - time.monotonic())
        if timeout <= 0:
            raise SessionError(f"session: {name}: the run timeout expired")
        log_path = self.output / f"{name}.log"
        offset = log_path.stat().st_size if log_path.exists() else 0
        process = self._spawn(command, name, env=env, cwd=cwd)
        try:
            code = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired as error:
            self._signal_group(process, signal.SIGKILL)
            process.wait(timeout=3)
            raise SessionError(f"session: {name} exceeded {timeout:.1f}s") from error
        with log_path.open("rb") as stream:
            stream.seek(offset)
            text = stream.read().decode(errors="replace")
        result = subprocess.CompletedProcess(command, code, text, "")
        if check and code:
            raise SessionError(f"session: {name} exited with status {code}; see {self.output / (name + '.log')}")
        return result

    def _wait_for(self, predicate, description, timeout):
        end = min(self.deadline, time.monotonic() + timeout)
        while time.monotonic() < end:
            self.assert_running()
            if predicate():
                return
            time.sleep(0.1)
        raise SessionError(f"session: timed out waiting for {description}")

    def _wait_socket(self, path, timeout):
        def ready():
            with socket.socket(socket.AF_UNIX) as client:
                client.settimeout(0.1)
                try:
                    client.connect(str(path))
                    return True
                except (TimeoutError, FileNotFoundError, ConnectionRefusedError):
                    return False
        self._wait_for(ready, str(path), timeout)

    @staticmethod
    def _signal_group(process, sig):
        if process.poll() is None:
            with contextlib.suppress(ProcessLookupError):
                os.killpg(process.pid, sig)

    def _owned_pids(self):
        marker = f"JC3VR_HARNESS_TOKEN={self.token}".encode()
        prefix_path = getattr(self, "prefix", None)
        prefix = str(prefix_path).encode() if prefix_path is not None else b""
        result = []
        for entry in Path("/proc").iterdir():
            if not entry.name.isdecimal() or int(entry.name) == os.getpid():
                continue
            try:
                if entry.stat().st_uid != os.getuid():
                    continue
                env = (entry / "environ").read_bytes().split(b"\0")
                owned = marker in env or (prefix and (b"STEAM_COMPAT_DATA_PATH=" + prefix in env
                         or b"WINEPREFIX=" + prefix + b"/pfx" in env))
                if owned:
                    fd = os.pidfd_open(int(entry.name))
                    try:
                        current = (entry / "environ").read_bytes().split(b"\0")
                        if marker in current or (prefix and (
                                b"STEAM_COMPAT_DATA_PATH=" + prefix in current
                                or b"WINEPREFIX=" + prefix + b"/pfx" in current)):
                            result.append(fd)
                            fd = None
                    finally:
                        if fd is not None:
                            os.close(fd)
            except (FileNotFoundError, ProcessLookupError, PermissionError):
                continue
        return result

    def _terminate_owned(self):
        for sig, duration in ((signal.SIGTERM, 4.0), (signal.SIGKILL, 3.0)):
            for process in reversed(self.processes):
                self._signal_group(process, sig)
            end = time.monotonic() + duration
            while time.monotonic() < end:
                fds = self._owned_pids()
                if not fds:
                    break
                for fd in fds:
                    try:
                        with contextlib.suppress(ProcessLookupError):
                            signal.pidfd_send_signal(fd, sig)
                    finally:
                        os.close(fd)
                time.sleep(0.1)
        remaining = self._owned_pids()
        if remaining:
            self.cleanup_errors.append("owned processes survive termination; private files are retained")
            for fd in remaining:
                os.close(fd)


def child_main(args):
    destination, *command = args
    display = {key: os.environ[key] for key in ("DISPLAY", "XAUTHORITY", "WAYLAND_DISPLAY") if key in os.environ}
    if not re.fullmatch(r":\d+(?:\.\d+)?", display.get("DISPLAY", "")):
        raise SessionError("session: gamescope child has no private local DISPLAY")
    if display["DISPLAY"] == os.environ.get("JC3VR_HOST_DISPLAY"):
        raise SessionError("session: gamescope child inherited the host DISPLAY")
    os.environ.pop("WAYLAND_DISPLAY", None)
    os.environ.pop("XR_RUNTIME_JSON", None)
    display.pop("WAYLAND_DISPLAY", None)
    path = Path(destination)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(display) + "\n")
    temporary.replace(path)
    os.execvp(command[0], command)
