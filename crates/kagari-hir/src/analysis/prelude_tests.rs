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
            paths: vec![],
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

#[test]
fn reflection_helpers_reject_unknown_members_and_invalid_index_targets_in_hir() {
    for (body, expected_code) in [
        ("get_field(point, \"missing\");", "KG_RESOLVE_UNKNOWN_NAME"),
        (
            "set_field(point, \"missing\", 7);",
            "KG_RESOLVE_UNKNOWN_NAME",
        ),
        ("get_field(1, \"x\");", "KG_RESOLVE_UNKNOWN_NAME"),
        ("set_index(1, 0, 7);", "KG_TYPE_INVALID_INDEX_TARGET"),
        ("set_index([1], true, 7);", "KG_TYPE_INVALID_INDEX_TARGET"),
        (
            "set_index((1, true), 2, 7);",
            "KG_TYPE_INVALID_INDEX_TARGET",
        ),
    ] {
        let source = SourceFile::new(
            "invalid-reflection.kgr",
            format!(
                "struct Point {{ var x: i32 }} fn main() {{ val point = Point {{ x: 1 }}; {body} }}"
            ),
        );
        let analysis = crate::analyze_source(
            &source,
            crate::LanguageFeatureProfile {
                allow_reflection: true,
                allow_reflection_write: true,
                ..Default::default()
            },
        );
        assert!(
            analysis
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == expected_code),
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_err(), "{body}");
    }
}

#[test]
fn reflection_helpers_do_not_cascade_errors_from_unknown_operands() {
    for body in [
        "get_field(missing, \"x\");",
        "set_field(missing, \"x\", 7);",
        "set_index(missing, 0, 7);",
        "set_index([1], missing, 7);",
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new("recovery-reflection.kgr", format!("fn main() {{ {body} }}")),
            crate::LanguageFeatureProfile {
                allow_reflection: true,
                allow_reflection_write: true,
                ..Default::default()
            },
        );
        assert_eq!(
            analysis.diagnostics().len(),
            1,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert!(matches!(&analysis.diagnostics()[0].kind,
            kagari_common::DiagnosticKind::UnknownName { name } if name == "missing"));
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn reflective_field_writes_check_declared_writeability_and_keep_rhs_context() {
    for (binding, expression, readonly, mismatch) in [
        ("val", "get_field(box, \"value\");", false, false),
        (
            "var",
            "set_field(box, \"value\", Marker { value: 42 });",
            false,
            false,
        ),
        (
            "val",
            "set_field(box, \"value\", Marker { value: 42 });",
            true,
            false,
        ),
        (
            "val",
            "set_field(box, \"value\", Marker<bool> { value: 42 });",
            true,
            true,
        ),
    ] {
        let source = SourceFile::new(
            "readonly-reflection.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} struct Box {{ {binding} value: Marker<i32> }} fn main() {{ val box = Box {{ value: Marker {{ value: 0 }} }}; {expression} }}"
            ),
        );
        let analysis = crate::analyze_source(
            &source,
            crate::LanguageFeatureProfile {
                allow_reflection: true,
                allow_reflection_write: true,
                ..Default::default()
            },
        );
        assert_eq!(
            analysis.diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::InvalidAssignmentTarget { .. }
            )),
            readonly,
            "{expression}"
        );
        assert_eq!(
            analysis.diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::AssignmentTypeMismatch { .. }
            )),
            mismatch,
            "{expression}"
        );
        assert_eq!(
            analysis.diagnostics().len(),
            usize::from(readonly) + usize::from(mismatch),
            "{:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), !readonly);
    }
}

#[test]
fn invalid_reflection_names_keep_helper_targets_and_precise_argument_diagnostics() {
    for (helper, expression, code, count) in [
        (
            BuiltinFunction::GetField,
            "get_field(point, name)",
            "KG_TYPE_REFLECTION_FIELD_NAME_NOT_CONSTANT",
            1,
        ),
        (
            BuiltinFunction::GetField,
            "get_field(point, true)",
            "KG_TYPE_ARGUMENT_TYPE_MISMATCH",
            1,
        ),
        (
            BuiltinFunction::GetField,
            "get_field(point, missing)",
            "KG_RESOLVE_UNKNOWN_NAME",
            1,
        ),
        (
            BuiltinFunction::SetField,
            "set_field(point, name, missing)",
            "KG_TYPE_REFLECTION_FIELD_NAME_NOT_CONSTANT",
            2,
        ),
        (
            BuiltinFunction::SetField,
            "set_field(point, true, 7)",
            "KG_TYPE_ARGUMENT_TYPE_MISMATCH",
            1,
        ),
        (
            BuiltinFunction::SetField,
            "set_field(point, missing, 7)",
            "KG_RESOLVE_UNKNOWN_NAME",
            1,
        ),
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new(
                "reflection-names.kgr",
                format!(
                    "struct Point {{ var x: i32 }} fn bad(name: String) {{ val point = Point {{ x: 1 }}; {expression}; }}"
                ),
            ),
            crate::LanguageFeatureProfile {
                allow_reflection: true,
                allow_reflection_write: true,
                ..Default::default()
            },
        );
        assert_eq!(
            analysis.diagnostics().len(),
            count,
            "{expression}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.diagnostics().iter().any(|d| d.kind.code() == code));
        let facts = analysis.facts();
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
        assert_eq!(facts.typed.type_table.expr_type(call), Some(TypeId::Error));
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn partial_index_errors_do_not_hide_known_noninteger_index_types() {
    for (body, invalid) in [
        ("values[(1, missing)];", true),
        ("set_index(values, (1, missing), 7);", true),
        ("values[missing];", false),
        ("set_index(values, missing, 7);", false),
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new(
                "partial-index.kgr",
                format!("fn bad(values: [i32]) {{ {body} }} fn good() -> i32 {{ 42 }}"),
            ),
            crate::LanguageFeatureProfile {
                allow_reflection: true,
                allow_reflection_write: true,
                ..Default::default()
            },
        );
        assert_eq!(
            analysis.diagnostics().len(),
            1 + usize::from(invalid),
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(
            analysis.diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::InvalidIndexTarget { .. }
            )),
            invalid,
            "{body}"
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn reflective_assignment_values_obey_normal_completion_without_hiding_target_errors() {
    for call in [
        r#"set_field(box, "value", VALUE)"#,
        "set_index(array, 0, VALUE)",
    ] {
        for (value, valid) in [
            ("if true { return 42; } else { return 7; }", true),
            ("if true { return 42; } else { 7 }", true),
            ("if true { return 42; } else { false }", false),
            ("if true { return false; } else { return 7; }", false),
        ] {
            let body = call.replace("VALUE", value);
            let analysis = crate::analyze_source(
                &SourceFile::new(
                    "reflection-completion.kgr",
                    format!(
                        "struct Box {{ var value: i32 }} fn run(box: Box, array: [i32]) -> i32 {{ {body}; 0 }}"
                    ),
                ),
                crate::LanguageFeatureProfile {
                    allow_reflection: true,
                    allow_reflection_write: true,
                    ..Default::default()
                },
            );
            assert_eq!(
                analysis.diagnostics().is_empty(),
                valid,
                "{body}: {:?}",
                analysis.diagnostics()
            );
            assert_eq!(analysis.into_codegen().is_ok(), valid, "{body}");
        }
    }
    for call in [
        r#"set_field(box, "value", VALUE)"#,
        "set_index(array, true, VALUE)",
    ] {
        let body = call.replace("VALUE", "if true { return 42; } else { return 7; }");
        let analysis = crate::analyze_source(
            &SourceFile::new(
                "reflection-target-completion.kgr",
                format!(
                    "struct Box {{ val value: i32 }} fn run(box: Box, array: [i32]) -> i32 {{ {body}; 0 }}"
                ),
            ),
            crate::LanguageFeatureProfile {
                allow_reflection: true,
                allow_reflection_write: true,
                ..Default::default()
            },
        );
        assert!(!analysis.diagnostics().is_empty(), "{body}");
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn reflection_receivers_must_produce_values_before_target_checks() {
    for (call, valid) in [
        (r#"get_field(BASE, "value")"#, true),
        (r#"set_field(BASE, "value", 7)"#, true),
        ("set_index(BASE, 0, 7)", true),
        (r#"set_field(BASE, "value", missing)"#, false),
        ("set_index(BASE, missing, 7)", false),
        ("get_field(BASE, 7)", false),
    ] {
        let call = call.replace("BASE", "if true { return 42; } else { return 7; }");
        let source = format!("fn main() -> i32 {{ {call}; 0 }}");
        let analysis = crate::analyze_source(
            &SourceFile::new("reflection-receiver-completion.kgr", source.clone()),
            crate::LanguageFeatureProfile {
                allow_reflection: true,
                allow_reflection_write: true,
                ..Default::default()
            },
        );
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{source}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid, "{source}");
    }
}
