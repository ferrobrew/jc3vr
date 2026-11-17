"""Tests for the shared private rendering session."""

import json
import math
import os
import signal
import tempfile
import time
import tomllib
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

from tools.rendering.capture import harness
from tools.rendering.protocol import Command, Request, RestorationAck, RunOutcome
from tools.rendering.session.attach import (
    AttachedSession,
    SessionState,
    owning_process,
)
from tools.rendering.session.errors import SessionError
from tools.rendering.session.launch import RenderingSession
from tools.rendering.session.options import Options, parse_args
from tools.rendering.session import cli as session_cli


def restoration_wire(identifier):
    return {"id": identifier, "state": "restored", "metadata": {
        "generation": 1, "mode": None, "profile": "native", "ready": True,
        "paused": False, "frozen": False, "error": None, "warmup_frames": 0,
        "completed_frames": 0, "engine_dimensions": [2, 1]}}


def publish_restoration(config_dir):
    def publish(_):
        path = config_dir / "test_request.toml"
        if not path.exists():
            return
        request = tomllib.loads(path.read_text())
        path.unlink()
        folder = config_dir / "visual"
        folder.mkdir(exist_ok=True)
        (folder / f"{request['id']}.json").write_text(json.dumps(restoration_wire(request["id"])))
    return publish


class SessionTestCase(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def session(self, **options):
        runner = RenderingSession(Options(no_build=True, **options))
        runner.output = self.root / "output"
        runner.output.mkdir(exist_ok=True)
        runner.config_dir = self.root / "config"
        runner.config_dir.mkdir()
        runner.deadline = time.monotonic() + 20
        self.addCleanup(runner.close)
        return runner


class SharedChannelTests(SessionTestCase):
    def test_launch_and_attach_use_the_same_request_channel(self):
        launch = self.session()
        attach = AttachedSession(launch.config_dir, launch.deadline, "token", os.getpid())
        with patch.object(launch, "assert_running"), patch.object(attach, "assert_running"), \
                patch("tools.rendering.session.channel.time.sleep",
                      side_effect=publish_restoration(launch.config_dir)):
            launch_result = launch.request(Request(Command.END, "launch_end"))
            attach_result = attach.request(Request(Command.END, "attach_end"))
        self.assertIsInstance(launch_result, RestorationAck)
        self.assertIsInstance(attach_result, RestorationAck)
        self.assertEqual(launch_result.id, "launch_end")
        self.assertEqual(attach_result.id, "attach_end")

    def test_cancel_pending_request_is_shared(self):
        launch = self.session()
        attach = AttachedSession(launch.config_dir, launch.deadline, "token", os.getpid())
        for channel, identifier in ((launch, "launch_pending"), (attach, "attach_pending")):
            request = Request(Command.END, identifier)
            (launch.config_dir / "test_request.toml").write_text(request.to_toml())
            channel.pending_request = request
            self.assertTrue(channel.cancel_pending_request())
            self.assertFalse((launch.config_dir / "test_request.toml").exists())
            self.assertEqual(len(list(launch.config_dir.glob("test_request.toml.*.cancelled"))), 1)
            for cancelled in launch.config_dir.glob("test_request.toml.*.cancelled"):
                cancelled.unlink()


class AttachTests(SessionTestCase):
    def test_attach_requires_a_finite_positive_timeout(self):
        for timeout in (0.0, -1.0, math.nan, math.inf):
            with self.subTest(timeout=timeout), self.assertRaisesRegex(
                    SessionError, "finite and positive"):
                AttachedSession.attach(self.root / "runner.json", timeout)

    def test_attach_uses_the_session_state_and_releases_only_its_lock(self):
        config_dir = self.root / "config"
        config_dir.mkdir()
        path = self.root / "runner.json"
        path.write_text(json.dumps({"token": "abc123", "config_dir": str(config_dir)}))
        lock = Mock()
        with patch("tools.rendering.session.attach.owning_process", return_value=123), \
                patch("tools.rendering.session.attach.ExclusiveLock.acquire",
                      return_value=lock) as acquire:
            attach = AttachedSession.attach(path, 10)
        self.assertEqual(attach.pid, 123)
        self.assertEqual(attach.token, "abc123")
        acquire.assert_called_once_with(config_dir)
        with patch("tools.rendering.session.attach.os.kill") as kill:
            attach.close()
        kill.assert_not_called()
        lock.release.assert_called_once()
        self.assertIsNone(attach.lock)

    def test_attached_deadline_and_token_are_enforced(self):
        expired = AttachedSession(self.root, time.monotonic() - 1, "token", os.getpid())
        with self.assertRaisesRegex(SessionError, "deadline expired"):
            expired.assert_running()
        current = AttachedSession(self.root, time.monotonic() + 10, "token", os.getpid())
        with patch("tools.rendering.session.attach.Path.read_bytes",
                   return_value=b"OTHER=1\0"), \
                patch("tools.rendering.session.attach.os.kill"):
            with self.assertRaisesRegex(SessionError, "no longer owns"):
                current.assert_running()

    def test_ownership_requires_exactly_one_game_process(self):
        config_dir = self.root / "config"
        config_dir.mkdir()
        state = SessionState("abc123", config_dir)
        proc = self.root / "proc"
        for pid, comm, argv, token in (
                (10, "justcause3.exe", ["Z:\\JustCause3.exe"], "abc123"),
                (11, "justcause3.exe", ["Z:\\JustCause3.exe"], "other"),
                (12, "wineserver", ["wineserver"], "abc123")):
            entry = proc / str(pid)
            entry.mkdir(parents=True)
            (entry / "comm").write_text(comm + "\n")
            (entry / "cmdline").write_bytes(b"\0".join(item.encode() for item in argv) + b"\0")
            (entry / "environ").write_bytes(
                f"JC3VR_HARNESS_TOKEN={token}\0JC3VR_CONFIG_DIR={config_dir}\0".encode())
        self.assertEqual(owning_process(state, proc), 10)


class KeepAliveTests(SessionTestCase):
    def test_keep_alive_clears_the_overall_deadline_after_injection(self):
        runner = self.session(keep_alive=True)
        seen = {}
        def start():
            runner.deadline = time.monotonic() + 1
            return runner
        def serve():
            seen["deadline"] = runner.deadline
            raise KeyboardInterrupt
        with patch.object(runner, "start", side_effect=start), \
                patch.object(runner, "inject_until_ready") as inject, \
                patch.object(runner, "serve", side_effect=serve), \
                patch.object(runner, "close"):
            with self.assertRaises(KeyboardInterrupt):
                runner.run()
        inject.assert_called_once()
        self.assertEqual(seen["deadline"], math.inf)
        self.assertEqual(runner.result.status, RunOutcome.FAILED)

    def test_keep_alive_rejects_an_execution_callback(self):
        runner = self.session(keep_alive=True)
        with patch.object(runner, "start") as start, patch.object(runner, "close"):
            with self.assertRaisesRegex(SessionError, "execution callback"):
                runner.run(lambda _: None)
        start.assert_not_called()

    def test_launch_only_remains_bounded_and_does_not_inject(self):
        runner = self.session(launch_only=True)
        with patch.object(runner, "start", return_value=runner), \
                patch.object(runner, "inject_until_ready") as inject, \
                patch.object(runner, "serve", side_effect=KeyboardInterrupt), \
                patch.object(runner, "close"):
            with self.assertRaises(KeyboardInterrupt):
                runner.run()
        inject.assert_not_called()
        self.assertNotEqual(runner.deadline, math.inf)

    def test_keep_alive_is_mutually_exclusive_with_execution_modes(self):
        runner = RenderingSession(Options(keep_alive=True, suite=True))
        with patch.object(runner, "reject_existing") as reject, \
                self.assertRaisesRegex(SessionError, "mutually exclusive"):
            runner.check()
        reject.assert_not_called()

    def test_keep_alive_main_installs_handlers_and_closes(self):
        runner = Mock()
        runner.run.side_effect = KeyboardInterrupt
        with patch("tools.rendering.capture.harness.RenderingSession",
                   return_value=runner) as construct, \
                patch("tools.rendering.capture.harness.signal.signal") as install:
            self.assertEqual(harness.main(["--keep-alive"]), 130)
        options = construct.call_args.args[0]
        self.assertTrue(options.keep_alive)
        self.assertFalse(options.launch_only)
        runner.run.assert_called_once_with(None)
        runner.close.assert_called_once()
        self.assertEqual([call.args[0] for call in install.call_args_list],
                         [signal.SIGTERM, signal.SIGHUP])

    def test_session_cli_defaults_to_keep_alive(self):
        with patch("tools.rendering.session.cli.launch_main", return_value=0) as launch:
            self.assertEqual(session_cli.main(["--no-build"]), 0)
        launch.assert_called_once_with(["--no-build"], keep_alive=True)

    def test_parser_defaults(self):
        self.assertFalse(parse_args([]).keep_alive)
        self.assertTrue(parse_args(["--keep-alive"]).keep_alive)
        self.assertTrue(parse_args([], keep_alive=True).keep_alive)
        with patch("sys.stderr"), self.assertRaises(SystemExit):
            parse_args(["--keep-alive", "--suite"])
        with patch("sys.stderr"), self.assertRaises(SystemExit):
            parse_args(["--suite"], keep_alive=True)


class StatePointerTests(SessionTestCase):
    def test_runner_pointer_is_written_and_removed_for_its_token(self):
        runner = self.session()
        runner.gpu = "10de:2b85"
        runner.runtime = self.root / "runtime"
        runner.runtime.mkdir()
        runner.prefix = self.root / "prefix"
        runner.prefix.mkdir()
        runner.sessions_dir = self.root / "sessions"
        runner.sessions_dir.mkdir()
        runner.display_file = self.root / "display.json"
        pointer = self.root / "runner.json"
        with patch("tools.rendering.session.launch.RUNNER_POINTER", pointer), \
                patch.object(runner, "_terminate_owned"):
            runner._write_state()
            self.assertEqual(json.loads((runner.output / "runner.json").read_text())["token"],
                             runner.token)
            self.assertEqual(json.loads(pointer.read_text())["token"], runner.token)
            runner.close()
        self.assertFalse(pointer.exists())

    def test_runner_pointer_for_another_session_is_preserved(self):
        runner = self.session()
        runner.gpu = "10de:2b85"
        runner.runtime = self.root / "runtime"
        runner.runtime.mkdir()
        runner.sessions_dir = self.root / "sessions"
        runner.sessions_dir.mkdir()
        runner.display_file = self.root / "display.json"
        pointer = self.root / "runner.json"
        pointer.write_text(json.dumps({"token": "other"}))
        with patch("tools.rendering.session.launch.RUNNER_POINTER", pointer), \
                patch.object(runner, "_terminate_owned"):
            runner.close()
        self.assertTrue(pointer.exists())
