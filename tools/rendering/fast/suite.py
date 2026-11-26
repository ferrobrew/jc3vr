"""Scene lists for the fast comparison: experiment configs, optionally under a headset rig or with a
stage supplied for a fixed-camera config that has none."""

import dataclasses
import tomllib
from dataclasses import dataclass
from pathlib import Path

from tools.rendering.experiment import ExperimentConfig
from tools.rendering.fast.plan import FastError
from tools.rendering.protocol import HEADSET_RIGS, ProtocolError, StageRequest, VisualOverrides

EXPERIMENTS = Path(__file__).resolve().parents[1] / "experiments"


@dataclass(frozen=True)
class Scene:
    name: str
    path: Path
    config: ExperimentConfig


def load_config(path: Path, name: str | None = None) -> Scene:
    config = ExperimentConfig.load(path)
    return Scene(name or path.stem, path, config)


def load_suite(path: Path) -> list[Scene]:
    """Read a suite file: `[[scene]]` tables with a `config` (relative to the suite file, then to
    `tools/rendering/experiments`), and optionally a `name`, a `headset` rig, and a `stage` table
    for a config without one."""
    try:
        value = tomllib.loads(path.read_text())
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise FastError(f"suite {path}: {error}") from error
    if set(value) != {"scene"} or not isinstance(value["scene"], list) or not value["scene"]:
        raise FastError(f"suite {path}: expected one or more [[scene]] tables and nothing else")
    scenes = []
    for index, table in enumerate(value["scene"]):
        context = f"suite {path}: scene {index}"
        if not isinstance(table, dict) or "config" not in table or set(table) - {
                "config", "name", "headset", "stage"}:
            raise FastError(f"{context}: expected config, and optionally name, headset, and stage")
        config_path = path.parent / table["config"]
        if not config_path.is_file():
            config_path = EXPERIMENTS / table["config"]
        scene = load_config(config_path, table.get("name"))
        config = scene.config
        headset = table.get("headset")
        if headset is not None:
            if headset not in HEADSET_RIGS:
                raise FastError(f"{context}: unknown headset {headset!r}")
            overrides = (VisualOverrides(headset=headset) if config.visual_overrides is None
                         else dataclasses.replace(config.visual_overrides, headset=headset))
            config = dataclasses.replace(config, visual_overrides=overrides)
        if "stage" in table:
            if config.scene.stage is not None:
                raise FastError(f"{context}: the config already has a stage")
            try:
                stage = StageRequest.from_toml("stage", table["stage"], f"{context}.stage")
            except ProtocolError as error:
                raise FastError(str(error)) from error
            config = dataclasses.replace(config, scene=dataclasses.replace(config.scene, stage=stage))
        names = [existing.name for existing in scenes]
        if scene.name in names:
            raise FastError(f"{context}: the name {scene.name!r} is used twice")
        scenes.append(Scene(scene.name, config_path, config))
    return scenes
