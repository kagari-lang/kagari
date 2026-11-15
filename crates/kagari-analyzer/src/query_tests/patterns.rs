use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_hir::{analysis::FileAnalysis, hir::stmt::StmtKind, types::TypeId};
use kagari_source::{
    diagnostic::{Diagnostic, DiagnosticKind},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::scalar::BuiltinType;
use std::sync::*;

const POINT: &str = "struct Point { val x: i32, val y: i32 }";

fn analyze_source(source: &str) -> Arc<FileAnalysis> {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("patterns.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    snapshot.file(root).unwrap().clone()
}

fn check_source(source: &str) -> Vec<Diagnostic> {
    analyze_source(source).result().diagnostics().to_vec()
}

#[test]
fn declaration_pattern_shadowing_publishes_all_leaves_after_the_initializer() {
    let source = "
const LIMIT: i32 = 7;
struct Names { val None: i32, val LIMIT: i32 }
fn simple() -> i32 { val None = 1; None }
fn read(x: i32) -> i32 {
    val ((x, None), LIMIT) = ((x + 1, 2), LIMIT);
    val Names { None, LIMIT } = Names { None: None, LIMIT: LIMIT };
    x + None + LIMIT
}";
    let file = analyze_source(source);
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let parameter = source.find("x: i32").unwrap();
    let rhs = source.find("x + 1").unwrap();
    let declaration = source.find("((x, None)").unwrap() + 2;
    let tail = source.rfind("x + None").unwrap();
    assert_eq!(
        file.queries().definition_at(rhs),
        file.queries().definition_at(parameter)
    );
    assert_eq!(
        file.queries().definition_at(tail),
        file.queries().definition_at(declaration)
    );
    assert_ne!(
        file.queries().definition_at(rhs),
        file.queries().definition_at(tail)
    );
    for name in ["None", "LIMIT"] {
        let site = source.find("val Names { None, LIMIT }").unwrap()
            + "val Names { ".len()
            + if name == "LIMIT" { "None, ".len() } else { 0 };
        let tail = source.rfind(name).unwrap();
        assert_eq!(
            file.queries().definition_at(tail),
            file.queries().definition_at(site)
        );
        assert!(
            file.queries()
                .visible_bindings(tail)
                .iter()
                .any(|binding| binding.declaration.name == name)
        );
    }
    let unknown = check_source("fn bad() { val (first, second) = (1, first); }");
    assert!(
        unknown.iter().any(|diagnostic| matches!(
            &diagnostic.kind, DiagnosticKind::UnknownName { name } if name == "first"
        )),
        "{unknown:?}"
    );
    let wildcard = check_source("fn bad() { val _ = 1; _ }");
    assert!(
        !wildcard.is_empty(),
        "wildcard is not a referenceable local"
    );
}

#[test]
fn declaration_pattern_read_only_and_mutable_leaves_keep_usage_and_captures() {
    let source = "
fn read() -> i32 {
    var (left, right) = (1, 2);
    val bump = || { left += right; left };
    right = 3;
    bump()
}";
    let file = analyze_source(source);
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let diagnostics =
        check_source("fn bad() { val (left, right) = (1, 2); left = 3; right += 1; }");
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::InvalidAssignmentTarget { .. }
            ))
            .count(),
        2,
        "{diagnostics:?}"
    );
    let source = "fn read() -> i32 { val (left, _) = (1, 2); left }";
    let file = analyze_source(source);
    assert!(file.result().diagnostics().is_empty());
    assert!(
        file.queries()
            .visible_bindings(source.rfind("left").unwrap())
            .iter()
            .all(|binding| binding.declaration.name != "_")
    );
}

#[test]
fn declaration_pattern_canonical_leaf_order_uses_the_first_alternative() {
    let source = format!(
        "{POINT} fn read(input: (Point, i32)) -> i32 {{
            val (Point {{ y: second, x: first }}, (last | last)) = input;
            first + second + last
        }}"
    );
    let file = analyze_source(&source);
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let module = &file.result().facts().lowered.module;
    let block = module.block(module.functions[0].body.unwrap());
    let StmtKind::Binding { pattern, .. } = module.stmt(block.statements[0]).kind else {
        panic!("declaration");
    };
    let bindings = module.body.pattern_bindings(pattern);
    assert_eq!(
        bindings.iter().map(|(_, name)| *name).collect::<Vec<_>>(),
        ["second", "first", "last"]
    );
    for (local, name) in bindings {
        let span = file.result().facts().lowered.source_map.local_span(local);
        assert_eq!(&source[span.start..span.end], name);
        assert_eq!(
            file.queries().type_at(source.rfind(name).unwrap()),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
    }
}
