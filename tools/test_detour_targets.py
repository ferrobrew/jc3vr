"""Guard the payload against duplicate inline detour targets."""

import re
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
PAYLOAD_SRC = REPO_ROOT / "payload" / "src"
DETOUR = re.compile(r"(?m)^[ \t]*#\[\s*detour\s*\(")
ADDRESS_PATH = re.compile(r"[\w:]+_ADDRESS")


def detour_attributes(source):
    for match in DETOUR.finditer(source):
        depth = 1
        index = match.end()
        while index < len(source) and depth:
            if source[index] == "(":
                depth += 1
            elif source[index] == ")":
                depth -= 1
            index += 1
        if depth:
            raise AssertionError(f"unterminated detour attribute at offset {match.start()}")
        yield match.start(), source[match.end() : index - 1]


def address_expression(body):
    match = re.search(r"\baddress\s*=\s*(.+?)\s*,?\s*$", body, re.DOTALL)
    if not match:
        raise AssertionError(f"detour attribute without an address: {body!r}")
    return match.group(1).strip()


def collect(payload_src=PAYLOAD_SRC, root=REPO_ROOT):
    occurrences = {}
    for path in sorted(Path(payload_src).rglob("*.rs")):
        source = path.read_text(encoding="utf-8")
        for offset, body in detour_attributes(source):
            expression = re.sub(r"\s+", "", address_expression(body))
            line = source.count("\n", 0, offset) + 1
            occurrences.setdefault(expression, []).append((path.relative_to(root), line, expression))
    return occurrences


class DetourTargetTests(unittest.TestCase):
    def test_scanner_finds_detours(self):
        self.assertTrue(collect(), f"no detour attributes found under {PAYLOAD_SRC}")

    def test_every_address_is_a_constant_path(self):
        aliased = [
            (path, line)
            for entries in collect().values()
            for path, line, expression in entries
            if not ADDRESS_PATH.fullmatch(expression)
        ]
        self.assertEqual(aliased, [], "detour addresses must name an `_ADDRESS` constant directly")

    def test_no_duplicate_detour_targets(self):
        duplicates = {expression: entries for expression, entries in collect().items() if len(entries) > 1}
        report = "\n".join(
            f"  {expression}:\n" + "\n".join(f"    {path}:{line}" for path, line, _ in entries)
            for expression, entries in sorted(duplicates.items())
        )
        self.assertEqual(duplicates, {}, f"duplicate inline detour targets in the payload:\n{report}")


class ScannerTests(unittest.TestCase):
    def scan(self, files):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, text in files.items():
                (root / name).write_text(text, encoding="utf-8")
            return collect(root, root)

    def test_canonicalises_multiline_and_compact_attributes(self):
        occurrences = self.scan(
            {
                "a.rs": "#[detour(\n    address = jc3gi::x::F::Go_ADDRESS,\n)]\nfn a() {}\n",
                "b.rs": "#[detour(address=jc3gi::x::F::Go_ADDRESS)]\nfn b() {}\n",
            }
        )
        self.assertEqual(list(occurrences), ["jc3gi::x::F::Go_ADDRESS"])
        self.assertEqual(len(occurrences["jc3gi::x::F::Go_ADDRESS"]), 2)

    def test_canonicalises_whitespace_inside_expression(self):
        occurrences = self.scan(
            {"a.rs": "#[detour(address = jc3gi :: x :: F :: Go_ADDRESS)]\nfn a() {}\n"}
        )
        self.assertEqual(list(occurrences), ["jc3gi::x::F::Go_ADDRESS"])
        self.assertIsNotNone(ADDRESS_PATH.fullmatch(next(iter(occurrences))))

    def test_ignores_commented_out_attributes(self):
        occurrences = self.scan(
            {
                "a.rs": (
                    "// #[detour(address = jc3gi::x::F::Go_ADDRESS)]\n"
                    "/// #[detour(address = jc3gi::x::F::Go_ADDRESS)]\n"
                    "fn a() {}\n"
                )
            }
        )
        self.assertEqual(occurrences, {})


if __name__ == "__main__":
    unittest.main()
