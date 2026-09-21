use super::*;
use crate::{
    hir::ExprKind,
    resolver::{NameResolution, ResolvedName},
    types::BuiltinType,
};
use kagari_common::{
    host_interface::{HostFunctionDeclaration, HostInterface, HostValueType},
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};

fn setup(text: &str) -> (SourceDatabase, AnalysisDatabase, FileId) {
    let mut sources = SourceDatabase::default();
    sources
        .bind_module(
            "library.kgr",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["library".into()],
            },
        )
        .unwrap();
    sources
        .set(
            "library.kgr",
            "pub struct Point {} pub fn number() -> i32 { 8 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let file = sources
        .set("root.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    db.set_host_declarations(
        crate::host::HostDeclarations::new(HostInterface {
            field_paths: vec![],
            types: Vec::new(),
            functions: vec![HostFunctionDeclaration::new(
                "demo.number",
                vec![],
                HostValueType::I32,
            )],
        })
        .unwrap(),
    );
    (sources, db, file)
}

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(
        sources.snapshot(),
        crate::LanguageFeatureProfile {
            allow_host_calls: true,
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn qualified_standard_source_and_host_calls_respect_lexical_bindings() {
    let cases = [
        ("use std::math as api;", "api::clamp(1, 1, 1)", "api"),
        ("", "std::math::clamp(1, 1, 1)", "std"),
        ("use pkg::library as api;", "api::number()", "api"),
        ("use demo as api;", "api::number()", "api"),
        ("", "demo::number()", "demo"),
    ];
    for (imports, call, binding) in cases {
        let text = format!(
            "{imports} fn before() -> i32 {{ {call} }} fn shadow({binding}: i32) -> i32 {{ {call} }} fn after() -> i32 {{ {call} }}"
        );
        let (sources, mut db, file) = setup(&text);
        let snapshot = snapshot(&mut db, &sources);
        let analysis = snapshot.file(file).unwrap();
        let facts = analysis.result().facts();
        let names = facts
            .lowered
            .module
            .body
            .expressions()
            .filter(|(_, expr)| matches!(&expr.kind, ExprKind::Name { name, .. } if name.contains("::")))
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 3, "{text}");
        assert!(facts.names.expr_resolution(names[0].0).is_some(), "{text}");
        assert!(facts.names.expr_resolution(names[1].0).is_none(), "{text}");
        assert!(facts.names.expr_resolution(names[2].0).is_some(), "{text}");
        let calls = facts
            .lowered
            .module
            .body
            .expressions()
            .filter(|(_, expr)| matches!(expr.kind, ExprKind::Call { .. }))
            .collect::<Vec<_>>();
        assert!(
            facts.typed.type_table.call_resolution(calls[1].0).is_none(),
            "{text}"
        );
        assert_eq!(
            analysis.type_at(text.find(call).unwrap()),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(analysis.result().clone().into_codegen().is_err(), "{text}");
    }
}

#[test]
fn invalid_or_ambiguous_imports_never_leave_a_fallback_target() {
    let cases = [
        (
            "use missing as print;",
            "print(\"blocked\")",
            "print",
            NameResolution::Unresolved,
        ),
        (
            "const print: i32 = 1; fn print() {}",
            "print(\"blocked\")",
            "print",
            NameResolution::Ambiguous,
        ),
        (
            "use std::math as api; use std::array as api;",
            "api::clamp(1, 1, 1)",
            "api",
            NameResolution::Ambiguous,
        ),
        (
            "use demo as api; use pkg::library as api;",
            "api::number()",
            "api",
            NameResolution::Ambiguous,
        ),
        (
            "use missing as demo;",
            "demo::number()",
            "demo",
            NameResolution::Unresolved,
        ),
        (
            "use missing as std;",
            "std::math::clamp(1, 1, 1)",
            "std",
            NameResolution::Unresolved,
        ),
        (
            "use demo::number; const number: i32 = 9;",
            "number()",
            "number",
            NameResolution::Ambiguous,
        ),
    ];
    for (imports, call, alias, expected) in cases {
        let text = format!("{imports} fn bad() -> i32 {{ {call} }} fn good() -> i32 {{ 4 }}");
        let (sources, mut db, file) = setup(&text);
        let snapshot = snapshot(&mut db, &sources);
        let analysis = snapshot.file(file).unwrap();
        let facts = analysis.result().facts();
        assert_eq!(facts.names.items.lookup(alias), Some(expected), "{text}");
        for (id, expr) in facts.lowered.module.body.expressions() {
            if matches!(expr.kind, ExprKind::Call { .. }) {
                assert!(
                    facts.typed.type_table.call_resolution(id).is_none(),
                    "{text}"
                );
            }
            if matches!(expr.kind, ExprKind::Name { .. }) {
                assert!(facts.names.expr_resolution(id).is_none(), "{text}");
            }
        }
        assert_eq!(
            analysis.type_at(text.rfind("4 }").unwrap()),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(analysis.result().clone().into_codegen().is_err());
    }
}

#[test]
fn duplicate_type_imports_invalidate_all_alias_targets() {
    for imports in [
        "use pkg::library as api; use pkg::library as api;",
        "use pkg::library::Point as api; use pkg::library::Point as api;",
    ] {
        let ty = if imports.contains("::Point") {
            "api"
        } else {
            "api::Point"
        };
        let text = format!("{imports} fn bad(x: {ty}) {{ val y = {ty} {{}}; }}");
        let (sources, mut db, file) = setup(&text);
        let snapshot = snapshot(&mut db, &sources);
        let analysis = snapshot.file(file).unwrap();
        let facts = analysis.result().facts();
        assert!(
            facts
                .names
                .imports
                .entries
                .iter()
                .all(|import| import.target.is_none())
        );
        assert!(facts.declarations.imported_types().get(ty).is_none());
        assert_eq!(
            analysis.type_at(text.find("x: ").unwrap() + 3),
            Some(TypeId::Error)
        );
        assert!(
            analysis
                .definition_at(text.find("x: ").unwrap() + 3)
                .is_none()
        );
        assert!(analysis.result().clone().into_codegen().is_err());
    }
}

#[test]
fn local_bindings_can_shadow_ambiguous_module_names() {
    let text = "const clash: i32 = 1; fn clash() -> i32 { 2 } fn good(clash: i32) -> i32 { clash }";
    let (sources, mut db, file) = setup(text);
    let snapshot = snapshot(&mut db, &sources);
    let analysis = snapshot.file(file).unwrap();
    let facts = analysis.result().facts();
    let (id, _) = facts
        .lowered
        .module
        .body
        .expressions()
        .find(|(_, expr)| matches!(&expr.kind, ExprKind::Name { name, .. } if name == "clash"))
        .unwrap();
    assert!(matches!(
        facts.names.expr_resolution(id),
        Some(ResolvedName::Param(_))
    ));
    assert_eq!(
        analysis.type_at(text.rfind("clash }").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}
