use super::*;
use crate::{
    hir::expr::ExprKind,
    imports::{DirectiveResolution, SourceItem, bindings::LookupOutcome},
    resolver::{resolved::ResolvedName, table::NameResolution},
    tests::native,
    typeck::scalar::ScalarValue,
};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    declaration::names::NameNamespace,
    scalar::{BuiltinType, IntegerType},
    ty::Ty,
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
    sources
        .bind_module(
            "root.kgr",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["root".into()],
            },
        )
        .unwrap();
    let file = sources
        .set("root.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = native::database();
    let mut modules = kagari_stdlib::catalog::shared().unwrap();
    modules.push(native::module());
    modules.push(native::function_module(
        ModuleIdentity {
            package: PackageId("demo".into()),
            path: vec![],
        },
        "number",
        vec![],
        Ty::Builtin(BuiltinType::I32),
    ));
    db.set_native_modules(modules);
    (sources, db, file)
}

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn imported_constant_aliases_publish_checked_values_and_original_identity() {
    for text in [
        "use self::labels::CODE as code; mod labels { pub const CODE: i32 = 7; } fn main() -> i32 { code }",
        "use self::labels::CODE as code; mod labels { const BASE: i32 = 6; pub const CODE = BASE + 1; } fn main() -> i32 { code }",
        "mod labels { pub const CODE: i32 = 7; } mod relay { pub use super::labels::CODE as EXPORTED; } use self::relay::EXPORTED as code; fn main() -> i32 { code }",
    ] {
        let (sources, mut db, file) = setup(text);
        let snapshot = snapshot(&mut db, &sources);
        let analysis = snapshot.file(file).unwrap();
        assert!(
            analysis.result().diagnostics().is_empty(),
            "{:?}",
            analysis.result().diagnostics()
        );
        let facts = analysis.result().facts();
        let (id, _) = facts
            .lowered
            .module
            .body
            .expressions()
            .find(|(_, expr)| matches!(&expr.kind, ExprKind::Name { name, .. } if name == "code"))
            .unwrap();
        let Some(ResolvedName::Source(source)) = facts.names.expr_resolution(id) else {
            panic!("qualified imported identity");
        };
        let SourceItem::Const(constant) = source.item else {
            panic!("constant owner");
        };
        let owner = snapshot.file(source.unit.file).unwrap().result().facts();
        assert_eq!(owner.lowered.module.constant(constant).name, "CODE");
        assert_eq!(
            facts.typed.type_table.scalar_value(id),
            owner.typed.const_values.get(&constant)
        );
        assert_eq!(
            facts.typed.type_table.scalar_value(id),
            Some(&ScalarValue::integer(7, IntegerType::I32).unwrap())
        );
        let declaration = analysis.definition_at(text.rfind("code").unwrap()).unwrap();
        assert_eq!(declaration.name, "CODE");
        assert_eq!(
            declaration.id,
            owner
                .declarations
                .target(ResolvedName::Const(constant))
                .unwrap()
                .id
        );
    }
}

#[test]
fn imported_constant_edits_invalidate_value_facts_without_retargeting_aliases() {
    let text = "use pkg::library::CODE as code; fn main() -> i32 { code }";
    let (mut sources, mut db, file) = setup(text);
    sources
        .set(
            "library.kgr",
            "pub const CODE: i32 = 7;".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let before = snapshot(&mut db, &sources);
    let old = before.file(file).unwrap();
    assert!(
        old.result().diagnostics().is_empty(),
        "{:?}",
        old.result().diagnostics()
    );
    let old_facts = old.result().facts();
    let (id, _) = old_facts
        .lowered
        .module
        .body
        .expressions()
        .find(|(_, expr)| matches!(&expr.kind, ExprKind::Name { name, .. } if name == "code"))
        .unwrap();
    let original = old
        .definition_at(text.rfind("code").unwrap())
        .unwrap()
        .id
        .clone();
    let owner = *old_facts
        .declarations
        .definition(ResolvedName::Function(
            old_facts.lowered.module.functions[0].id,
        ))
        .unwrap();
    let selected_old = db
        .body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(
        selected_old.diagnostics().is_empty(),
        "{:?}",
        selected_old.diagnostics()
    );
    assert_eq!(
        selected_old.type_table().scalar_value(id),
        Some(&ScalarValue::integer(7, IntegerType::I32).unwrap())
    );
    sources
        .set(
            "library.kgr",
            "pub const CODE: i32 = 9;".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let after = snapshot(&mut db, &sources);
    let new = after.file(file).unwrap();
    assert!(
        new.result().diagnostics().is_empty(),
        "{:?}",
        new.result().diagnostics()
    );
    assert_eq!(
        old_facts.typed.type_table.scalar_value(id),
        Some(&ScalarValue::integer(7, IntegerType::I32).unwrap())
    );
    assert_eq!(
        new.result().facts().typed.type_table.scalar_value(id),
        Some(&ScalarValue::integer(9, IntegerType::I32).unwrap())
    );
    assert_eq!(
        new.definition_at(text.rfind("code").unwrap()).unwrap().id,
        original
    );
    let selected_new = db
        .body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(
        selected_new.diagnostics().is_empty(),
        "{:?}",
        selected_new.diagnostics()
    );
    assert_eq!(
        selected_new.type_table().scalar_value(id),
        Some(&ScalarValue::integer(9, IntegerType::I32).unwrap())
    );
    assert_eq!(
        selected_old.type_table().scalar_value(id),
        Some(&ScalarValue::integer(7, IntegerType::I32).unwrap())
    );
}

#[test]
fn imported_constants_preserve_visibility_immutability_and_invalid_owner_rejection() {
    for (provider, body) in [
        ("const CODE: i32 = 7;", "fn main() -> i32 { code }"),
        ("pub const CODE: i32 = 7;", "fn main() { code = 8; }"),
        ("pub const CODE: i32 = 1 / 0;", "fn main() -> i32 { code }"),
        ("pub const CODE: i32 = true;", "fn main() -> i32 { code }"),
    ] {
        let text = format!("use pkg::library::CODE as code; {body}");
        let (mut sources, mut db, file) = setup(&text);
        sources
            .set("library.kgr", provider.into(), SourceLayer::Overlay)
            .unwrap();
        let result = snapshot(&mut db, &sources);
        let analysis = result.file(file).unwrap();
        assert!(
            analysis.result().clone().into_codegen().is_err(),
            "{provider}: {body}"
        );
    }
}

#[test]
fn qualified_standard_source_and_host_calls_use_type_prefixes() {
    let cases = [
        ("use demo::native as api;", "api::choose(1, 1, 1)", "api"),
        ("use demo::native;", "native::choose(1, 1, 1)", "native"),
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
        assert!(facts.names.expr_resolution(names[1].0).is_some(), "{text}");
        assert!(facts.names.expr_resolution(names[2].0).is_some(), "{text}");
        let calls = facts
            .lowered
            .module
            .body
            .expressions()
            .filter(|(_, expr)| matches!(expr.kind, ExprKind::Call { .. }))
            .collect::<Vec<_>>();
        assert!(
            facts.typed.type_table.call_resolution(calls[1].0).is_some(),
            "{text}"
        );
        assert_eq!(
            analysis.type_at(text.find(call).unwrap()),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(analysis.result().clone().into_codegen().is_ok(), "{text}");
    }
}

#[test]
fn qualified_call_navigation_selects_only_the_terminal_name() {
    let text = "// 中文 😀\r\nuse pkg::library as api; fn source() -> i32 { api::number() } fn host() -> i32 { demo::number() }";
    let (sources, mut db, file) = setup(text);
    let snapshot = snapshot(&mut db, &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );

    let source_call = text.find("api::number()").unwrap();
    for offset in source_call..source_call + "api::".len() {
        assert!(
            analysis.source_function_at(offset).is_none(),
            "offset {offset}"
        );
        assert!(
            snapshot.definition_at(file, offset).is_none(),
            "offset {offset}"
        );
    }
    let source_name = source_call + "api::".len();
    assert_eq!(
        analysis
            .source_function_at(source_name)
            .unwrap()
            .signature
            .name,
        "number"
    );
    assert_eq!(
        snapshot.definition_at(file, source_name).unwrap().name,
        "number"
    );

    let host_call = text.find("demo::number()").unwrap();
    for offset in host_call..host_call + "demo::".len() {
        assert!(
            analysis.source_function_at(offset).is_none(),
            "offset {offset}"
        );
    }
    let host_name = host_call + "demo::".len();
    assert_eq!(
        analysis
            .source_function_at(host_name)
            .unwrap()
            .signature
            .name,
        "number"
    );
    assert!(
        analysis
            .source_function_at(host_name + "number".len())
            .is_none()
    );
}

#[test]
fn absent_imports_release_categories_and_ambiguous_imports_block_fallback() {
    let cases = [
        ("use missing as print;", "print(\"blocked\")", "print", None),
        (
            "const print: i32 = 1; fn print() {}",
            "print(\"blocked\")",
            "print",
            Some(NameResolution::Ambiguous),
        ),
        (
            "use demo::native as api; use core::cmp as api;",
            "api::choose(1, 1, 1)",
            "api",
            Some(NameResolution::Ambiguous),
        ),
        (
            "use demo as api; use pkg::library as api;",
            "api::number()",
            "api",
            Some(NameResolution::Ambiguous),
        ),
        ("use missing as demo;", "demo::number()", "demo", None),
        (
            "use missing as native;",
            "native::choose(1, 1, 1)",
            "native",
            None,
        ),
        (
            "use demo::number; const number: i32 = 9;",
            "number()",
            "number",
            Some(NameResolution::Ambiguous),
        ),
    ];
    for (imports, call, alias, expected) in cases {
        let text = format!("{imports} fn bad() -> i32 {{ {call} }} fn good() -> i32 {{ 4 }}");
        let (sources, mut db, file) = setup(&text);
        let snapshot = snapshot(&mut db, &sources);
        let analysis = snapshot.file(file).unwrap();
        let facts = analysis.result().facts();
        if let Some(expected) = &expected {
            assert_eq!(
                facts.names.items.lookup(
                    alias,
                    if alias == "print" || alias == "number" {
                        NameNamespace::Value
                    } else {
                        NameNamespace::Type
                    }
                ),
                Some(expected.clone()),
                "{text}"
            );
        } else {
            assert!(facts.names.imports.directives.iter().any(|directive|
                matches!(&directive.resolution, DirectiveResolution::Named(outcomes)
                    if outcomes.types == LookupOutcome::Absent && outcomes.values == LookupOutcome::Absent)));
        }
        for (id, expr) in facts
            .lowered
            .module
            .body
            .expressions()
            .filter(|_| expected.is_some())
        {
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
                .directives
                .iter()
                .take(facts.lowered.module.imports.len())
                .all(|import| import.resolution.target().is_none())
        );
        assert!(
            facts
                .declarations
                .resolve_name(ty, NameNamespace::Type)
                .is_none()
        );
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
