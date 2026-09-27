"""Regression coverage for strict source audits, including common false positives."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from tree_sitter import Parser

from structure_check.rust import LANGUAGE, check_sources, effective_loc


MANIFEST = b'[package]\nname = "fixture"\nversion = "0.1.0"\n'
SCRIPT = Path(__file__).resolve().parents[1] / "check_structure.py"


def audit(sources, manifests=None):
    return check_sources({p: s.encode() for p, s in sources.items()}, manifests)["findings"]


def rules(text, path="src/worker.rs"):
    return [f["rule"] for f in audit({path: text})]


class SourceRulesTests(unittest.TestCase):
    def test_explicit_imports_short_qualifiers_and_associated_calls(self):
        self.assertEqual(rules('''
use std::{fmt, sync::Arc};
use crate::error::Error;
fn f(x: Arc<Error>) -> fmt::Result { Type::method(); Ok(()) }
'''), [])

    def test_production_globs_include_groups_and_local_variants(self):
        result = rules('''
use crate::{a::{Thing, *}, b::Other};
fn f() { use Color::*; }
pub use crate::api::*;
''', "src/lib.rs")
        self.assertEqual(result, ["wildcard-import"] * 3)

    def test_deep_parent_paths_include_grouped_and_commented_imports(self):
        for text in ["use super::super::Thing;", "use super::{super::Thing};",
                     "use super::{super::*};", "use super::{super::Thing as T};",
                     "use super /* note */ :: super::Thing;",
                     "fn f() { super::super::run(); }"]:
            with self.subTest(text=text):
                self.assertIn("parent-traversal", rules(text))
        self.assertEqual(rules("use super::Thing; fn f() { super::run(); }"), [])

    def test_long_paths_in_signatures_bodies_and_type_arguments(self):
        result = rules('''
fn f(x: std::sync::Arc<crate::error::Error>) {
    crate::run();
    some::nested::run();
    let x = Vec::<crate::error::Error>::new();
}
''')
        self.assertEqual(result, ["qualified-path"] * 5)

    def test_dependency_names_and_leading_absolute_paths(self):
        manifest = MANIFEST + b'[dependencies]\nrenamed-api = "1"\n'
        result = audit({"src/lib.rs": "fn f() { renamed_api::run(); ::local::run(); }"},
                       {"Cargo.toml": manifest})
        self.assertEqual([f["rule"] for f in result], ["qualified-path"] * 2)

    def test_ufcs_attributes_macros_and_literals(self):
        self.assertEqual(rules('''
#[derive(thiserror::Error)]
struct E;
fn f() {
    <crate::model::Thing as crate::api::Trait>::call();
    let text = r###"use super::super::*; std::x::y();"###;
    println!("use bad::*;");
}
macro_rules! make { () => { $crate::private::run(); }; }
'''), [])

    def test_reexports_are_explicit_facades_only(self):
        text = "pub use crate::a::A; pub(crate) use crate::b::B;"
        for path in ["src/lib.rs", "src/feature/mod.rs"]:
            self.assertEqual(rules(text, path), [])
        self.assertEqual(rules(text), ["reexport-location"] * 2)
        manifest = MANIFEST + b'[lib]\npath = "entry.rs"\n'
        self.assertEqual(audit({"entry.rs": text}, {"Cargo.toml": manifest}), [])

    def test_comment_markers_in_literals_and_nested_comments(self):
        text = '''// Only a comment.
/// Documentation.
/* Outer
   /* Nested */
*/
fn f() { // Mixed.
    let text = r#"
// String content.
/* Still string content. */
"#;
    let url = "https://example.test";
    let slash = '/'; /* Trailing comment. */
}
'''.replace("\n", "\r\n").encode()
        self.assertEqual(effective_loc(text, Parser(LANGUAGE).parse(text).root_node), 8)
        self.assertEqual(audit({"src/worker.rs": text.decode()}), [])

    def test_effective_loc_boundary_includes_tests(self):
        text = "const _: () = ();\n" * 1200
        self.assertEqual(rules("// comment\n\n" + text), [])
        self.assertEqual(rules(text + "const _: () = ();"), ["effective-loc"])
        result = audit({"tests/large.rs": text + "const _: () = ();"},
                       {"Cargo.toml": MANIFEST})
        self.assertEqual([f["rule"] for f in result], ["effective-loc"])

    def test_malformed_syntax_including_missing_unnamed_tokens(self):
        for text in ["fn broken( {", "fn f() { let x = 1 }", "mod {", "use ;", "pub use;"]:
            self.assertIn("parse-error", rules(text))

    def test_locations_ignore_multiline_string_fixtures(self):
        result = audit({"src/worker.rs": 'const S: &str = r#"\nuse bad::*;\n"#;\nuse real::*;\n'})
        self.assertEqual(len(result), 1)
        self.assertEqual((result[0]["line"], result[0]["column"]), (4, 1))


class TestScopeTests(unittest.TestCase):
    def test_inline_cfg_scopes_do_not_exempt_neighboring_code(self):
        result = rules('''
#[cfg(test)] mod tests { use super::*; mod nested { use super::*; } }
use real::*;
''')
        self.assertEqual(result, ["wildcard-import"])

    def test_cfg_boolean_expressions_and_comments(self):
        for cfg in ['test', 'all(test, feature = "extra")', 'not(not(test))',
                    'all(feature = "test", /* explanation */ test)']:
            with self.subTest(cfg=cfg):
                self.assertEqual(rules(f"#[cfg({cfg})] mod t {{ use super::*; }}"), [])
        for cfg in ['any(test, feature = "extra")', 'not(test)', 'feature = "test"']:
            with self.subTest(cfg=cfg):
                self.assertEqual(rules(f"#[cfg({cfg})] mod t {{ use super::*; }}"),
                                 ["wildcard-import"])

    def test_inner_cfg_and_cfg_attr(self):
        self.assertEqual(rules("#![cfg(test)]\nuse super::*;"), [])
        self.assertEqual(rules("#[cfg_attr(test, allow(unused))] use other::*;"),
                         ["wildcard-import"])

    def test_out_of_line_test_module_with_arbitrary_name(self):
        sources = {
            "src/lib.rs": "#[cfg(test)] mod checks; mod worker;",
            "src/checks.rs": "use super::*; mod nested;",
            "src/checks/nested.rs": "use super::*;",
            "src/worker.rs": "use external::*;",
        }
        result = audit(sources, {"Cargo.toml": MANIFEST})
        self.assertEqual([(f["path"], f["rule"]) for f in result],
                         [("src/worker.rs", "wildcard-import")])

    def test_path_attribute_and_production_shared_test_source(self):
        sources = {
            "src/lib.rs": '#[cfg(test)] #[path = "../support/checks.rs"] mod checks;',
            "support/checks.rs": "use shared::*;",
        }
        self.assertEqual(audit(sources, {"Cargo.toml": MANIFEST}), [])
        sources["src/lib.rs"] += '\n#[path = "../support/checks.rs"] mod production;'
        self.assertEqual(len(audit(sources, {"Cargo.toml": MANIFEST})), 1)

    def test_integration_targets_are_tests_but_examples_and_benches_are_not(self):
        sources = {"tests/check.rs": "use api::*;", "examples/demo.rs": "use api::*;",
                   "benches/speed.rs": "use api::*;", "custom/check.rs": "use api::*;"}
        manifest = MANIFEST + b'[[test]]\nname = "custom"\npath = "custom/check.rs"\n'
        result = audit(sources, {"Cargo.toml": manifest})
        self.assertEqual([f["path"] for f in result], ["benches/speed.rs", "examples/demo.rs"])

    def test_inline_parent_resolves_out_of_line_test_descendant(self):
        self.assertEqual(audit({
            "src/lib.rs": "mod feature { #[cfg(test)] mod checks; }",
            "src/feature/checks.rs": "use super::*;",
        }), [])

    def test_custom_crate_root_resolves_modules_from_its_directory(self):
        manifest = MANIFEST + b'[lib]\npath = "entry.rs"\n'
        self.assertEqual(audit({
            "entry.rs": "#[cfg(test)] mod checks;",
            "checks.rs": "use super::*;",
        }, {"Cargo.toml": manifest}), [])

    def test_integration_root_and_disabled_auto_targets(self):
        sources = {"tests/check.rs": "mod support;", "tests/support/mod.rs": "use super::*;"}
        self.assertEqual(audit(sources, {"Cargo.toml": MANIFEST}), [])
        manifest = MANIFEST + b'autotests = false\n'
        self.assertEqual(len(audit(sources, {"Cargo.toml": manifest})), 1)


class CommandLineTests(unittest.TestCase):
    def test_strict_exit_status_json_and_git_file_selection(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "--quiet", directory], check=True)
            (root / ".gitignore").write_text("ignored/\n", encoding="utf-8")
            (root / "ignored").mkdir()
            (root / "ignored" / "bad.rs").write_text("use bad::*;", encoding="utf-8")
            source = root / "worker.rs"
            source.write_text("fn f() {}", encoding="utf-8")

            def run():
                return subprocess.run([sys.executable, str(SCRIPT), "--root", directory, "--json"],
                                      capture_output=True, text=True, encoding="utf-8")

            result = run()
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout),
                             {"files": 1, "findings": [], "exceptions": []})
            subprocess.run(["git", "-C", directory, "add", "worker.rs"], check=True)
            source.write_text("use bad::*;", encoding="utf-8")
            result = run()
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertEqual(json.loads(result.stdout)["findings"][0]["rule"], "wildcard-import")
            source.unlink()
            self.assertEqual(json.loads(run().stdout)["files"], 0)

    def test_missing_repository_exits_as_tool_error(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run([sys.executable, str(SCRIPT), "--root", directory],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)

    def test_utf8_paths_and_invalid_manifests(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "--quiet", directory], check=True)
            filename = "\u4e2d\u6587-\U0001f680.rs"
            (root / filename).write_text("use bad::*;", encoding="utf-8")

            def run():
                return subprocess.run([sys.executable, str(SCRIPT), "--root", directory, "--json"],
                                      capture_output=True, text=True, encoding="utf-8")

            result = run()
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertEqual(json.loads(result.stdout)["findings"][0]["path"], filename)
            (root / "Cargo.toml").write_text("[invalid", encoding="utf-8")
            self.assertEqual(run().returncode, 2)
