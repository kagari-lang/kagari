use crate::AnalyzerQueries;
use kagari_hir::{analysis::*, types::*};
use kagari_hir::{
    builtin::BuiltinFunction, hir::expr::ExprKind, resolver::resolved::ResolvedName,
    typeck::table::CallTarget,
};
use kagari_source::identity::*;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::BuiltinType;

const HELPERS: [(&str, BuiltinFunction); 5] = [
    ("print", BuiltinFunction::Print),
    ("type_of", BuiltinFunction::TypeOf),
    ("get_field", BuiltinFunction::GetField),
    ("set_field", BuiltinFunction::SetField),
    ("set_index", BuiltinFunction::SetIndex),
];

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
fn helper_targets_survive_invalid_arguments_without_reinterpreting_names() {
    for (name, helper) in HELPERS {
        let text = format!("fn bad() {{ {name}(); }} fn good(x: i32) -> i32 {{ x }}");
        let (snapshot, file) = analyze(&text);
        let file = snapshot.file(file).unwrap();
        let facts = file.result().facts();
        let (callee, _) = facts
            .lowered
            .module
            .body
            .expressions()
            .find(|(_, expr)| matches!(&expr.kind, ExprKind::Name { name: n, .. } if n == name))
            .unwrap();
        assert_eq!(
            facts.names.expr_resolution(callee),
            Some(ResolvedName::RuntimeHelper(helper))
        );
        let (call, _) = facts
            .lowered
            .module
            .body
            .expressions()
            .find(|(_, expr)| matches!(expr.kind, ExprKind::Call { .. }))
            .unwrap();
        assert_eq!(
            facts
                .typed
                .type_table()
                .call_resolution(call)
                .unwrap()
                .target,
            CallTarget::RuntimeHelper(helper)
        );
        assert!(
            file.result()
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_TYPE_CALL_ARITY_MISMATCH")
        );
        assert_eq!(
            file.queries().type_at(text.rfind(" x }").unwrap() + 1),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(file.result().clone().into_codegen().is_err());
    }
}

#[test]
fn declarations_and_lexical_bindings_shadow_every_helper() {
    for (name, _) in HELPERS {
        let text = format!("fn {name}(x: i32) -> i32 {{ x }} fn main() -> i32 {{ {name}(4) }}");
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
            .body
            .expressions()
            .find(|(_, expr)| matches!(expr.kind, ExprKind::Call { .. }))
            .unwrap();
        assert!(matches!(
            facts
                .typed
                .type_table()
                .call_resolution(call)
                .unwrap()
                .target,
            CallTarget::Function(_)
        ));
        assert!(
            file.queries()
                .definition_at(text.rfind(name).unwrap())
                .is_some()
        );

        let text = format!("fn bad({name}: i32) {{ {name}(4); }}");
        let (snapshot, file) = analyze(&text);
        let file = snapshot.file(file).unwrap();
        assert!(
            file.result()
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_TYPE_INVALID_CALL_TARGET")
        );
        let facts = file.result().facts();
        let (callee, _) = facts
            .lowered
            .module
            .body
            .expressions()
            .find(|(_, expr)| matches!(expr.kind, ExprKind::Name { .. }))
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
        "print",
        "type_of",
        "get_field",
        "set_field",
        "set_index",
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
            .body
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
