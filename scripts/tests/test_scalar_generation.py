"""Generation must reject ambiguous inputs and never repair source in check mode."""

from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import generate_scalar_execution as generator


class ScalarGenerationTests(unittest.TestCase):
    def setUp(self):
        temporary_parent = generator.ROOT / "target/scalar-generation/tests"
        temporary_parent.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(dir=temporary_parent)
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for relative in [generator.CATALOG, *generator.TARGETS.values()]:
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes((generator.ROOT / relative).read_bytes())

    def test_catalog_rejects_unknown_shapes_and_duplicate_instruction_authorities(self):
        path = self.root / generator.CATALOG
        original = path.read_text(encoding="utf-8")
        cases = [
            (original + "\n[[unknown]]\nvalue = 'extra'\n", "six scalar families"),
            (original.replace('rr = "I8AddRR"', 'typo = "I8AddRR"', 1), "expected fields"),
            (original.replace('rr = "I8SubRR"', 'rr = "I8AddRR"', 1), "duplicate variant"),
            (original.replace('operation = "IntegerConvert"', 'operation = "I8AddRR"', 1),
             "duplicate variant"),
            (original.replace('helper = "payload::integer_value::<0, 8, true>"',
                              'helper = 123', 1), "single-line strings"),
        ]
        for content, diagnostic in cases:
            with self.subTest(diagnostic=diagnostic):
                path.write_text(content, encoding="utf-8")
                with self.assertRaisesRegex(ValueError, diagnostic):
                    generator.load_catalog(path)

    def test_region_replacement_preserves_handwritten_text_and_rejects_bad_boundaries(self):
        source = "before\n    // BEGIN GENERATED x\nold\n    // END GENERATED x\nafter\n"
        self.assertEqual(generator.replace_region(source, "x", "new\nline"),
                         "before\n    // BEGIN GENERATED x\n    new\n    line\n"
                         "    // END GENERATED x\nafter\n")
        for content in [source.replace("END GENERATED x", "END GENERATED y"),
                        source + source, source.replace("old", "// BEGIN GENERATED nested")]:
            with self.subTest(source=content):
                with self.assertRaisesRegex(ValueError, "marker pair"):
                    generator.replace_region(content, "x", "new")

    def test_regeneration_is_idempotent_and_check_mode_preserves_bytes_and_mtime(self):
        # Exercise both native Windows checkout bytes and LF without changing policy.
        for index, relative in enumerate(generator.TARGETS.values()):
            path = self.root / relative
            content = path.read_bytes().replace(b"\r\n", b"\n")
            path.write_bytes(content if index % 2 else content.replace(b"\n", b"\r\n"))
        self.assertEqual(generator.generate(self.root, False), [])
        relative = generator.TARGETS["scalar-variants"]
        path = self.root / relative
        path.write_bytes(path.read_bytes().replace(b"I32AddRR", b"ManualDrift", 1))
        snapshot = {target: ((self.root / target).read_bytes(), (self.root / target).stat().st_mtime_ns)
                    for target in generator.TARGETS.values()}
        with patch.object(generator, "ROOT", self.root), patch.object(sys, "argv", ["generate", "--check"]):
            with redirect_stdout(StringIO()):
                self.assertEqual(generator.main(), 1)
        for target, (data, modified) in snapshot.items():
            self.assertEqual((self.root / target).read_bytes(), data)
            self.assertEqual((self.root / target).stat().st_mtime_ns, modified)
        self.assertEqual(generator.generate(self.root, False), [relative])
        self.assertEqual(generator.generate(self.root, False), [])
        self.assertEqual(generator.generate(self.root, True), [])

    def test_invalid_last_target_prevents_earlier_replacements(self):
        first = self.root / generator.TARGETS["scalar-variants"]
        last = self.root / generator.TARGETS["scalar-execution"]
        first.write_bytes(first.read_bytes().replace(b"I32AddRR", b"ManualDrift", 1))
        last.write_bytes(last.read_bytes().replace(b"END GENERATED scalar-execution", b"missing", 1))
        snapshot = first.read_bytes()
        with self.assertRaisesRegex(ValueError, "marker pair"):
            generator.generate(self.root, False)
        self.assertEqual(first.read_bytes(), snapshot)


if __name__ == "__main__":
    unittest.main()
