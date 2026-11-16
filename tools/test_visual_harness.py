import contextlib
import json
import logging
import os
import struct
import subprocess
import sys
import tempfile
import time
import unittest
import zlib
from pathlib import Path
from unittest.mock import patch

from tools.visual_harness import (
    HarnessError,
    Options,
    VisualHarness,
    child_main,
    default_smoke,
    validate_capture,
    validate_png,
)
from tools.visual_protocol import NativeCapture, ProtocolError, RunOutcome, SmokeResult


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.runner = VisualHarness(Options(no_build=True))
        self.runner.output = self.root
        self.runner.env = os.environ.copy()
        self.runner.env["JC3VR_HARNESS_TOKEN"] = self.runner.token
        self.runner.deadline = time.monotonic() + 10

    def tearDown(self):
        self.runner.close()
        self.temporary.cleanup()

    def test_check_has_no_startup_side_effects(self):
        runner = VisualHarness(Options(check=True, no_build=True, gpu="10de:2b85"))
        manifest = self.root / "openxr.json"
        library = self.root / "lib/libopenxr_monado.so"
        manifest.write_text(json.dumps({"runtime": {"library_path": str(library)}}))
        with patch("tools.visual_harness.MANIFEST", manifest), \
                patch.object(runner, "reject_existing"), \
                patch.object(runner, "_library", return_value=self.root), \
                patch.object(runner, "_xrizer", return_value=self.root), \
                patch.object(runner, "_gpu", return_value="10de:2b85"), \
                patch.object(runner, "_check_artifacts"), \
                patch("tools.visual_harness.shutil.which", return_value="/fake/tool"), \
                patch("tools.visual_harness.os.access", return_value=True), \
                patch.object(Path, "exists", return_value=True), \
                patch("tools.visual_harness.tempfile.mkdtemp") as create, \
                patch("tools.visual_harness.subprocess.Popen") as launch:
            runner.check()
        create.assert_not_called()
        launch.assert_not_called()
        self.assertIsNone(runner.runtime)
        self.assertIsNone(runner.env)
        self.assertIsNone(runner.work)

    def test_preview_follows_a_run_even_when_startup_fails(self):
        output = self.root / "preview-run"
        pointer = self.root / "latest.json"
        runner = VisualHarness(Options(no_build=True, output=output))
        with patch("tools.visual_harness.ROOT", self.root), \
                patch("tools.visual_harness.DEFAULT_POINTER", pointer), \
                patch.dict(os.environ, {"XDG_RUNTIME_DIR": str(self.root)}), \
                patch.object(runner, "check"), \
                patch.object(runner, "_acquire_lock"), \
                patch.object(runner, "reject_existing"), \
                patch.object(runner, "_seed_settings"), \
                patch.object(runner, "_base_env", side_effect=HarnessError("startup probe")), \
                self.assertRaisesRegex(HarnessError, "startup probe"):
            runner.run()
        self.assertEqual(json.loads(pointer.read_text())["output"], str(output))
        report = json.loads((output / "result.json").read_text())
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["error"], "startup probe")
        self.assertTrue(runner._closed)

    def test_nonzero_command_is_an_error(self):
        with self.assertRaisesRegex(HarnessError, "status 7"):
            self.runner._command([sys.executable, "-c", "raise SystemExit(7)"], "exit", 2)

    def test_commands_do_not_return_previous_log_contents(self):
        for value in ("first", "second"):
            result = self.runner._command([sys.executable, "-c", f"print({value!r})"], "repeat", 2)
            self.assertEqual(result.stdout.strip(), value)

    def test_command_timeout_reaps_the_process(self):
        start = time.monotonic()
        with self.assertRaisesRegex(HarnessError, "exceeded"):
            self.runner._command([sys.executable, "-c", "import time;time.sleep(10)"], "slow", 0.1)
        self.assertLess(time.monotonic() - start, 3)
        self.assertIsNotNone(self.runner.processes[-1].poll())

    def test_run_deadline_prevents_new_commands(self):
        self.runner.deadline = time.monotonic() - 1
        with patch.object(self.runner, "_spawn") as spawn, self.assertRaisesRegex(HarnessError, "run timeout"):
            self.runner._command(["never"], "late", 2)
        spawn.assert_not_called()

    def test_early_exit_aborts_readiness_wait(self):
        self.runner.monado = subprocess.Popen([sys.executable, "-c", "pass"])
        self.runner.processes.append(self.runner.monado)
        self.runner.monado.wait(timeout=2)
        with self.assertRaisesRegex(HarnessError, "Monado exited"):
            self.runner._wait_for(lambda: False, "socket", 2)

    def test_readiness_wait_is_bounded(self):
        start = time.monotonic()
        with self.assertRaisesRegex(HarnessError, "timed out waiting"):
            self.runner._wait_for(lambda: False, "socket", 0.1)
        self.assertLess(time.monotonic() - start, 1)

    def test_monado_stdin_pipe_stays_open_until_cleanup(self):
        process = self.runner._spawn(
            [sys.executable, "-c", "import sys; sys.stdin.buffer.read()"],
            "stdin", stdin=subprocess.PIPE,
        )
        time.sleep(0.05)
        self.assertIsNone(process.poll())
        self.assertFalse(process.stdin.closed)
        self.runner.close()
        self.assertTrue(process.stdin.closed)
        self.assertIsNotNone(process.poll())

    def test_menu_stops_when_ready_or_expired(self):
        self.runner.menu_deadline = time.monotonic() + 10
        self.runner.mark_gameplay_ready()
        with patch.object(self.runner, "_command") as command:
            self.assertFalse(self.runner.pump_menu())
            self.runner.gameplay_ready = False
            self.runner.menu_deadline = time.monotonic() - 1
            self.assertFalse(self.runner.pump_menu())
        command.assert_not_called()

    @contextlib.contextmanager
    def _menu(self, focus="42", focus_code=0):
        self.runner.xdotool = "/private/xdotool"
        self.runner.display_file = self.root / "display.json"
        self.runner.display_file.write_text(json.dumps({"DISPLAY": ":123", "XAUTHORITY": "/private/auth"}))
        self.runner.env["DISPLAY"] = ":0"
        self.runner.env["XAUTHORITY"] = "/host/auth"
        self.runner.deadline = 110
        self.runner.menu_deadline = 110
        events = []

        def execute(arguments, name, timeout, **kwargs):
            events.append(arguments[1:])
            self.assertGreater(timeout, 0)
            self.assertLessEqual(timeout, 0.5)
            self.assertEqual(kwargs["env"]["DISPLAY"], ":123")
            self.assertEqual(kwargs["env"]["XAUTHORITY"], "/private/auth")
            output = "7\n42\n" if arguments[1] == "search" else focus if arguments[1] == "getwindowfocus" else ""
            code = focus_code if arguments[1] == "windowfocus" else 0
            return subprocess.CompletedProcess(arguments, code, output)

        def key(arguments, **kwargs):
            events.append(arguments[1:])
            return subprocess.CompletedProcess(arguments, 0)

        with patch("tools.visual_harness.time.monotonic", return_value=100), \
                patch.dict(os.environ, {"DISPLAY": ":0", "XAUTHORITY": "/host/auth"}), \
                patch.object(self.runner, "_command", side_effect=execute) as command, \
                patch("tools.visual_harness.subprocess.run", side_effect=key) as keys, \
                patch("tools.visual_harness.time.sleep", side_effect=lambda seconds: events.append(["hold", seconds])) as hold:
            yield events, command, keys, hold

    def test_menu_verifies_focus_and_holds_return_on_private_display(self):
        with self._menu() as (events, _, keys, hold):
            self.assertTrue(self.runner.pump_menu())
            self.assertEqual(events, [
                ["search", "--onlyvisible", "--class", "[Jj]ust.?[Cc]ause"],
                ["search", "--onlyvisible", "--name", "[Jj]ust.?[Cc]ause"],
                ["windowfocus", "42"], ["getwindowfocus", "-f"],
                ["keydown", "Return"], ["hold", 0.3], ["keyup", "Return"],
            ])
            self.assertGreaterEqual(hold.call_args.args[0], 0.25)
            for call in keys.call_args_list:
                self.assertEqual(call.kwargs["env"]["DISPLAY"], ":123")
                self.assertEqual(call.kwargs["env"]["XAUTHORITY"], "/private/auth")
                self.assertEqual(call.kwargs["timeout"], 0.5)
                self.assertTrue(call.kwargs["check"])
            self.assertFalse(self.runner.pump_menu())
            self.assertEqual(keys.call_count, 2)

    def test_menu_focus_mismatch_sends_no_keys(self):
        for focus in ("7", "420", "", "42\n7"):
            with self.subTest(focus=focus), self._menu(focus=focus) as (_, _, keys, hold):
                self.runner.last_menu = 0
                self.assertFalse(self.runner.pump_menu())
                keys.assert_not_called()
                hold.assert_not_called()

    def test_menu_focus_failure_sends_no_keys(self):
        with self._menu(focus_code=1) as (events, _, keys, hold):
            self.assertFalse(self.runner.pump_menu())
            self.assertEqual(events[-1], ["windowfocus", "42"])
            keys.assert_not_called()
            hold.assert_not_called()

    def test_menu_always_releases_after_keydown_failure(self):
        for error in (OSError("unavailable"), subprocess.TimeoutExpired("xdotool", 0.5),
                      subprocess.CalledProcessError(1, "xdotool"), KeyboardInterrupt()):
            with self.subTest(error=type(error).__name__), self._menu() as (_, _, keys, hold):
                self.runner.last_menu = 0
                keys.side_effect = [error, subprocess.CompletedProcess([], 0)]
                with self.assertRaises(KeyboardInterrupt if isinstance(error, KeyboardInterrupt) else HarnessError):
                    self.runner.pump_menu()
                self.assertEqual([call.args[0][1:] for call in keys.call_args_list],
                                 [["keydown", "Return"], ["keyup", "Return"]])
                hold.assert_not_called()

    def test_menu_releases_on_hold_failure_and_after_deadline(self):
        for error in (RuntimeError("hold failed"), KeyboardInterrupt()):
            with self.subTest(error=type(error).__name__), self._menu() as (_, _, keys, hold):
                self.runner.last_menu = 0

                def interrupt(seconds, error=error):
                    self.runner.deadline = 99
                    self.runner.menu_deadline = 99
                    self.runner.mark_gameplay_ready()
                    raise error

                hold.side_effect = interrupt
                with self.assertRaises(type(error)):
                    self.runner.pump_menu()
                self.assertEqual([call.args[0][1:] for call in keys.call_args_list],
                                 [["keydown", "Return"], ["keyup", "Return"]])
                self.assertEqual(keys.call_args.kwargs["env"]["DISPLAY"], ":123")
                self.runner.gameplay_ready = False

    def test_menu_release_failure_is_reported(self):
        with self._menu() as (_, _, keys, hold):
            keys.side_effect = [subprocess.CompletedProcess([], 0), OSError("release failed")]
            with self.assertRaisesRegex(HarnessError, "menu keyup.*release failed"):
                self.runner.pump_menu()
            hold.assert_called_once_with(0.3)
            self.assertEqual(keys.call_args.args[0][1:], ["keyup", "Return"])

    def test_menu_release_failure_preserves_original_interruption(self):
        with self._menu() as (_, _, keys, hold):
            error = KeyboardInterrupt()
            hold.side_effect = error
            keys.side_effect = [subprocess.CompletedProcess([], 0), OSError("release failed")]
            with self.assertLogs("visual_harness", level="ERROR"), self.assertRaises(KeyboardInterrupt) as raised:
                self.runner.pump_menu()
            self.assertIs(raised.exception, error)
            self.assertIn("menu keyup", error.__notes__[0])
            self.assertEqual(keys.call_args.args[0][1:], ["keyup", "Return"])

    def test_display_is_reported_by_child_not_host(self):
        self.runner.display_file = self.root / "display.json"
        env = {"DISPLAY": ":123", "XAUTHORITY": "/private/auth",
               "XR_RUNTIME_JSON": "/host/native-runtime.json"}
        with patch.dict(os.environ, env, clear=True), \
                patch("tools.visual_harness.os.execvp") as execute:
            child_main([str(self.runner.display_file), "private-command"])
            self.assertNotIn("XR_RUNTIME_JSON", os.environ)
        self.assertEqual(self.runner.display_env()["DISPLAY"], ":123")
        self.assertEqual(self.runner.display_env()["XAUTHORITY"], "/private/auth")
        execute.assert_called_once_with("private-command", ["private-command"])

    def test_external_documents_are_private_but_saves_are_copied(self):
        self.runner.prefix = self.root / "compatdata"
        user = self.runner.prefix / "pfx/drive_c/users/player"
        user.mkdir(parents=True)
        external = self.root / "external-documents"
        save = external / "Square Enix/Just Cause 3"
        save.mkdir(parents=True)
        (save / "save.dat").write_text("original")
        (user / "Documents").symlink_to(external, target_is_directory=True)
        (user / "My Documents").symlink_to("Documents", target_is_directory=True)
        self.runner._isolate_profile_links()
        copied = user / "Documents/Square Enix/Just Cause 3/save.dat"
        self.assertFalse((user / "Documents").is_symlink())
        self.assertTrue((user / "My Documents").is_symlink())
        self.assertEqual(copied.read_text(), "original")
        copied.write_text("private")
        self.assertEqual((save / "save.dat").read_text(), "original")

    @unittest.skipUnless(hasattr(os, "pidfd_open"), "Linux pidfds are required")
    def test_cleanup_kills_owned_escaped_session_but_not_unrelated_process(self):
        escaped = subprocess.Popen([sys.executable, "-c", "import time;time.sleep(20)"],
                                   env=self.runner.env, start_new_session=True)
        unrelated = subprocess.Popen([sys.executable, "-c", "import time;time.sleep(20)"],
                                     start_new_session=True)
        try:
            self.runner.close()
            escaped.wait(timeout=2)
            self.assertIsNone(unrelated.poll())
        finally:
            for process in (escaped, unrelated):
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=2)

    def test_callback_result_is_not_replaced_by_launch_success(self):
        with patch.object(self.runner, "start", return_value=self.runner):
            self.assertEqual(self.runner.run(lambda _: SmokeResult(False)), SmokeResult(False))
        self.assertEqual(self.runner.result.status, RunOutcome.NOT_VERIFIED)
        self.assertTrue(self.runner._closed)

    def test_untyped_callback_result_is_rejected(self):
        with patch.object(self.runner, "start", return_value=self.runner), self.assertRaises(ProtocolError):
            self.runner.run(lambda _: {"smoke_succeeded": True})
        self.assertEqual(self.runner.result.status, RunOutcome.FAILED)
        self.assertFalse(self.runner.result.smoke_succeeded)

    def test_png_crc_deflate_and_dimensions(self):
        def chunk(kind, payload):
            return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))
        path = self.root / "pair.png"
        header = chunk(b"IHDR", struct.pack(">IIBBBBB", 2, 1, 8, 6, 0, 0, 0))
        body = chunk(b"IDAT", zlib.compress(b"\x00" + b"\xff" * 8))
        good = b"\x89PNG\r\n\x1a\n" + header + body + chunk(b"IEND", b"")
        path.write_bytes(good)
        self.assertEqual(validate_png(path), (2, 1))
        damaged = bytearray(good)
        damaged[-1] ^= 1
        path.write_bytes(damaged)
        with self.assertRaisesRegex(HarnessError, "CRC"):
            validate_png(path)
        path.write_bytes(b"\x89PNG\r\n\x1a\n" + header + chunk(b"IDAT", zlib.compress(b"short")) + chunk(b"IEND", b""))
        with self.assertRaisesRegex(HarnessError, "decoded length"):
            validate_png(path)

    def test_capture_metadata_must_match_acknowledgement(self):
        capture = NativeCapture("smoke_1", 1, 100, "eye_textures", 2, 1, 1, 1, (True, True), True, True)
        (self.root / "smoke_1.capture.json").write_text(json.dumps({
            "id": "wrong", "frame_ordinal": 1, "predicted_display_time_ns": 100,
            "source": "eye_textures", "width": 2, "height": 1, "eye_width": 1, "eye_height": 1,
            "fresh_eyes": [True, True], "gameplay_ready": True, "xr_session_running": True,
        }))
        with self.assertRaisesRegex(ProtocolError, "metadata disagrees"):
            validate_capture(self.root, "smoke_1", capture)

    def test_output_rejects_stale_artifacts_before_launch(self):
        self.runner.options.output = self.root
        (self.root / "stale.json").write_text("{}")
        with (patch.object(self.runner, "reject_existing"), patch.object(self.runner, "_spawn") as spawn,
              self.assertRaisesRegex(HarnessError, "empty directory")):
            self.runner.check()
        spawn.assert_not_called()

    def test_launch_whitelist_excludes_host_path_secrets_and_display(self):
        self.runner.gpu = "10de:2b85"
        self.runner.sniper = Path("/runtime/entry")
        self.runner.proton = Path("/runtime/proton")
        self.runner.boot_injector = Path("/build/boot.exe")
        self.runner.game = Path("/game/JustCause3.exe")
        self.runner.display_file = self.root / "display.json"
        self.runner.env.update(PATH="/host/path", SECRET_TOKEN="private-value", DISPLAY=":42",
                               XR_RUNTIME_JSON="/host/native-runtime.json", JC3VR_CONFIG_DIR="/private/config")
        command = self.runner._launch_command()
        self.assertNotIn("PATH=/host/path", command)
        self.assertNotIn("SECRET_TOKEN=private-value", command)
        self.assertNotIn("DISPLAY=:42", command)
        self.assertNotIn("XR_RUNTIME_JSON=/host/native-runtime.json", command)
        self.assertIn("JC3VR_CONFIG_DIR=/private/config", command)

    def test_native_runtime_uses_private_config_without_windows_override(self):
        self.runner.runtime = self.root / "runtime"
        self.runner.runtime.mkdir()
        self.runner.work = self.root / "work"
        self.runner.prefix = self.root / "prefix"
        self.runner.config_dir = self.root / "config"
        self.runner.sessions_dir = self.root / "sessions"
        self.runner.vr_override = self.root / "xrizer"
        self.runner.steam = self.root / "steam"
        self.runner.appid = "123456"
        manifest = self.root / "native-runtime.json"
        manifest.write_text("{}")
        with patch("tools.visual_harness.MANIFEST", manifest), \
                patch.dict(os.environ, {"XR_RUNTIME_JSON": "/host/runtime.json"}):
            self.runner._base_env()
        self.assertNotIn("XR_RUNTIME_JSON", self.runner.env)
        config = Path(self.runner.env["XDG_CONFIG_HOME"])
        active = config / "openxr/1/active_runtime.json"
        self.assertTrue(active.is_symlink())
        self.assertEqual(active.resolve(), manifest)
        registration = json.loads((config / "openvr/openvrpaths.vrpath").read_text())
        self.assertEqual(registration, {
            "version": 1, "jsonid": "vrpathreg", "runtime": [str(self.runner.vr_override)],
        })
        self.assertTrue(config.is_relative_to(self.runner.runtime))
        self.assertEqual(self.runner.env["PRESSURE_VESSEL_IMPORT_OPENXR_1_RUNTIMES"], "1")

    def test_default_smoke_waits_for_two_sequential_acknowledgements(self):
        self.runner.config_dir = self.root / "config"
        self.runner.config_dir.mkdir()
        folder = self.runner.config_dir / "captures"
        folder.mkdir()
        seen = []
        def publish(_):
            request = self.runner.config_dir / "test_request.toml"
            if not request.exists():
                return
            capture_id = f"smoke_{len(seen) + 1}"
            self.assertIn(f'id = "{capture_id}"', request.read_text())
            request.unlink()
            seen.append(capture_id)
            (folder / f"{capture_id}.json").write_text(json.dumps({
                "id": capture_id, "state": "completed", "gameplay_ready": True,
                "frame_ordinal": len(seen), "predicted_display_time_ns": len(seen) * 100,
                "source": "eye_textures", "width": 2, "height": 1, "eye_width": 1, "eye_height": 1,
                "fresh_eyes": [True, True], "xr_session_running": True,
            }))
        with patch.object(self.runner, "inject", return_value=subprocess.CompletedProcess([], 0, "")), \
                patch.object(self.runner, "pump_menu"), \
                patch("tools.visual_harness.time.sleep", side_effect=publish), \
                patch("tools.visual_harness.validate_capture", side_effect=lambda _, __, result: result):
            result = default_smoke(self.runner)
        self.assertTrue(result.smoke_succeeded)
        self.assertEqual(seen, ["smoke_1", "smoke_2"])
        self.assertTrue(self.runner.gameplay_ready)

    def test_smoke_never_retries_an_export_failure(self):
        self.runner.options.startup_timeout = 1
        with (patch.object(self.runner, "inject", return_value=subprocess.CompletedProcess([], 1, "could not run the payload")) as inject,
              self.assertRaisesRegex(HarnessError, "not safe to retry")):
            default_smoke(self.runner)
        inject.assert_called_once()

    def test_report_records_failure_after_cleanup(self):
        with (tempfile.TemporaryFile() as lock, patch.object(self.runner, "start", return_value=self.runner),
              self.assertRaises(HarnessError)):
            self.runner._lock = lock
            self.runner.run(lambda _: (_ for _ in ()).throw(HarnessError("smoke failed")))
        report = json.loads((self.root / "result.json").read_text())
        self.assertFalse(report["smoke_succeeded"])
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["error"], "smoke failed")
        self.assertIn("cleanup_errors", report)

    def test_surviving_owned_process_preserves_private_files_and_fails_report(self):
        self.runner.prefix = self.root / "prefix"
        self.runner.prefix.mkdir()
        self.runner.runtime = self.root / "runtime"
        self.runner.runtime.mkdir()
        with patch.object(self.runner, "_owned_pids", return_value=[31]), \
                patch("tools.visual_harness.os.close") as close_fd, \
                patch("tools.visual_harness.signal.pidfd_send_signal"), \
                patch("tools.visual_harness.time.monotonic", side_effect=[0, 5, 6, 10]):
            self.runner.close()
        close_fd.assert_called_once_with(31)
        self.assertTrue(self.runner.prefix.is_dir())
        self.assertTrue(self.runner.runtime.is_dir())
        self.runner.result.complete(SmokeResult(True))
        self.runner._write_report()
        report = json.loads((self.root / "result.json").read_text())
        self.assertFalse(report["smoke_succeeded"])
        self.assertEqual(report["status"], "failed")
        self.assertTrue(report["cleanup_errors"])

    def test_callback_failure_always_cleans_up(self):
        def fail(_):
            raise HarnessError("smoke: not ready")
        with patch.object(self.runner, "start", return_value=self.runner), self.assertRaisesRegex(HarnessError, "not ready"):
            self.runner.run(fail)
        self.assertTrue(self.runner._closed)


if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    unittest.main()
