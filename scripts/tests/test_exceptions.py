"""A justification can exempt only its reviewed declaration or bounded file size."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from structure_check.exceptions import POLICY_PATH, apply_exceptions
from structure_check.rust import check_sources

SCRIPT = Path(__file__).resolve().parents[1] / "check_structure.py"


class ExceptionTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "src").mkdir()
        (self.root / "scripts").mkdir()
        (self.root / "design.md").write_text(
            "# Rationale\nThis fixture keeps one ordered exhaustive table together.\n",
            encoding="utf-8")

    def policy(self, rule, **fields):
        entry = dict(path="src/worker.rs", rule=rule, reason="One cohesive reviewed unit",
                     evidence="design.md", **fields)
        return "[[exceptions]]\n" + "\n".join(
            f"{key} = {json.dumps(value)}" for key, value in entry.items()) + "\n"

    def check(self, source, policy):
        (self.root / "src/worker.rs").write_text(source, encoding="utf-8")
        (self.root / POLICY_PATH).write_text(policy, encoding="utf-8")
        report = check_sources({"src/worker.rs": source.encode()})
        return apply_exceptions(report, self.root)

    def test_loc_exception_preserves_visible_reason_and_bound(self):
        result = self.check("const _: () = ();\n" * 1201,
                            self.policy("effective-loc", max_loc=1201))
        self.assertEqual(result["findings"], [])
        self.assertEqual(result["exceptions"][0]["max_loc"], 1201)
        self.assertEqual(result["exceptions"][0]["evidence"], "design.md")

    def test_growth_beyond_reviewed_limit_still_fails(self):
        result = self.check("const _: () = ();\n" * 1202,
                            self.policy("effective-loc", max_loc=1201))
        self.assertEqual(result["exceptions"], [])
        self.assertEqual(result["findings"][0]["rule"], "effective-loc")

    def test_reexport_exception_does_not_exempt_other_declarations(self):
        result = self.check("pub use crate::api::A; pub use crate::api::B;",
                            self.policy("reexport-whitelist", declaration="pub use crate::api::A;"))
        self.assertEqual(len(result["exceptions"]), 1)
        self.assertEqual(result["findings"][0]["declaration"], "pub use crate::api::B;")

    def test_whitelist_applies_to_library_roots_and_test_only_declarations(self):
        declaration = "pub(crate) use crate::api::A;"
        for path, source in [("src/lib.rs", declaration),
                             ("src/lib.rs", "#[cfg(test)] mod tests { " + declaration + " }")]:
            with self.subTest(source=source):
                (self.root / path).write_text(source, encoding="utf-8")
                policy = self.policy("reexport-whitelist", declaration=declaration)
                policy = policy.replace('path = "src/worker.rs"', f'path = "{path}"')
                (self.root / POLICY_PATH).write_text(policy, encoding="utf-8")
                report = apply_exceptions(check_sources({path: source.encode()}), self.root)
                self.assertEqual(report["findings"], [])
                self.assertEqual(len(report["exceptions"]), 1)

    def test_reexport_exception_does_not_exempt_glob_rule(self):
        result = self.check("pub use crate::api::*;",
                            self.policy("reexport-whitelist", declaration="pub use crate::api::*;"))
        self.assertEqual([f["rule"] for f in result["findings"]], ["wildcard-import"])

    def test_stale_or_ambiguous_declaration_requires_policy_update(self):
        policy = self.policy("reexport-whitelist", declaration="pub use crate::api::A;")
        for source in ["fn f() {}", "pub use crate::api::B;",
                       "pub use crate::api::A; pub use crate::api::A;"]:
            with self.subTest(source=source), self.assertRaisesRegex(ValueError, "stale or ambiguous"):
                self.check(source, policy)

    def test_loc_exception_becomes_stale_after_split(self):
        with self.assertRaisesRegex(ValueError, "stale"):
            self.check("fn f() {}", self.policy("effective-loc", max_loc=1250))

    def test_missing_evidence_reason_unknown_fields_and_unsupported_rules(self):
        policy = self.policy("reexport-whitelist", declaration="pub use crate::api::A;")
        variants = [policy.replace('evidence = "design.md"', 'evidence = "missing.md"'),
                    policy.replace('reason = "One cohesive reviewed unit"', 'reason = ""'),
                    policy + 'allow_everything = true\n',
                    self.policy("wildcard-import"),
                    policy.replace('evidence = "design.md"', 'evidence = "../design.md"')]
        for variant in variants:
            with self.subTest(policy=variant), self.assertRaises(ValueError):
                self.check("pub use crate::api::A;", variant)

    def test_duplicate_exception_is_rejected(self):
        policy = self.policy("reexport-whitelist", declaration="pub use crate::api::A;")
        with self.assertRaisesRegex(ValueError, "duplicate"):
            self.check("pub use crate::api::A;", policy + policy)

    def test_cli_passes_only_with_valid_documented_exception(self):
        self.check("pub use crate::api::A;",
                   self.policy("reexport-whitelist", declaration="pub use crate::api::A;"))
        subprocess.run(["git", "init", "--quiet", str(self.root)], check=True)
        result = subprocess.run([sys.executable, str(SCRIPT), "--root", str(self.root), "--json"],
                                capture_output=True, text=True, encoding="utf-8")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(json.loads(result.stdout)["exceptions"]), 1)
