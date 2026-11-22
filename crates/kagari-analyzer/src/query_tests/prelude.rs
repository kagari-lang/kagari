use crate::AnalyzerQueries;
use kagari_common::span::Span;
use kagari_hir::{analysis::*, types::*};
use kagari_hir::{
    hir::expr::ExprKind, resolver::resolved::ResolvedName, typeck::table::CallTarget,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    identity::FileId,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::scalar::BuiltinType;

const FORMER_HELPERS: [&str; 4] = ["type_of", "get_field", "set_field", "set_index"];

fn analyze(text: &str) -> (AnalysisSnapshot, FileId) {
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("prelude.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let result = crate::query_tests::native::database()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    (result, file)
}

#[test]
fn undeclared_former_helpers_report_an_unknown_callee() {
    for name in FORMER_HELPERS {
        let text = format!("fn bad() {{ {name}(1); }} fn good(x: i32) -> i32 {{ x }}");
        let (snapshot, file) = analyze(&text);
        let file = snapshot.file(file).unwrap();
        let facts = file.result().facts();
        let (callee, _) = facts
            .lowered
            .module
            .nodes
            .expressions()
            .find(|(_, expr)| matches!(&expr.kind, ExprKind::Name { name: n, .. } if n == name))
            .unwrap();
        assert_eq!(facts.names.expr_resolution(callee), None);
        let (call, _) = facts
            .lowered
            .module
            .nodes
            .expressions()
            .find(|(_, expr)| matches!(expr.kind, ExprKind::Call { .. }))
            .unwrap();
        assert!(facts.typed.type_table().call_resolution(call).is_none());
        let offset = text.find(name).unwrap();
        let diagnostics = file.result().diagnostics();
        assert_eq!(diagnostics.len(), 1, "{name}: {diagnostics:?}");
        assert_eq!(
            diagnostics[0].kind,
            DiagnosticKind::UnknownName { name: name.into() }
        );
        assert_eq!(
            diagnostics[0].span,
            Some(Span::new(offset, offset + name.len()))
        );
        assert!(file.queries().definition_at(offset).is_none());
        assert_eq!(
            file.queries().type_at(text.rfind(" x }").unwrap() + 1),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(file.result().clone().into_codegen().is_err());
    }
}

#[test]
fn former_helper_spellings_resolve_as_ordinary_functions_and_parameters() {
    for name in FORMER_HELPERS {
        let declaration = format!("fn {name}(x: i32) -> i32 {{ x }}");
        let text = format!("{declaration} fn main() -> i32 {{ {name}(4) }}");
        let (snapshot, file) = analyze(&text);
        let file = snapshot.file(file).unwrap();
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
        let facts = file.result().facts();
        let (call, _) = facts
            .lowered
            .module
            .nodes
            .expressions()
            .find(|(_, expr)| matches!(expr.kind, ExprKind::Call { .. }))
            .unwrap();
        let function = facts
            .lowered
            .module
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        assert_eq!(
            facts
                .typed
                .type_table()
                .call_resolution(call)
                .unwrap()
                .target,
            CallTarget::Function(function.id)
        );
        let declaration_site = file
            .queries()
            .definition_at(text.find(name).unwrap())
            .unwrap();
        assert_eq!(
            file.queries().definition_at(text.rfind(name).unwrap()),
            Some(declaration_site)
        );
        assert!(file.result().clone().into_codegen().is_ok());

        let text = format!("{declaration} fn bad({name}: i32) {{ {name}(4); }}");
        let (snapshot, file) = analyze(&text);
        let file = snapshot.file(file).unwrap();
        let diagnostics = file.result().diagnostics();
        assert_eq!(diagnostics.len(), 1, "{name}: {diagnostics:?}");
        assert!(matches!(
            diagnostics[0].kind,
            DiagnosticKind::InvalidCallTarget { .. }
        ));
        let facts = file.result().facts();
        let (callee, _) = facts
            .lowered
            .module
            .nodes
            .expressions()
            .find(|(_, expr)| matches!(&expr.kind, ExprKind::Name { name: n, .. } if n == name))
            .unwrap();
        assert!(matches!(
            facts.names.expr_resolution(callee),
            Some(ResolvedName::Param(_))
        ));
    }
}

#[test]
fn invalid_values_and_type_only_names_are_rejected_without_losing_neighbor_targets() {
    for name in [
        "answer",
        "Point",
        "Mode",
        "View",
        "bindings::choose",
        "bindings",
    ] {
        let text = format!(
            "use demo::bindings; fn answer() -> i32 {{ 42 }} struct Point {{}} enum Mode {{ Ready }} trait View {{}} fn bad() {{ val value = {name}; }} fn good(x: i32) -> i32 {{ x }}"
        );
        let (snapshot, file) = analyze(&text);
        let file = snapshot.file(file).unwrap();
        let facts = file.result().facts();
        let (value, _) = facts
            .lowered
            .module
            .nodes
            .expressions()
            .find(|(_, expr)| matches!(&expr.kind, ExprKind::Name { name: n, .. } if n == name))
            .unwrap();
        let type_only = matches!(name, "Point" | "Mode" | "View" | "bindings");
        let expected_code = if type_only {
            "KG_RESOLVE_UNKNOWN_NAME"
        } else {
            "KG_TYPE_INVALID_VALUE_TARGET"
        };
        assert_eq!(
            facts.names.expr_resolution(value).is_none(),
            type_only,
            "{name}"
        );
        assert_eq!(
            facts.typed.type_table().expr_type(value),
            Some(TypeId::Error)
        );
        assert!(
            file.result()
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == expected_code),
            "{name}: {:?}",
            file.result().diagnostics()
        );
        assert_eq!(
            file.result()
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_RESOLVE_UNKNOWN_NAME"),
            type_only
        );
        assert!(
            file.queries()
                .definition_at(text.rfind(" x }").unwrap() + 1)
                .is_some()
        );
        assert!(file.result().clone().into_codegen().is_err());
    }
    let (snapshot, file) = analyze("fn bad() { missing; }");
    assert!(
        snapshot
            .file(file)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| d.kind.code() == "KG_RESOLVE_UNKNOWN_NAME")
    );
}
