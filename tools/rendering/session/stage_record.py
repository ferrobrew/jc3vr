"""The record of the last stage a session staged, so a run at the same location and environment can
ask the payload to reuse it instead of teleporting and settling again. The payload still checks
that the player stands at the location (`reuse_within_m`), so a record that no longer holds costs
only a fresh stage."""

import dataclasses
import json
from pathlib import Path

# The record's file in the session's configuration directory.
RECORD = "last-stage.json"
# How close the player must still stand to a recorded stage's location for it to be reused.
REUSE_WITHIN_M = 5.0


def stage_key(stage, environment) -> str:
    """What makes two stages the same scene: the location, the spawns, and the environment."""
    return json.dumps({
        "location": None if stage.location is None else dataclasses.asdict(stage.location),
        "spawns": [dataclasses.asdict(spawn) for spawn in stage.spawns],
        "environment": None if environment is None else dataclasses.asdict(environment),
    }, sort_keys=True, default=str)


def reuse_distance(config_dir: Path, key: str) -> float | None:
    """The reuse distance to send with a stage of `key`, or `None` when the session last staged
    something else (or nothing)."""
    try:
        recorded = json.loads((Path(config_dir) / RECORD).read_text())
    except (OSError, ValueError):
        return None
    return REUSE_WITHIN_M if isinstance(recorded, dict) and recorded.get("key") == key else None


def record(config_dir: Path, key: str) -> None:
    path = Path(config_dir) / RECORD
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps({"key": key}) + "\n")
    temporary.replace(path)
