import os
import tempfile
import types
import unittest
from pathlib import Path

from tools.rendering.session.processes import (
    Process,
    ProcessError,
    processes,
    steam_client,
)


class ProcessTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.proc = Path(self.temporary.name)

    def tearDown(self):
        self.temporary.cleanup()

    def add(self, pid, comm, argv=()):
        entry = self.proc / str(pid)
        entry.mkdir()
        (entry / "comm").write_text(comm + "\n")
        (entry / "cmdline").write_bytes(b"\0".join(arg.encode() for arg in argv))
        return entry

    def test_missing_steam_client(self):
        self.add(1, "steamwebhelper", ("steamwebhelper",))
        self.add(2, "steam-runtime-launcher-service")
        self.add(3, "steam-runtime-launch-client")
        self.assertIsNone(steam_client(self.proc))

    def test_running_steam_client(self):
        self.add(7, "steam", ("/usr/bin/steam",))
        self.assertEqual(steam_client(self.proc), Process(7, "steam", ("/usr/bin/steam",), os.getuid()))

    def test_steam_client_belongs_to_the_current_user(self):
        self.add(7, "steam")
        self.assertIsNone(steam_client(self.proc, uid=os.getuid() + 1))

    def test_steam_client_of_another_user_is_ignored(self):
        class Entry:
            name = "7"

            def stat(self):
                return types.SimpleNamespace(st_uid=os.getuid() + 1)

            def __truediv__(self, _):
                return self

            def read_text(self):
                return "steam\n"

            def read_bytes(self):
                return b"/usr/bin/steam"

        class Root:
            def iterdir(self):
                return [Entry()]

        self.assertIsNone(steam_client(Root()))

    def test_conflicts_of_another_user_are_detected(self):
        class Entry:
            name = "11"

            def stat(self):
                return types.SimpleNamespace(st_uid=os.getuid() + 1)

            def __truediv__(self, _):
                return self

            def read_text(self):
                return "justcause3.exe\n"

            def read_bytes(self):
                return b"justcause3.exe"

        class Root:
            def iterdir(self):
                return [Entry()]

        found = processes(Root())
        self.assertEqual(len(found), 1)
        self.assertEqual(found[0].uid, os.getuid() + 1)
        self.assertIs(found[0].conflicts, True)

    def test_non_conflicting_process(self):
        self.add(12, "bash", ("bash",))
        found = processes(self.proc)
        self.assertEqual(len(found), 1)
        self.assertIs(found[0].conflicts, False)

    def test_stale_pid_is_skipped(self):
        (self.proc / "8").mkdir()
        self.assertEqual(processes(self.proc), ())

    def test_race_errors_are_skipped(self):
        class Vanishing:
            name = "9"

            def stat(self):
                raise FileNotFoundError

        class Root:
            def iterdir(self):
                return [Vanishing()]

        self.assertEqual(processes(Root()), ())

    def test_read_race_errors_are_skipped(self):
        class Vanishing:
            name = "9"

            def stat(self):
                return types.SimpleNamespace(st_uid=os.getuid())

            def __truediv__(self, _):
                return self

            def read_text(self):
                raise ProcessLookupError

        class Root:
            def iterdir(self):
                return [Vanishing()]

        self.assertEqual(processes(Root()), ())

    def test_unreadable_same_user_process_is_an_error(self):
        class Unreadable:
            name = "10"

            def stat(self):
                return types.SimpleNamespace(st_uid=os.getuid())

            def __truediv__(self, _):
                return self

            def read_text(self):
                raise PermissionError

        class Root:
            def iterdir(self):
                return [Unreadable()]

        with self.assertRaisesRegex(ProcessError, "cannot inspect process 10"):
            processes(Root())

    def test_unreadable_other_user_process_is_skipped(self):
        class Unreadable:
            name = "13"

            def stat(self):
                return types.SimpleNamespace(st_uid=os.getuid() + 1)

            def __truediv__(self, _):
                return self

            def read_text(self):
                raise PermissionError

        class Root:
            def iterdir(self):
                return [Unreadable()]

        self.assertEqual(processes(Root()), ())
