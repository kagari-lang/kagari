use crate::analysis::*;
use crate::tests::standard::standard_modules;
use crate::types::*;
use crate::{
    declarations::DeclarationId, hir::expr::ExprKind, tests::test_analysis,
    typeck::table::CallTarget,
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn selected_ordinary_calls_follow_edited_source_and_signatures() {
    let text = "fn first() -> i32 { 1 } fn answer(x: i32) -> String { \"ok\" } fn keep() -> String { answer(7) }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("cache.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
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
        .nodes
        .expressions()
        .find(|(id, _)| old.type_table().call_resolution(*id).is_some())
        .unwrap()
        .0;
    let edited = text.replace("{ 1 }", "{ val n = 2; n }");
    sources
        .set("cache.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let current = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(current.type_table().call_resolution(old_call).is_none());
    let owner_path = old.definitions().resolve(*owner).unwrap().to_path();
    let mut fresh_database = test_analysis();
    let fresh_headers = fresh_database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let fresh_owner = fresh_headers
        .file(file)
        .unwrap()
        .declarations()
        .definitions()
        .lookup(&owner_path)
        .unwrap();
    let fresh = fresh_database
        .body(sources.snapshot(), &fresh_owner, &Default::default())
        .unwrap()
        .unwrap();
    current.type_table().assert_same_source_facts(
        current.definitions(),
        fresh.type_table(),
        fresh.definitions(),
        current.lowered().module.nodes.arena(),
        fresh.lowered().module.nodes.arena(),
    );
    sources
        .set(
            "cache.kgr",
            edited.replace("-> String { \"ok\" }", "-> i32 { x }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let changed = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(matches!(changed.diagnostics(), [diagnostic] if matches!(
        diagnostic.kind, DiagnosticKind::ReturnTypeMismatch { .. }
    )));
    let answer = old
        .lowered()
        .module
        .functions
        .iter()
        .find(|function| function.name == "answer")
        .unwrap();
    let old_resolution = old.type_table().call_resolution(old_call).unwrap();
    assert_eq!(old_resolution.target, CallTarget::Function(answer.id));
    assert_eq!(
        old_resolution.signature.as_ref().unwrap().return_type,
        TypeId::Builtin(BuiltinType::String)
    );
    assert!(old.diagnostics().is_empty());
}

#[test]
fn range_projection_helpers_are_private_to_the_core_ops_owner() {
    let mut sources = SourceDatabase::default();
    sources
        .bind_module(
            "ops.kgr",
            ModuleIdentity {
                package: PackageId("client".into()),
                path: vec!["ops".into()],
            },
        )
        .unwrap();
    let name = "range_bound";
    let file = sources
        .set(
            "ops.kgr",
            format!("use core::ops::{name} as private_start; fn {name}() -> i32 {{ 42 }} fn lower() -> i32 {{ {name}() }}"),
            SourceLayer::Base,
        )
        .unwrap();
    let mut db = AnalysisDatabase::default();

    let snapshot = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let diagnostics = snapshot.file(file).unwrap().result().diagnostics();
    assert!(
        matches!(diagnostics, [diagnostic] if matches!(&diagnostic.kind,
            DiagnosticKind::ImportNotPublic { path } if path == "core::ops::range_bound")),
        "{diagnostics:?}"
    );
}

#[test]
fn imported_native_function_with_a_former_helper_name_keeps_its_declaration() {
    let name = "get_field";
    let mut sources = SourceDatabase::default();
    let text = format!("use demo::{name}; fn main() -> i32 {{ {name}() }}");
    let file = sources.set("host.kgr", text, SourceLayer::Base).unwrap();
    let mut db = test_analysis();
    let module = crate::tests::native_fixture::function_module(
        ModuleIdentity {
            package: PackageId("demo".into()),
            path: vec![],
        },
        name,
        vec![],
        Ty::Builtin(BuiltinType::I32),
    );
    let expected = module
        .records()
        .definition(DefinitionKind::Function, name, module.definitions())
        .unwrap();
    let expected = module.definitions().resolve(expected).unwrap().to_path();
    let mut modules = standard_modules();
    modules.push(module);
    db.set_native_modules(modules);
    let snapshot = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = snapshot.file(file).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let facts = file.result().facts();
    let expected = file.definitions().lookup(&expected).unwrap();
    for (id, expr) in facts.lowered.module.nodes.expressions() {
        match expr.kind {
            ExprKind::Name { .. } => assert_eq!(
                facts
                    .imported_functions
                    .get(facts.names.expr_resolution(id).unwrap())
                    .unwrap()
                    .declaration,
                expected
            ),
            ExprKind::Call { .. } => assert_eq!(
                facts.typed.type_table.call_resolution(id).unwrap().target,
                CallTarget::SourceFunction(expected)
            ),
            _ => {}
        }
    }
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn partial_index_errors_do_not_hide_known_noninteger_index_types() {
    for (body, invalid) in [
        ("values[(1, missing)];", true),

        ("values[missing];", false),

    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new(
                "partial-index.kgr",
                format!("fn bad(values: Vec<i32>) {{ {body} }} fn good() -> i32 {{ 42 }}"),
            ),
            standard_modules(),
        )
        .expect("installed declaration analysis");
        assert_eq!(
            analysis.diagnostics().len(),
            1 + usize::from(invalid),
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(
            analysis
                .diagnostics()
                .iter()
                .any(|d| matches!(d.kind, DiagnosticKind::InvalidIndexTarget { .. })),
            invalid,
            "{body}"
        );
        assert!(analysis.into_codegen().is_err());
    }
}
*/

#[test]
fn selected_standard_variants_and_propagation_match_fresh_facts_after_an_edit() {
    let text = "fn first()->i32 { 1 } fn keep(x: Option<i32>)->Option<i32> { val y = Some(x?); match y { Some(n) => Some(n), None => None, } }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("cache.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
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
    assert!(old.diagnostics().is_empty(), "{:?}", old.diagnostics());
    sources
        .set(
            "cache.kgr",
            text.replace("{ 1 }", "{ val n = 2; n }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let current = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    let owner_path = old.definitions().resolve(*owner).unwrap().to_path();
    let mut fresh_database = test_analysis();
    let fresh_headers = fresh_database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let fresh_owner = fresh_headers
        .file(file)
        .unwrap()
        .declarations()
        .definitions()
        .lookup(&owner_path)
        .unwrap();
    let fresh = fresh_database
        .body(sources.snapshot(), &fresh_owner, &Default::default())
        .unwrap()
        .unwrap();
    current.type_table().assert_same_source_facts(
        current.definitions(),
        fresh.type_table(),
        fresh.definitions(),
        current.lowered().module.nodes.arena(),
        fresh.lowered().module.nodes.arena(),
    );
}
