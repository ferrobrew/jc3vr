"""Command-line entry point for the rendering tooling."""

import importlib
import sys

USAGE = """usage: python -m tools.rendering <command> [options]

commands:
  run        run a configured rendering comparison
  launch     run the visual capture harness
  session    launch and serve a private session until interrupted
  compare    compare frozen-camera captures
  stages     write native stage diagnostics for a suite manifest
  constants  write native constants diagnostics for a suite manifest
  viewer     serve the local capture preview
"""

COMMANDS = {
    "run": "tools.rendering.experiment",
    "launch": "tools.rendering.capture.harness",
    "session": "tools.rendering.session.cli",
    "compare": "tools.rendering.comparison.images",
    "stages": "tools.rendering.comparison.stages",
    "constants": "tools.rendering.comparison.constants",
    "viewer": "tools.rendering.viewer.server",
}


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if not argv or argv[0] in ("-h", "--help"):
        print(USAGE, end="")
        return 0
    command, *rest = argv
    target = COMMANDS.get(command)
    if target is None:
        print(f"tools.rendering: unknown command {command!r}", file=sys.stderr)
        print(USAGE, end="", file=sys.stderr)
        return 2
    module = importlib.import_module(target)
    if command == "launch" and rest and rest[0] == "_child":
        importlib.import_module("tools.rendering.session.launch").child_main(rest[1:])
        return 0
    return module.main(rest)


if __name__ == "__main__":
    sys.exit(main())
