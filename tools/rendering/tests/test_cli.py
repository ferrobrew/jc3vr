"""Command-line dispatch tests for the rendering tooling."""

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
COMMANDS = ("run", "launch", "session", "compare", "stages", "constants", "viewer")


class CliTests(unittest.TestCase):
    def invoke(self, *args, env=None):
        return subprocess.run(
            [sys.executable, "-m", "tools.rendering", *args],
            cwd=ROOT,
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )

    def child_env(self, **overrides):
        env = {key: value for key, value in os.environ.items()
               if key not in ("WAYLAND_DISPLAY", "XR_RUNTIME_JSON", "JC3VR_HOST_DISPLAY")}
        env.update(overrides)
        return env

    def test_help_lists_every_command(self):
        result = self.invoke("--help")
        self.assertEqual(result.returncode, 0, result.stderr)
        for command in COMMANDS:
            self.assertIn(command, result.stdout)

    def test_every_command_prints_its_own_help(self):
        for command in COMMANDS:
            with self.subTest(command=command):
                result = self.invoke(command, "--help")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("usage:", result.stdout)

    def test_session_rejects_execution_modes(self):
        result = self.invoke("session", "--suite")
        self.assertEqual(result.returncode, 2)
        self.assertIn("mutually exclusive", result.stderr)

    def test_unknown_command_reports_the_usage(self):
        result = self.invoke("frobnicate")
        self.assertEqual(result.returncode, 2)
        self.assertIn("unknown command 'frobnicate'", result.stderr)
        self.assertIn("usage:", result.stderr)

    def test_child_writes_its_display_and_executes_the_command(self):
        with tempfile.TemporaryDirectory() as directory:
            display = Path(directory) / "display.json"
            result = self.invoke(
                "launch", "_child", str(display),
                sys.executable, "-c", "print('child reached')",
                env=self.child_env(DISPLAY=":99"),
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("child reached", result.stdout)
            self.assertEqual(json.loads(display.read_text()), {"DISPLAY": ":99"})

    def test_child_rejects_the_host_display(self):
        with tempfile.TemporaryDirectory() as directory:
            display = Path(directory) / "display.json"
            result = self.invoke(
                "launch", "_child", str(display), sys.executable, "-c", "pass",
                env=self.child_env(DISPLAY=":99", JC3VR_HOST_DISPLAY=":99"),
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(display.exists())
            self.assertIn("inherited the host DISPLAY", result.stderr)


if __name__ == "__main__":
    unittest.main()
