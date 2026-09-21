use super::*;
use crate::{
    builtin::BuiltinFunction, declarations::DeclarationId, hir::ExprKind, resolver::ResolvedName,
    typeck::CallTarget, types::BuiltinType,
};
use kagari_common::source_database::{SourceDatabase, SourceLayer};

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
    let result = AnalysisDatabase::default()
        .snapshot(
            sources.snapshot(),
            crate::LanguageFeatureProfile {
                allow_host_calls: true,
                ..Default::default()
            },
            &Default::default(),
        )
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
            facts.typed.type_table.call_resolution(call).unwrap().target,
            CallTarget::RuntimeHelper(helper)
        );
        assert!(
            file.result()
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_TYPE_CALL_ARITY_MISMATCH")
        );
        assert_eq!(
            file.type_at(text.rfind(" x }").unwrap() + 1),
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
            facts.typed.type_table.call_resolution(call).unwrap().target,
            CallTarget::Function(_)
        ));
        assert!(file.definition_at(text.rfind(name).unwrap()).is_some());

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
fn non_value_names_are_rejected_in_hir_while_retaining_targets() {
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
        "std::math::clamp",
        "std::math",
    ] {
        let text = format!(
            "fn answer() -> i32 {{ 42 }} struct Point {{}} enum Mode {{ Ready }} trait View {{}} fn bad() {{ val value = {name}; }} fn good(x: i32) -> i32 {{ x }}"
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
        assert!(facts.names.expr_resolution(value).is_some(), "{name}");
        assert_eq!(facts.typed.type_table.expr_type(value), Some(TypeId::Error));
        assert!(
            file.result()
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_TYPE_INVALID_VALUE_TARGET"),
            "{name}: {:?}",
            file.result().diagnostics()
        );
        assert!(
            !file
                .result()
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_RESOLVE_UNKNOWN_NAME")
        );
        assert!(
            file.definition_at(text.rfind(" x }").unwrap() + 1)
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

#[test]
fn helper_calls_rebase_on_body_reuse_and_invalidate_on_shadowing() {
    let text = "fn first() -> i32 { 1 } fn keep() -> String { type_of(7) }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("cache.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let headers = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let DeclarationId::Definition(owner) = &headers
        .file(file)
        .unwrap()
        .declarations()
        .iter()
        .find(|d| d.name == "keep")
        .unwrap()
        .id
    else {
        panic!("function");
    };
    let old = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(old.diagnostics().is_empty());
    let old_call = old
        .lowered()
        .module
        .body
        .expressions()
        .find(|(id, _)| old.type_table().call_resolution(*id).is_some())
        .unwrap()
        .0;
    let edited = text.replace("{ 1 }", "{ val n = 2; n }");
    sources
        .set("cache.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let reused = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(reused.reused_bodies(), 1);
    assert!(reused.type_table().call_resolution(old_call).is_none());
    let fresh = AnalysisDatabase::default()
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    reused.type_table().assert_same_source_facts(
        fresh.type_table(),
        reused.lowered().module.body.arena(),
        fresh.lowered().module.body.arena(),
    );
    sources
        .set(
            "cache.kgr",
            format!("{edited} fn type_of(x: i32) -> i32 {{ x }}"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let shadowed = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(shadowed.reused_bodies(), 0);
    assert!(
        shadowed
            .diagnostics()
            .iter()
            .any(|d| d.kind.code() == "KG_TYPE_RETURN_TYPE_MISMATCH")
    );
    assert_eq!(
        old.type_table().call_resolution(old_call).unwrap().target,
        CallTarget::RuntimeHelper(BuiltinFunction::TypeOf)
    );
}

#[test]
fn explicit_host_declarations_take_precedence_over_the_helper_prelude() {
    use kagari_common::host_interface::{HostFunctionDeclaration, HostInterface, HostValueType};
    for (name, _) in HELPERS {
        let mut sources = SourceDatabase::default();
        let text = format!("fn main() -> i32 {{ {name}() }}");
        let file = sources.set("host.kgr", text, SourceLayer::Base).unwrap();
        let mut db = AnalysisDatabase::default();
        let hosts = crate::host::HostDeclarations::new(HostInterface {
            field_paths: vec![],
            types: Vec::new(),
            functions: vec![HostFunctionDeclaration::new(
                name,
                vec![],
                HostValueType::I32,
            )],
        })
        .unwrap();
        let expected = hosts.resolve(name).unwrap();
        db.set_host_declarations(hosts);
        let snapshot = db
            .snapshot(
                sources.snapshot(),
                crate::LanguageFeatureProfile {
                    allow_host_calls: true,
                    ..Default::default()
                },
                &Default::default(),
            )
            .unwrap();
        let file = snapshot.file(file).unwrap();
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
        let facts = file.result().facts();
        for (id, expr) in facts.lowered.module.body.expressions() {
            match expr.kind {
                ExprKind::Name { .. } => assert_eq!(
                    facts.names.expr_resolution(id),
                    Some(ResolvedName::HostFunction(expected))
                ),
                ExprKind::Call { .. } => assert_eq!(
                    facts.typed.type_table.call_resolution(id).unwrap().target,
                    CallTarget::HostFunction(expected)
                ),
                _ => {}
            }
        }
    }
}
