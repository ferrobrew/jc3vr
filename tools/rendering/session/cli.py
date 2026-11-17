"""Command-line entry point for a keep-alive private rendering session."""

from tools.rendering.capture.harness import main as launch_main


def main(argv=None):
    return launch_main(argv, keep_alive=True)
