"""Per-thread CPU time of the game process, sampled from /proc while a run records.

The payload's scopes time the work it knows about; the host's per-thread accounting covers the rest,
DXVK's command-stream and submission threads above all, which execute the Direct3D calls the draw
task records. A thread busy close to the whole window is the one that limits the frame. The game
thread is the process's initial thread, whose thread ID is the process ID (its name is the process's,
so it is not renamed). The payload names the draw task's thread `jc3-draw-task` while a run is
active (Wine passes thread descriptions on to the host's thread names); DXVK names its own."""

import os
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

# The thread groups a run reports, by host thread name (at most 15 characters).
GROUPS = {
    "jc3-draw-task": "draw_task",
    "dxvk-cs": "dxvk_cs",
    "dxvk-submit": "dxvk_submit",
    "dxvk-queue": "dxvk_queue",
    "jc3vr-frame-tai": "frame_tail",
    "CpuFragmentThre": "fragment_workers",
}


@dataclass
class Sample:
    at: float
    # Thread ID to (name, CPU seconds).
    threads: dict[int, tuple[str, float]]


def read_threads(pid: int, proc: Path = Path("/proc"), ticks_per_second: float | None = None
                 ) -> dict[int, tuple[str, float]]:
    """Every thread of `pid` with its name and its user plus system CPU time in seconds."""
    rate = ticks_per_second or os.sysconf("SC_CLK_TCK")
    result = {}
    for task in (proc / str(pid) / "task").iterdir():
        try:
            stat = (task / "stat").read_text()
        except (FileNotFoundError, ProcessLookupError):
            continue
        result[int(task.name)] = parse_stat(stat, rate)
    return result


def parse_stat(stat: str, ticks_per_second: float) -> tuple[str, float]:
    """A thread's name and CPU seconds from its /proc stat line. The name sits in parentheses and
    may itself contain spaces and parentheses, so the fields are counted from the last `)`."""
    start, end = stat.index("("), stat.rindex(")")
    name = stat[start + 1:end]
    fields = stat[end + 2:].split()
    # utime and stime are fields 14 and 15 of the line, 12 and 13 after the state.
    return name, (int(fields[11]) + int(fields[12])) / ticks_per_second


@dataclass
class ThreadSampler:
    """Samples the threads every `period` seconds on a background thread until stopped."""
    pid: int
    period: float = 0.25
    samples: list[Sample] = field(default_factory=list)
    _stop: threading.Event = field(default_factory=threading.Event)
    _thread: threading.Thread | None = None

    def __enter__(self):
        self._thread = threading.Thread(target=self._run, name="benchmark-threads", daemon=True)
        self._thread.start()
        return self

    def __exit__(self, *_):
        self._stop.set()
        if self._thread is not None:
            self._thread.join()
        # The acknowledgement arrives as the recording ends, usually before the next periodic
        # sample, so one more closes the window.
        try:
            self.samples.append(Sample(time.time(), read_threads(self.pid)))
        except OSError:
            pass

    def _run(self):
        while not self._stop.is_set():
            try:
                self.samples.append(Sample(time.time(), read_threads(self.pid)))
            except OSError:
                return
            self._stop.wait(self.period)


def window(samples: list[Sample], start: float, end: float, pid: int | None = None) -> dict | None:
    """The threads' CPU time between the last sample at or before `start` and the first at or after
    `end`, as busy fractions of that window, with the named groups summed; the thread whose ID is
    `pid` is the game thread. `None` without samples on both sides."""
    before = [s for s in samples if s.at <= start]
    after = [s for s in samples if s.at >= end]
    if not before or not after:
        return None
    first, last = before[-1], after[0]
    seconds = last.at - first.at
    if seconds <= 0:
        return None
    threads = []
    for tid, (name, cpu) in last.threads.items():
        if tid not in first.threads:
            continue
        busy = (cpu - first.threads[tid][1]) / seconds
        threads.append({"tid": tid, "name": name, "busy": round(busy, 4)})
    threads.sort(key=lambda entry: -entry["busy"])
    groups: dict[str, dict] = {}
    for entry in threads:
        group = "game" if entry["tid"] == pid else GROUPS.get(entry["name"], "other")
        bucket = groups.setdefault(group, {"threads": 0, "busy": 0.0, "max_thread_busy": 0.0})
        bucket["threads"] += 1
        bucket["busy"] = round(bucket["busy"] + entry["busy"], 4)
        bucket["max_thread_busy"] = max(bucket["max_thread_busy"], entry["busy"])
    return {"window_seconds": round(seconds, 3), "groups": groups, "top": threads[:16],
            "process_busy": round(sum(entry["busy"] for entry in threads), 4)}
