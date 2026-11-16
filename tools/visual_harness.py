#!/usr/bin/env python3

import argparse
import contextlib
import dataclasses
import fcntl
import json
import logging
import math
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time
import uuid
import zlib


LOG = logging.getLogger("visual_harness")
ROOT = Path(__file__).resolve().parents[1]
MANIFEST = Path("/run/current-system/sw/share/openxr/1/openxr_monado.json")


class HarnessError(RuntimeError):
    pass


@dataclasses.dataclass
class Options:
    no_build: bool = False
    check: bool = False
    gpu: str | None = None
    xdotool: str | None = None
    timeout: float = 180.0
    startup_timeout: float = 90.0
    build_timeout: float = 900.0
    copy_timeout: float = 180.0
    menu_interval: float = 3.0
    menu_timeout: float = 60.0
    output: Path | None = None
    release: bool = False
    launch_only: bool = False


class VisualHarness:
    def __init__(self, options=None):
        self.options = options or Options()
        self.token = uuid.uuid4().hex
        self.bus_name = f"com.jc3vr.VisualHarness.r{self.token}"
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
        self.cleanup_errors = []
        self.result = {"smoke_succeeded": False, "status": "not_started"}

    def check(self):
        for name in ("timeout", "startup_timeout", "build_timeout", "copy_timeout",
                     "menu_interval", "menu_timeout"):
            value = getattr(self.options, name)
            if not math.isfinite(value) or value <= 0:
                raise HarnessError(f"visual harness: {name} must be finite and positive")
        self.reject_existing()
        if self.options.output is not None:
            output = self.options.output.expanduser().resolve()
            if output.exists() and (not output.is_dir() or any(output.iterdir())):
                raise HarnessError(f"visual harness: output must be an empty directory: {output}")
        for command in ("cp", "gamescope", "steam-run", "setpriv", "dbus-daemon", "nix-shell"):
            if not shutil.which(command):
                raise HarnessError(f"visual harness: missing executable {command}")
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
            raise HarnessError(f"visual harness: cannot read Monado manifest {MANIFEST}: {error}") from error
        if not library.is_absolute():
            library = MANIFEST.resolve().parent / library
        self.monado_service = library.resolve().parent.parent / "bin/monado-service"
        for path in (self.source_prefix / "pfx", self.game, self.proton / "proton",
                     self.sniper, self.client, self.monado_service, library):
            if not path.exists():
                raise HarnessError(f"visual harness: required path does not exist: {path}")
        self.vr_override = self._xrizer()
        self.gpu = self._gpu()
        self.xdotool = self.options.xdotool or shutil.which("xdotool")
        if not self.xdotool:
            candidates = sorted(Path("/nix/store").glob("*-xdotool-*/bin/xdotool"))
            self.xdotool = str(candidates[-1]) if candidates else None
        if not self.xdotool or not os.access(self.xdotool, os.X_OK):
            raise HarnessError("visual harness: xdotool is unavailable; pass --xdotool or install it with nix-build '<nixpkgs>' -A xdotool --no-out-link")
        if self.options.no_build:
            self._check_artifacts()
        LOG.info("checks pass: GPU %s, prefix %s, Monado %s", self.gpu, self.source_prefix, self.monado_service)
        return self

    def start(self):
        if self.env is not None or self._closed:
            raise HarnessError("visual harness: runner cannot be started twice")
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
                raise HarnessError("visual harness: output became nonempty before launch")
            self.output.mkdir(parents=True, exist_ok=True)
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
            LOG.info("private launch started; artifacts: %s", self.output)
            return self
        except BaseException:
            self.close()
            raise

    def run(self, smoke=None):
        try:
            self.start()
            self.result["status"] = "started"
            if smoke is not None or not self.options.launch_only:
                result = (smoke or default_smoke)(self)
                if isinstance(result, dict):
                    self.result.update(result)
                self.result["status"] = "completed" if self.result["smoke_succeeded"] else "not_verified"
                return result
            while True:
                self.assert_running()
                self.pump_menu()
                time.sleep(0.2)
        except BaseException as error:
            self.result.update(status="failed", smoke_succeeded=False, error=str(error))
            raise
        finally:
            self.close()
            self._write_report()

    def _write_report(self):
        if self.output is None:
            return
        self.result["cleanup_errors"] = self.cleanup_errors
        if self.cleanup_errors:
            self.result.update(status="failed", smoke_succeeded=False)
        path = self.output / "result.json"
        temporary = path.with_suffix(".tmp")
        temporary.write_text(json.dumps(self.result, indent=2) + "\n")
        temporary.replace(path)

    def assert_running(self):
        if self.deadline is not None and time.monotonic() >= self.deadline:
            raise HarnessError("visual harness: the run timeout expired")
        for name, process in (("gamescope", self.launch), ("Monado", self.monado), ("D-Bus", self.bus)):
            if process is not None and process.poll() is not None:
                raise HarnessError(f"visual harness: {name} exited with status {process.returncode}; see {self.output}")

    def display_env(self):
        try:
            display = json.loads(self.display_file.read_text())
        except (OSError, ValueError) as error:
            raise HarnessError(f"visual harness: cannot read the private display: {error}") from error
        if not re.fullmatch(r":\d+(?:\.\d+)?", display.get("DISPLAY", "")):
            raise HarnessError("visual harness: the child did not report a local private DISPLAY")
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
        windows = set()
        for selector in ("--class", "--name"):
            result = self._command([self.xdotool, "search", "--onlyvisible", selector, "[Jj]ust.?[Cc]ause"],
                                   "menu-search", 3, env=env, check=False)
            if result.returncode not in (0, 1):
                raise HarnessError(f"visual harness: xdotool search exited with {result.returncode}")
            windows.update(line for line in result.stdout.splitlines() if line.isdecimal())
        if not windows:
            return False
        window = sorted(windows, key=int)[-1]
        self._command([self.xdotool, "windowactivate", "--sync", window,
                       "key", "--clearmodifiers", "Return"], "menu-return", 3, env=env)
        LOG.info("sent Return to the private game window %s", window)
        return True

    def run_in_container(self, command, timeout=20, check=True):
        self.assert_running()
        return self._command(["steam-run", str(self.client), f"--bus-name={self.bus_name}", "--", *map(str, command)],
                             "container-command", timeout, env=self.display_env(), check=check)

    def inject(self, timeout=30, check=True):
        return self.run_in_container([self.proton / "proton", "runinprefix", self.injector,
                                      "--no-capture-host"], timeout=timeout, check=check)

    def close(self):
        if self._closed:
            return
        self._closed = True
        try:
            self._terminate_owned()
        finally:
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
        found = []
        for pid in Path("/proc").iterdir():
            if not pid.name.isdecimal():
                continue
            try:
                comm = (pid / "comm").read_text().strip().lower()
                argv = (pid / "cmdline").read_bytes().split(b"\0")
            except (FileNotFoundError, ProcessLookupError):
                continue
            except PermissionError:
                if pid.stat().st_uid == os.getuid():
                    raise HarnessError(f"visual harness: cannot inspect process {pid.name}")
                continue
            names = {Path(os.fsdecode(arg).replace("\\", "/")).name.lower() for arg in argv if arg}
            if comm.startswith(("justcause3", "monado-service", "wivrn-server")) or names.intersection({"justcause3.exe", "monado-service", "wivrn-server"}):
                found.append(f"{pid.name} ({comm})")
        if found:
            raise HarnessError(f"visual harness: refusing to coexist with a game or Monado service: {', '.join(found)}")

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
                raise HarnessError("visual harness: xrizer is unavailable; set VR_OVERRIDE") from error
        if not path.is_dir():
            raise HarnessError(f"visual harness: xrizer runtime does not exist: {path}")
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
            raise HarnessError("visual harness: select a GPU using --gpu vendor:device")
        selected = selected.lower()
        if selected not in gpus:
            raise HarnessError(f"visual harness: requested GPU {selected} is not in sysfs: {gpus}")
        if gpus.count(selected) > 1:
            raise HarnessError(f"visual harness: GPU ID {selected} is ambiguous")
        return selected

    def _check_artifacts(self):
        for path in (self.boot_injector, self.boot_injector.with_name("jc3boot_payload.dll"),
                     self.injector, self.injector.with_name("jc3vr_payload.dll")):
            if not path.is_file():
                raise HarnessError(f"visual harness: missing build artifact: {path}")

    def _acquire_lock(self):
        path = Path(tempfile.gettempdir()) / f"jc3vr-visual-harness-{os.getuid()}.lock"
        fd = os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
        self._lock = os.fdopen(fd, "w")
        try:
            fcntl.flock(self._lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            self._lock.close()
            self._lock = None
            raise HarnessError("visual harness: another private runner holds the launch lock") from error

    def _seed_settings(self):
        (self.config_dir / "settings.toml").write_text(
            "version = 1\n[vr]\nmirror = false\nresolution_scale = 0.5\n"
            "[controllers]\nenabled = false\n[foveation]\nenabled = false\n"
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
            "SIMULATED_ENABLE": "1", "XRT_COMPOSITOR_NULL": "1", "XRT_COMPOSITOR_NULL_FPS": "30",
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
                sys.executable, str(Path(__file__).resolve()), "_child", str(self.display_file), *chain]

    def _write_state(self):
        state = {"bus_name": self.bus_name, "gpu": self.gpu, "runtime_dir": str(self.runtime),
                 "prefix": str(self.prefix), "config_dir": str(self.config_dir),
                 "sessions_dir": str(self.sessions_dir), "display_file": str(self.display_file),
                 "token": self.token}
        (self.output / "runner.json").write_text(json.dumps(state, indent=2) + "\n")

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
            raise HarnessError(f"visual harness: {name}: the run timeout expired")
        log_path = self.output / f"{name}.log"
        offset = log_path.stat().st_size if log_path.exists() else 0
        process = self._spawn(command, name, env=env, cwd=cwd)
        try:
            code = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired as error:
            self._signal_group(process, signal.SIGKILL)
            process.wait(timeout=3)
            raise HarnessError(f"visual harness: {name} exceeded {timeout:.1f}s") from error
        with log_path.open("rb") as stream:
            stream.seek(offset)
            text = stream.read().decode(errors="replace")
        result = subprocess.CompletedProcess(command, code, text, "")
        if check and code:
            raise HarnessError(f"visual harness: {name} exited with status {code}; see {self.output / (name + '.log')}")
        return result

    def _wait_for(self, predicate, description, timeout):
        end = min(self.deadline, time.monotonic() + timeout)
        while time.monotonic() < end:
            self.assert_running()
            if predicate():
                return
            time.sleep(0.1)
        raise HarnessError(f"visual harness: timed out waiting for {description}")

    def _wait_socket(self, path, timeout):
        def ready():
            with socket.socket(socket.AF_UNIX) as client:
                client.settimeout(0.1)
                try:
                    client.connect(str(path))
                    return True
                except (FileNotFoundError, ConnectionRefusedError, socket.timeout):
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


def default_smoke(runner):
    end = min(runner.deadline, time.monotonic() + runner.options.startup_timeout)
    while True:
        runner.assert_running()
        outcome = runner.inject(timeout=min(30, max(0.1, end - time.monotonic())), check=False)
        if outcome.returncode == 0:
            break
        text = outcome.stdout.lower()
        retryable = "no justcause3.exe process found" in text or any(
            phrase in text for phrase in ("serviceunknown", "namehasnoowner", "no owner for name",
                                          "unable to connect to any of the specified bus names")
        )
        if not retryable:
            raise HarnessError(f"visual harness: injection failed and is not safe to retry: {outcome.stdout[-2000:]}")
        if time.monotonic() >= end:
            raise HarnessError("visual harness: timed out waiting for the game before injection")
        runner.pump_menu()
        time.sleep(1)
    captures = []
    attempt = 0
    while len(captures) < 2:
        runner.assert_running()
        attempt += 1
        capture_id = f"smoke_{attempt}"
        request = runner.config_dir / "test_request.toml"
        if request.exists():
            raise HarnessError("visual harness: a previous screenshot request is still outstanding")
        temporary = request.with_suffix(".tmp")
        temporary.write_text(f'command = "screenshot"\nid = "{capture_id}"\n')
        temporary.replace(request)
        acknowledgement = runner.config_dir / "captures" / f"{capture_id}.json"
        end = min(runner.deadline, time.monotonic() + 35)
        while not acknowledgement.exists():
            runner.assert_running()
            runner.pump_menu()
            if time.monotonic() >= end:
                raise HarnessError(f"visual harness: timed out waiting for screenshot acknowledgement {capture_id}")
            time.sleep(0.2)
        result = read_object(acknowledgement)
        if result.get("id") != capture_id:
            raise HarnessError("visual harness: screenshot acknowledgement ID mismatch")
        if result.get("gameplay_ready") is True:
            runner.mark_gameplay_ready()
        if result.get("state") == "failed":
            error = str(result.get("error", "unknown capture failure"))
            if result.get("gameplay_ready") is False and (
                    "not_ready" in error or "not ready" in error or "waiting for gameplay" in error):
                runner.pump_menu()
                time.sleep(0.5)
                continue
            raise HarnessError(f"visual harness: screenshot {capture_id}: {error}")
        if result.get("state") != "completed":
            raise HarnessError(f"visual harness: screenshot {capture_id} has no completed acknowledgement")
        captures.append(validate_capture(runner.config_dir / "captures", capture_id, result))
    first, second = captures
    if second["frame_ordinal"] <= first["frame_ordinal"] or second["predicted_display_time_ns"] == first["predicted_display_time_ns"]:
        raise HarnessError("visual harness: sequential captures do not identify distinct rendered frames")
    return {"smoke_succeeded": True, "captures": captures}


def read_object(path):
    try:
        value = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise HarnessError(f"visual harness: invalid JSON artifact {path}: {error}") from error
    if not isinstance(value, dict):
        raise HarnessError(f"visual harness: expected a JSON object in {path}")
    return value


def validate_capture(folder, capture_id, result):
    metadata = read_object(folder / f"{capture_id}.capture.json")
    keys = ("id", "frame_ordinal", "predicted_display_time_ns", "source", "width", "height",
            "eye_width", "eye_height", "fresh_eyes", "gameplay_ready", "xr_session_running")
    for key in keys:
        if key not in result or metadata.get(key) != result[key]:
            raise HarnessError(f"visual harness: screenshot metadata disagrees on {key}")
    if result["id"] != capture_id or result["fresh_eyes"] != [True, True] or result["gameplay_ready"] is not True or result["xr_session_running"] is not True:
        raise HarnessError("visual harness: screenshot is not a fresh gameplay XR pair")
    if result["source"] not in ("eye_textures", "collapsed_back_buffer"):
        raise HarnessError("visual harness: screenshot has an unknown source")
    for key in ("frame_ordinal", "width", "height", "eye_width", "eye_height"):
        if type(result[key]) is not int or result[key] <= 0:
            raise HarnessError(f"visual harness: invalid screenshot {key}")
    if type(result["predicted_display_time_ns"]) is not int:
        raise HarnessError("visual harness: invalid predicted display time")
    width, height = validate_png(folder / f"{capture_id}.capture.png")
    if (width, height) != (result["width"], result["height"]) or width != result["eye_width"] * 2 or height != result["eye_height"]:
        raise HarnessError("visual harness: PNG dimensions disagree with the stereo metadata")
    return {key: result[key] for key in keys}


def validate_png(path):
    data = path.read_bytes()
    if len(data) > 128 * 1024 * 1024 or data[:8] != b"\x89PNG\r\n\x1a\n":
        raise HarnessError(f"visual harness: invalid PNG signature or size: {path}")
    offset = 8
    header = None
    compressed = bytearray()
    ended = False
    idat_closed = False
    while offset < len(data):
        if offset + 12 > len(data):
            raise HarnessError("visual harness: truncated PNG chunk")
        size, kind = struct.unpack_from(">I4s", data, offset)
        end = offset + 12 + size
        if end > len(data):
            raise HarnessError("visual harness: truncated PNG payload")
        payload = data[offset + 8:end - 4]
        crc = struct.unpack_from(">I", data, end - 4)[0]
        if zlib.crc32(kind + payload) & 0xffffffff != crc:
            raise HarnessError("visual harness: PNG chunk CRC mismatch")
        if header is None and kind != b"IHDR":
            raise HarnessError("visual harness: PNG does not begin with IHDR")
        if kind == b"IHDR":
            if header is not None or size != 13:
                raise HarnessError("visual harness: invalid PNG IHDR")
            header = struct.unpack(">IIBBBBB", payload)
        elif kind == b"IDAT":
            if idat_closed:
                raise HarnessError("visual harness: nonconsecutive PNG IDAT chunks")
            compressed.extend(payload)
        elif kind == b"IEND":
            if size or end != len(data):
                raise HarnessError("visual harness: invalid PNG IEND or trailing bytes")
            ended = True
            break
        else:
            if compressed:
                idat_closed = True
            if kind[:1].isupper():
                raise HarnessError(f"visual harness: unsupported critical PNG chunk {kind!r}")
        offset = end
    if header is None or not ended or not compressed:
        raise HarnessError("visual harness: PNG is incomplete")
    width, height, depth, color, compression, filtering, interlace = header
    if not (0 < width <= 16384 and 0 < height <= 16384) or depth != 8 or color not in (2, 6) or (compression, filtering, interlace) != (0, 0, 0):
        raise HarnessError("visual harness: unsupported PNG layout")
    stride = width * (3 if color == 2 else 4) + 1
    expected = stride * height
    if expected > 256 * 1024 * 1024:
        raise HarnessError("visual harness: PNG decoded image is too large")
    decoder = zlib.decompressobj()
    try:
        pixels = decoder.decompress(compressed, expected + 1)
    except zlib.error as error:
        raise HarnessError(f"visual harness: corrupt PNG deflate stream: {error}") from error
    if len(pixels) != expected or not decoder.eof or decoder.unused_data or decoder.unconsumed_tail:
        raise HarnessError("visual harness: PNG decoded length or zlib stream is invalid")
    if any(pixels[row * stride] > 4 for row in range(height)):
        raise HarnessError("visual harness: invalid PNG row filter")
    return width, height


def main(argv=None):
    parser = argparse.ArgumentParser(description="Run Just Cause 3 in a private headless VR session.")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--launch-only", action="store_true", help="launch without claiming smoke success")
    parser.add_argument("--gpu", help="Vulkan PCI vendor:device, e.g. 10de:2b85")
    parser.add_argument("--xdotool")
    parser.add_argument("--output", type=Path)
    for name, default in (("timeout", 180), ("startup-timeout", 90), ("build-timeout", 900),
                          ("copy-timeout", 180), ("menu-interval", 3), ("menu-timeout", 60)):
        parser.add_argument(f"--{name}", type=float, default=default)
    args = parser.parse_args(argv)
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s", stream=sys.stderr)
    def interrupted(signum, _frame):
        raise KeyboardInterrupt(f"signal {signum}")
    for sig in (signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, interrupted)
    runner = VisualHarness(Options(**vars(args)))
    try:
        if args.check:
            runner.check()
        else:
            runner.run()
            if not runner.result["smoke_succeeded"]:
                return 1
        return 0
    except KeyboardInterrupt:
        LOG.error("visual harness: interrupted")
        return 130
    except (HarnessError, OSError, ValueError) as error:
        LOG.error("%s", error)
        return 1
    finally:
        runner.close()


def child_main(args):
    destination, *command = args
    display = {key: os.environ[key] for key in ("DISPLAY", "XAUTHORITY", "WAYLAND_DISPLAY") if key in os.environ}
    if not re.fullmatch(r":\d+(?:\.\d+)?", display.get("DISPLAY", "")):
        raise HarnessError("visual harness: gamescope child has no private local DISPLAY")
    if display["DISPLAY"] == os.environ.get("JC3VR_HOST_DISPLAY"):
        raise HarnessError("visual harness: gamescope child inherited the host DISPLAY")
    os.environ.pop("WAYLAND_DISPLAY", None)
    os.environ.pop("XR_RUNTIME_JSON", None)
    display.pop("WAYLAND_DISPLAY", None)
    path = Path(destination)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(display) + "\n")
    temporary.replace(path)
    os.execvp(command[0], command)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "_child":
        child_main(sys.argv[2:])
    else:
        sys.exit(main())
