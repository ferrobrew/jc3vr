import hashlib
import json
import os
import re
import stat
import tempfile
from contextlib import contextmanager
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path

if os.name == "nt":
    import msvcrt
else:
    import fcntl


class ViewerError(RuntimeError):
    pass


class RunId(str):
    def __new__(cls, value):
        if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value):
            raise ViewerError("visual viewer: invalid run ID")
        return str.__new__(cls, value)

    @classmethod
    def for_output(cls, output):
        return cls(hashlib.sha256(str(output).encode("utf-8")).hexdigest())


@dataclass(frozen=True)
class RunPointer:
    output: str
    version: int = 1

    def __post_init__(self):
        if type(self.version) is not int or self.version != 1:
            raise ViewerError("visual viewer: unsupported run pointer version")
        if not isinstance(self.output, str) or not self.output or "\0" in self.output or not Path(self.output).is_absolute():
            raise ViewerError("visual viewer: the run pointer requires an absolute output path")

    @classmethod
    def from_wire(cls, value):
        if not isinstance(value, dict) or set(value) != {"version", "output"}:
            raise ViewerError("visual viewer: invalid run pointer fields")
        return cls(output=value["output"], version=value["version"])

    def to_wire(self):
        return asdict(self)


@dataclass(frozen=True)
class RunEntry:
    run_id: RunId
    output: str
    timestamp: str

    def __post_init__(self):
        RunPointer(self.output)
        if self.run_id != RunId.for_output(self.output):
            raise ViewerError("visual viewer: the run ID disagrees with its output")
        if not isinstance(self.timestamp, str):
            raise ViewerError("visual viewer: invalid run timestamp")
        try:
            stamp = datetime.fromisoformat(self.timestamp)
        except ValueError as error:
            raise ViewerError("visual viewer: invalid run timestamp") from error
        if stamp.tzinfo is None or stamp.utcoffset().total_seconds() != 0:
            raise ViewerError("visual viewer: the run timestamp must be UTC")
        object.__setattr__(self, "timestamp", stamp.isoformat(timespec="microseconds"))

    @classmethod
    def from_wire(cls, value):
        if not isinstance(value, dict) or set(value) != {"run_id", "output", "timestamp"}:
            raise ViewerError("visual viewer: invalid history entry")
        return cls(RunId(value["run_id"]), value["output"], value["timestamp"])


def history_path(pointer):
    return pointer.with_name(pointer.stem + ".history.json")


def read_history(pointer):
    try:
        wire = read_json(history_path(pointer))
    except FileNotFoundError:
        return []
    if set(wire) != {"version", "runs"} or type(wire["version"]) is not int or wire["version"] != 1:
        raise ViewerError("visual viewer: unsupported history index")
    if not isinstance(wire["runs"], list):
        raise ViewerError("visual viewer: invalid history runs")
    entries = [RunEntry.from_wire(value) for value in wire["runs"]]
    if len({entry.run_id for entry in entries}) != len(entries):
        raise ViewerError("visual viewer: duplicate history run ID")
    return sorted(entries, key=lambda entry: (entry.timestamp, entry.run_id), reverse=True)


def publish_run(output: Path, pointer: Path, *, started_at: datetime | str | None = None) -> RunPointer:
    value = RunPointer(str(Path(output).expanduser().resolve()))
    destination = Path(pointer).expanduser().absolute()
    destination.parent.mkdir(parents=True, exist_ok=True)
    with _publication_lock(destination):
        entries = read_history(destination)
        run_id = RunId.for_output(value.output)
        previous = next((entry for entry in entries if entry.run_id == run_id), None)
        stamp = started_at.isoformat() if isinstance(started_at, datetime) else started_at
        if stamp is None:
            stamp = previous.timestamp if previous else datetime.now(UTC).isoformat()
        entry = RunEntry(run_id, value.output, stamp)
        entries = [entry for entry in entries if entry.run_id != run_id]
        entries.insert(0, entry)
        _atomic_json(history_path(destination), {"version": 1, "runs": [asdict(entry) for entry in entries]})
        _atomic_json(destination, value.to_wire())
    return value


def read_bounded(path, limit):
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
            raise ViewerError("visual viewer: the artifact is not a bounded regular file")
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ViewerError("visual viewer: the artifact exceeds the size limit")
    return data


def read_json(path):
    wire = json.loads(read_bounded(path, MAX_JSON_BYTES))
    if not isinstance(wire, dict):
        raise ViewerError("visual viewer: expected a JSON object")
    return wire


MAX_JSON_BYTES = 256 * 1024


def _atomic_json(destination, wire):
    data = json.dumps(wire, allow_nan=False).encode("utf-8")
    if len(data) > MAX_JSON_BYTES:
        raise ViewerError("visual viewer: the history index exceeds the size limit")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="wb", prefix=f".{destination.name}.", dir=destination.parent,
                                         delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, destination)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


@contextmanager
def _publication_lock(pointer):
    with open(pointer.with_name(pointer.stem + ".history.lock"), "a+b") as stream:
        if os.name == "nt":
            stream.seek(0)
            stream.write(b"\0")
            stream.flush()
            stream.seek(0)
            msvcrt.locking(stream.fileno(), msvcrt.LK_LOCK, 1)
        else:
            fcntl.flock(stream.fileno(), fcntl.LOCK_EX)
        try:
            yield
        finally:
            if os.name == "nt":
                stream.seek(0)
                msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(stream.fileno(), fcntl.LOCK_UN)
