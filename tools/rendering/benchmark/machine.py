"""The machine a benchmark ran on, for the results' header."""

import platform
import subprocess
from pathlib import Path


def machine_info(proc: Path = Path("/proc")) -> dict:
    return {
        "cpu": cpu_model((proc / "cpuinfo").read_text()) if (proc / "cpuinfo").exists() else None,
        "cpu_threads": _cpu_threads(proc),
        "gpu": _gpu(),
        "kernel": platform.release(),
        "translation": "Proton (Wine) with DXVK; Direct3D 11 on Vulkan",
        "display": "gamescope headless",
        "xr_runtime": "Monado with the simulated headset and the null compositor (no reprojection)",
    }


def cpu_model(cpuinfo: str) -> str | None:
    for line in cpuinfo.splitlines():
        key, _, value = line.partition(":")
        if key.strip() == "model name":
            return value.strip()
    return None


def _cpu_threads(proc: Path) -> int | None:
    try:
        return sum(1 for line in (proc / "cpuinfo").read_text().splitlines()
                   if line.startswith("processor"))
    except OSError:
        return None


def _gpu() -> dict | None:
    try:
        output = subprocess.run(
            ["nvidia-smi", "--query-gpu=name,memory.total,driver_version", "--format=csv,noheader"],
            capture_output=True, text=True, timeout=20, check=True).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return None
    name, memory, driver = (part.strip() for part in output.splitlines()[0].split(","))
    return {"name": name, "memory": memory, "driver": driver}
