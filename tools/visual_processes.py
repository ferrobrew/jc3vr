"""Process inspection shared by the visual harness."""

import dataclasses
import os
from pathlib import Path


class ProcessError(RuntimeError):
    pass


@dataclasses.dataclass(frozen=True)
class Process:
    pid: int
    comm: str
    argv: tuple[str, ...]
    uid: int

    @property
    def names(self):
        return {Path(arg.replace("\\", "/")).name.lower() for arg in self.argv if arg}

    @property
    def conflicts(self):
        return self.comm.startswith(("justcause3", "monado-service", "wivrn-server")) or not self.names.isdisjoint({"justcause3.exe", "monado-service", "wivrn-server"})


def processes(proc=Path("/proc"), uid=None):
    result = []
    for entry in proc.iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            entry_uid = entry.stat().st_uid
        except (FileNotFoundError, ProcessLookupError):
            continue
        except PermissionError:
            continue
        if uid is not None and entry_uid != uid:
            continue
        try:
            comm = (entry / "comm").read_text().strip().lower()
            argv = tuple(os.fsdecode(arg) for arg in (entry / "cmdline").read_bytes().split(b"\0") if arg)
        except (FileNotFoundError, ProcessLookupError):
            continue
        except PermissionError as error:
            if entry_uid != os.getuid():
                continue
            raise ProcessError(f"visual harness: cannot inspect process {entry.name}") from error
        result.append(Process(int(entry.name), comm, argv, entry_uid))
    return tuple(result)


def steam_client(proc=Path("/proc"), uid=None):
    uid = os.getuid() if uid is None else uid
    return next((process for process in processes(proc, uid) if process.comm == "steam"), None)
