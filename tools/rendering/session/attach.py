"""Attach to a running private rendering session."""

import fcntl
import json
import math
import os
import time
from dataclasses import dataclass
from pathlib import Path

from tools.rendering.protocol import Request, valid_request_id
from tools.rendering.session.channel import RequestChannel
from tools.rendering.session.errors import SessionError
from tools.rendering.session.processes import processes


@dataclass(frozen=True)
class SessionState:
    token: str
    config_dir: Path

    @classmethod
    def load(cls, path) -> "SessionState":
        try:
            value = json.loads(Path(path).read_text())
        except (OSError, ValueError) as error:
            raise SessionError(f"session: could not read runner state {path}: {error}") from error
        if not isinstance(value, dict):
            raise SessionError("session: runner state is not a JSON object")
        token = _string(value.get("token"), "runner.token")
        config_dir = Path(_string(value.get("config_dir"), "runner.config_dir"))
        if not valid_request_id(token) or not config_dir.is_absolute() or not config_dir.is_dir():
            raise SessionError("session: runner state has an invalid token or config directory")
        return cls(token, config_dir)


class ExclusiveLock:
    def __init__(self, handle):
        self.handle = handle

    @classmethod
    def acquire(cls, config_dir: Path) -> "ExclusiveLock":
        handle = (config_dir / "session.lock").open("a+")
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            handle.close()
            raise SessionError(f"session: another attached tool holds {config_dir}") from error
        return cls(handle)

    def release(self):
        fcntl.flock(self.handle, fcntl.LOCK_UN)
        self.handle.close()


@dataclass
class AttachedSession(RequestChannel):
    config_dir: Path
    deadline: float
    token: str
    pid: int
    pending_request: Request | None = None
    lock: ExclusiveLock | None = None

    @classmethod
    def attach(cls, path, timeout: float) -> "AttachedSession":
        if not math.isfinite(timeout) or timeout <= 0:
            raise SessionError("session: attach timeout must be finite and positive")
        state = SessionState.load(path)
        pid = owning_process(state)
        lock = ExclusiveLock.acquire(state.config_dir)
        return cls(state.config_dir, time.monotonic() + timeout, state.token, pid, lock=lock)

    def assert_running(self):
        if time.monotonic() >= self.deadline:
            raise SessionError("session: the attached run deadline expired")
        try:
            fields = set(Path(f"/proc/{self.pid}/environ").read_bytes().split(b"\0"))
            os.kill(self.pid, 0)
        except (FileNotFoundError, ProcessLookupError, PermissionError) as error:
            raise SessionError(f"session: the attached game process {self.pid} is gone") from error
        if f"JC3VR_HARNESS_TOKEN={self.token}".encode() not in fields:
            raise SessionError("session: the attached game process no longer owns this session")

    def pump_menu(self):
        return False

    def close(self):
        if self.lock is not None:
            self.lock.release()
            self.lock = None

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def owning_process(state: SessionState, proc: Path = Path("/proc")) -> int:
    expected = {f"JC3VR_HARNESS_TOKEN={state.token}".encode(),
                f"JC3VR_CONFIG_DIR={state.config_dir}".encode()}
    matches = []
    for process in processes(proc):
        if not process.comm.startswith("justcause3"):
            continue
        if not process.argv or Path(process.argv[0].replace("\\", "/")).name.lower() != "justcause3.exe":
            continue
        try:
            fields = set((proc / str(process.pid) / "environ").read_bytes().split(b"\0"))
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            continue
        if expected <= fields:
            matches.append(process.pid)
    if len(matches) != 1:
        raise SessionError(
            f"session: expected exactly one game process owning {state.config_dir}, "
            f"found {len(matches)}"
        )
    return matches[0]


def _string(value, context):
    if not isinstance(value, str) or not value:
        raise SessionError(f"session: {context} must be a nonempty string")
    return value
