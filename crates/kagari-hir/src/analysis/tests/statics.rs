use crate::{
    AnalysisResult, AnalyzedModule,
    analysis::eligibility::BodyEligibility,
    analyze_source,
    declarations::DeclarationId,
    hir::{
        expr::ExprKind,
        ids::{BodyOwner, HirOwner},
        item::Item,
    },
    resolver::resolved::ResolvedName,
    tests::test_analysis,
    types::TypeId,
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId};
use kagari_source::{
    diagnostic::DiagnosticKind,
    identity::FileId,
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};

fn analyze(text: &str) -> AnalysisResult<AnalyzedModule> {
    analyze_source(
        &SourceFile::new("static.kgr", text),
        kagari_stdlib::catalog::shared().unwrap(),
    )
    .unwrap()
}

fn insert(sources: &mut SourceDatabase, name: &str, text: &str) -> FileId {
    let path = format!("mem://{name}");
    sources
        .bind_module(
            &path,
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: name.split("::").map(str::to_owned).collect(),
            },
        )
        .unwrap();
    sources.set(&path, text.into(), SourceLayer::Base).unwrap()
}

#[test]
fn runtime_static_initializers_have_distinct_owners_and_ordinary_inference() {
    let text = include_str!("../../../tests/fixtures/analysis/statics/runtime_initializers.kgr");
    let result = analyze(text);
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    let facts = result.facts();
    let module = &facts.lowered.module;
    assert_eq!(module.statics.len(), 4);
    assert_eq!(facts.typed.const_values.len(), 1);
    for item in &module.statics {
        let owner = HirOwner::Body(BodyOwner::Static(item.id));
        assert_eq!(item.initializer.owner(), owner);
        assert_eq!(item.ty.unwrap().owner(), owner);
        assert!(
            facts
                .typed
                .type_table
                .expr_type(item.initializer)
                .unwrap()
                .is_concrete()
        );
        assert_eq!(
            facts.typed.signatures.statics()[&item.id],
            facts.signatures.facts().statics()[&item.id]
        );
        assert!(module.items.contains(&Item::Static(item.id)));
        let declaration = facts
            .declarations
            .target(ResolvedName::Static(item.id))
            .unwrap();
        let DeclarationId::Definition(id) = declaration.id else {
            panic!("static definition")
        };
        assert_eq!(
            facts
                .declarations
                .definitions()
                .resolve(id)
                .unwrap()
                .to_path()
                .path
                .last()
                .unwrap()
                .kind,
            DefinitionKind::Static
        );
    }
    let captures: Vec<_> = module
        .nodes
        .expressions()
        .filter(|(_, expression)| matches!(expression.kind, ExprKind::Closure { .. }))
        .map(|(id, _)| facts.names.closure_captures(id))
        .collect();
    assert_eq!(captures.len(), 2);
    assert!(captures[0].is_empty());
    assert!(matches!(captures[1], [ResolvedName::Local(_)]));
}

#[test]
fn static_cycles_and_runtime_traps_are_checked_without_const_evaluation() {
    let result = analyze(include_str!(
        "../../../tests/fixtures/analysis/statics/cycles_and_runtime_traps.kgr"
    ));
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    assert!(result.facts().typed.const_values.is_empty());
    let forbidden = analyze("static val state: i32 = 1; const BAD: i32 = state;");
    assert!(
        forbidden
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::InvalidConstInitializer { .. }))
    );
}

#[test]
fn static_annotations_initializers_and_rebinding_are_fully_checked() {
    for (text, expected) in [
        (
            "static val x: i32 = true;",
            "KG_TYPE_ASSIGNMENT_TYPE_MISMATCH",
        ),
        ("static var x: Missing = 1;", "KG_TYPE_UNKNOWN_ANNOTATION"),
        ("static val x: _ = 1;", "KG_TYPE_UNKNOWN_ANNOTATION"),
        (
            "struct Box<T> { val x: T } static val x: Box = Box { x: 1 };",
            "KG_TYPE_UNKNOWN_ANNOTATION",
        ),
        (
            "static val x: i32 = 1; fn bad() { x = 2; }",
            "KG_TYPE_INVALID_ASSIGNMENT_TARGET",
        ),
        (
            "static var x: i32 = 1; fn bad() { x = true; }",
            "KG_TYPE_ASSIGNMENT_TYPE_MISMATCH",
        ),
        ("static val x: i32 = missing();", "KG_RESOLVE_UNKNOWN_NAME"),
    ] {
        let result = analyze(text);
        assert!(result.clone().into_codegen().is_err(), "accepted {text}");
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == expected),
            "{text}: {:?}",
            result.diagnostics()
        );
    }
    for text in [
        "static val x = 1;",
        "static val x: i32;",
        "static val x: i32 = ;",
    ] {
        assert!(analyze(text).into_codegen().is_err(), "accepted {text}");
    }
}

#[test]
fn static_storage_contract_rejects_views_but_allows_callable_view_inputs() {
    let callbacks = analyze(
        "struct Point { val x: i32 }
         static val read: fn(Ref<Point>) -> i32 = |value| value.x;
         static val read_mut: fn(RefMut<Point>) -> i32 = |value| value.x;",
    );
    assert!(
        callbacks.diagnostics().is_empty(),
        "{:?}",
        callbacks.diagnostics()
    );
    assert!(callbacks.into_codegen().is_ok());

    let mut accepted = Vec::new();
    for view in ["Ref<Point>", "RefMut<Point>"] {
        let text = format!(
            "struct Point {{ val x: i32 }} static var saved: {view} = loop {{}};
             fn save(value: {view}) {{ saved = value; }}"
        );
        let result = analyze(&text);
        if result.clone().into_codegen().is_ok() {
            accepted.push(view);
            continue;
        }
        let annotation = result.facts().lowered.module.statics[0].ty.unwrap();
        let span = result.facts().lowered.source_map.type_span(annotation);
        assert!(
            result.diagnostics().iter().any(|diagnostic| {
                matches!(diagnostic.kind, DiagnosticKind::InvalidReference { .. })
                    && diagnostic.span == Some(span)
            }),
            "{view}: {:?}",
            result.diagnostics()
        );
    }
    assert!(accepted.is_empty(), "accepted stored views: {accepted:?}");
}

#[test]
fn static_storage_contract_applies_contextual_interface_coercion() {
    let declarations = "trait Read { fn read(self) -> i32; }
        struct Number { val value: i32 }
        impl Read for Number { fn read(self) -> i32 { self.value } }";
    // Check the existing block-tail path before the direct initializer boundary.
    for initializer in ["{ Number { value: 7 } }", "Number { value: 7 }"] {
        let result = analyze(&format!(
            "{declarations} static val reader: Read = {initializer};
             fn read() -> i32 {{ reader.read() }}"
        ));
        assert!(
            result.diagnostics().is_empty(),
            "{initializer}: {:?}",
            result.diagnostics()
        );
        let facts = result.facts();
        let item = &facts.lowered.module.statics[0];
        let site = match facts.lowered.module.expr(item.initializer).kind {
            ExprKind::Block(block) => facts.lowered.module.block(block).tail_expr.unwrap(),
            _ => item.initializer,
        };
        let coercion = facts
            .typed
            .type_table
            .interface_coercion(site)
            .expect("initializer must retain the checked interface conversion");
        assert!(matches!(coercion.concrete_type, TypeId::Struct(_)));
        assert_eq!(
            TypeId::Trait(coercion.interface_type.clone()),
            facts.typed.signatures.statics()[&item.id]
        );
        assert!(result.into_codegen().is_ok());
    }
}

#[test]
fn imported_static_mutability_visibility_and_duplicates_use_value_namespace() {
    let mut sources = SourceDatabase::default();
    insert(
        &mut sources,
        "state",
        "pub static val fixed: i32 = 1; static var hidden: i32 = 2;",
    );
    let immutable = insert(
        &mut sources,
        "immutable",
        "use pkg::state::fixed as alias; fn bad() { alias += 1; }",
    );
    let private = insert(
        &mut sources,
        "private",
        "fn bad() -> i32 { pkg::state::hidden }",
    );
    let duplicate = insert(
        &mut sources,
        "duplicate",
        "static val x: i32 = 1; fn x() {} ",
    );
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(
        snapshot
            .file(immutable)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::InvalidAssignmentTarget { .. }))
    );
    assert!(
        snapshot
            .file(private)
            .unwrap()
            .result()
            .clone()
            .into_codegen()
            .is_err()
    );
    assert!(
        snapshot
            .file(duplicate)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::DuplicateDeclaration { .. }))
    );
}

#[test]
fn selected_body_queries_check_all_static_initializers_without_checking_neighbors() {
    let mut sources = SourceDatabase::default();
    let text = "static val state: i32 = { fn helper() -> i32 { true } helper() }; static val wrong: i32 = true; fn neighbor() -> i32 { missing } fn good() -> i32 { 1 }";
    let file = insert(&mut sources, "selected", text);
    let mut db = test_analysis();
    let declarations = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let good = declarations
        .file(file)
        .unwrap()
        .declarations()
        .iter()
        .find(|d| d.name == "good")
        .unwrap();
    let DeclarationId::Definition(owner) = good.id else {
        panic!("function")
    };
    let body = db
        .body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(
        body.diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::AssignmentTypeMismatch { .. }))
    );
    assert!(body.diagnostics().iter().any(|d| matches!(&d.kind, DiagnosticKind::ReturnTypeMismatch { function_name, .. } if function_name == "helper")));
    assert!(
        !body
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::UnknownName { .. }))
    );
    for item in &body.lowered().module.statics {
        assert!(body.type_table().expr_type(item.initializer).is_some());
    }
}

#[test]
fn imported_static_contract_edits_invalidate_cached_body_facts() {
    let mut sources = SourceDatabase::default();
    insert(&mut sources, "state", "pub static var value: i32 = 1;");
    let root = insert(
        &mut sources,
        "root",
        "use pkg::state::value; fn run() -> i32 { value = 2; value }",
    );
    let mut db = test_analysis();
    let first = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(first.file(root).unwrap().result().diagnostics().is_empty());
    sources
        .set(
            "mem://state",
            "pub static val value: i32 = 1;".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let second = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(
        second
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::InvalidAssignmentTarget { .. }))
    );
    sources
        .set(
            "mem://state",
            "pub static var value: bool = true;".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let third = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(
        third
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(
                d.kind,
                DiagnosticKind::AssignmentTypeMismatch { .. }
                    | DiagnosticKind::ReturnTypeMismatch { .. }
            ))
    );
}

#[test]
fn static_initializer_binding_usage_retains_positive_evidence() {
    let mut sources = SourceDatabase::default();
    let file = insert(
        &mut sources,
        "usage",
        "static val good: i32 = { val used = 2; used }; static val bad: i32 = true;",
    );
    let mut db = test_analysis();
    db.set_max_semantic_diagnostics(0);
    let snapshot = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = snapshot.file(file).unwrap();
    let statics = &file.result().facts().lowered.module.statics;
    assert_eq!(
        file.static_initializer_eligibility(statics[0].id),
        BodyEligibility::Complete
    );
    assert_eq!(
        file.static_initializer_eligibility(statics[1].id),
        BodyEligibility::Unavailable
    );
}
