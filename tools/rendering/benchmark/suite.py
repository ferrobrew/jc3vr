"""The benchmark's scene list: where to stand (an experiment config's stage and environment, or a
stage given here), how the held camera moves, and the single-pass variants to measure."""

import tomllib
from dataclasses import dataclass, field
from pathlib import Path

from tools.rendering.experiment import ExperimentConfig
from tools.rendering.protocol import ProtocolError, StageRequest
from tools.rendering.protocol.benchmark import BenchmarkMode, CameraPath

EXPERIMENTS = Path(__file__).resolve().parents[1] / "experiments"
DEFAULT_SUITE = Path(__file__).resolve().parents[1] / "suites/benchmark.toml"


class BenchmarkError(RuntimeError):
    pass


@dataclass(frozen=True)
class BenchmarkScene:
    name: str
    config: Path
    stage: StageRequest
    environment: object | None
    camera: CameraPath = field(default_factory=CameraPath)


@dataclass(frozen=True)
class Variant:
    """A mode with some switches forced, to attribute a cost to what they control: single-pass
    switches (single-pass only), the frame ordering (`vr.overlap_render_update`), the placement
    on the largest cache's cores (`vr.large_cache_cores`), FSR, its hybrid decode, and its
    sub-pixel jitter, or the foveation."""
    name: str
    switches: tuple[tuple[str, bool], ...]
    base: BenchmarkMode = BenchmarkMode.SINGLE_PASS
    overlap_render_update: bool | None = None
    fsr: bool | None = None
    hybrid_velocity: bool | None = None
    large_cache_cores: bool | None = None
    jitter: bool | None = None
    foveation: bool | None = None


@dataclass(frozen=True)
class Suite:
    scenes: tuple[BenchmarkScene, ...]
    variants: tuple[Variant, ...]


def load_suite(path: Path) -> Suite:
    try:
        value = tomllib.loads(path.read_text())
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise BenchmarkError(f"benchmark suite {path}: {error}") from error
    if set(value) - {"scene", "variant"} or not isinstance(value.get("scene"), list):
        raise BenchmarkError(f"benchmark suite {path}: expected [[scene]] tables and optionally "
                             "[[variant]] tables")
    scenes = []
    for index, table in enumerate(value["scene"]):
        context = f"benchmark suite {path}: scene {index}"
        if not isinstance(table, dict) or set(table) - {"name", "config", "stage", "camera"} or not {
                "name", "config"} <= set(table):
            raise BenchmarkError(f"{context}: expected name and config, and optionally stage and "
                                 "camera")
        config_path = path.parent / table["config"]
        if not config_path.is_file():
            config_path = EXPERIMENTS / table["config"]
        try:
            config = ExperimentConfig.load(config_path)
            stage = (StageRequest.from_toml("stage", table["stage"], f"{context}.stage")
                     if "stage" in table else config.scene.stage)
            camera = CameraPath(**table.get("camera", {}))
        except (ProtocolError, TypeError, ValueError, OSError) as error:
            raise BenchmarkError(f"{context}: {error}") from error
        if stage is None:
            raise BenchmarkError(f"{context}: neither the config nor the suite gives a stage")
        if table["name"] in [scene.name for scene in scenes]:
            raise BenchmarkError(f"{context}: the name {table['name']!r} is used twice")
        scenes.append(BenchmarkScene(table["name"], config_path, stage, config.scene.environment,
                                     camera))
    variants = []
    for index, table in enumerate(value.get("variant", [])):
        context = f"benchmark suite {path}: variant {index}"
        if not isinstance(table, dict):
            raise BenchmarkError(f"{context}: expected a table")
        switches = table.get("switches", {})
        flags = {name: table.get(name) for name in ("overlap_render_update", "fsr", "hybrid_velocity",
                                                "large_cache_cores", "jitter", "foveation")}
        valid = (set(table) - {"name", "base", "switches", *flags} == set()
                 and isinstance(table.get("name"), str)
                 and table.get("base", "single_pass") in {m.value for m in BenchmarkMode}
                 and isinstance(switches, dict)
                 and all(isinstance(v, bool) for v in switches.values())
                 and all(flag is None or isinstance(flag, bool) for flag in flags.values())
                 and (switches or any(flag is not None for flag in flags.values())))
        if not valid:
            raise BenchmarkError(f"{context}: expected a name, optionally a base mode, and a "
                                 "switches table of booleans or one of overlap_render_update, "
                                 "fsr, hybrid_velocity, jitter, foveation, and large_cache_cores")
        base = BenchmarkMode(table.get("base", "single_pass"))
        if switches and base != BenchmarkMode.SINGLE_PASS:
            raise BenchmarkError(f"{context}: single-pass switches need the single_pass base")
        variants.append(Variant(table["name"], tuple(sorted(switches.items())), base,
                                flags["overlap_render_update"], flags["fsr"], flags["hybrid_velocity"],
                                flags["large_cache_cores"], flags["jitter"], flags["foveation"]))
    return Suite(tuple(scenes), tuple(variants))
