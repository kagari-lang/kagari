use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_hir::{analysis::*, types::*};
use kagari_hir::{
    hir::{ids::HirOwner, ty::TypeKind},
    typeck::table::ResolvedTypeTarget,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::scalar::BuiltinType;

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn impl_headers_keep_trait_targets_arguments_and_exact_source_positions() {
    let text = "// 中文 😀\r\ntrait View {} struct Point {} impl View for Point {} impl View<Missing, Point> for Point {} fn good(x: i32) -> i32 { x }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("impl.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let signatures = db
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    let header = signatures.file(file).unwrap();
    assert!(matches!(
        header
            .queries()
            .type_at(text.find("impl View for").unwrap() + 5),
        Some(TypeId::Trait(_))
    ));
    assert_eq!(
        header.queries().type_at(text.find("Missing").unwrap()),
        Some(TypeId::Error)
    );
    assert!(matches!(
        header.queries().type_at(text.find(", Point>").unwrap() + 2),
        Some(TypeId::Struct(_))
    ));
    let snapshot = analyze(&mut db, &sources);
    let analysis = snapshot.file(file).unwrap();
    let authoring_facts = analysis.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.records().facts();
    let valid = facts.lowered.module.impls[0].trait_ref.as_ref().unwrap();
    let applied = facts.lowered.module.impls[1].trait_ref.as_ref().unwrap();
    assert_eq!(valid.ty.owner(), HirOwner::Declaration);
    assert_eq!(applied.ty.owner(), HirOwner::Declaration);
    let nominal = facts.typed.type_table().type_ref(valid.ty).unwrap();
    assert_eq!(
        nominal.target,
        Some(ResolvedTypeTarget::Trait(facts.lowered.module.traits[0].id))
    );
    assert!(facts.typed.type_table().constraint(valid.ty).is_some());
    let invalid = facts.typed.type_table().type_ref(applied.ty).unwrap();
    assert_eq!(invalid.ty, TypeId::Error);
    assert_eq!(invalid.target, nominal.target);
    assert!(facts.typed.type_table().constraint(applied.ty).is_none());
    let TypeKind::Generic { args, .. } = &facts.lowered.module.type_ref(applied.ty).kind else {
        panic!("retained application");
    };
    assert_eq!(args.len(), 2);
    assert!(args.iter().all(|arg| arg.owner() == HirOwner::Declaration));
    let span = facts.lowered.source_map.type_span(applied.ty);
    assert_eq!(&text[span.start..span.end], "View<Missing, Point>");
    let error = analysis
        .result()
        .diagnostics()
        .iter()
        .find(|d| d.kind.code() == "KG_TYPE_INVALID_TRAIT_REFERENCE")
        .unwrap();
    assert_eq!(error.span, Some(span));
    assert_eq!(
        analysis.queries().definition_at(span.start),
        analysis
            .queries()
            .definition_at(facts.lowered.source_map.type_span(valid.ty).start)
    );
    assert_eq!(
        analysis.queries().type_at(text.rfind(" x }").unwrap() + 1),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert!(analysis.result().clone().into_codegen().is_err());
}

#[test]
fn imported_generic_traits_require_arguments_and_keep_their_navigation_target() {
    for (path, declaration) in [
        ("core::ops::Add", None),
        ("pkg::library::Build", Some("pub trait Build<T> {}")),
    ] {
        let mut sources = SourceDatabase::default();
        if let Some(declaration) = declaration {
            sources
                .bind_module(
                    "library",
                    ModuleIdentity {
                        package: PackageId("pkg".into()),
                        path: vec!["library".into()],
                    },
                )
                .unwrap();
            sources
                .set("library", declaration.into(), SourceLayer::Base)
                .unwrap();
        }
        let text = format!(
            "use {path} as Protocol; fn bad<T: Protocol>(value: T) {{}} fn good<T: Protocol<i32>>(value: T) {{}}"
        );
        sources
            .bind_module(
                "root",
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec!["root".into()],
                },
            )
            .unwrap();
        let file = sources
            .set("root", text.clone(), SourceLayer::Base)
            .unwrap();
        let snapshot = analyze(&mut test_analysis(), &sources);
        let analysis = snapshot.file(file).unwrap();
        let bad = text.find("T: Protocol>").unwrap() + 3;
        let good = text.find("T: Protocol<i32>").unwrap() + 3;
        assert_eq!(
            snapshot.queries().definition_at(file, bad).unwrap().id,
            snapshot.queries().definition_at(file, good).unwrap().id
        );
        assert!(
            analysis
                .result()
                .diagnostics()
                .iter()
                .any(|diagnostic| matches!(
                    &diagnostic.kind,
                    DiagnosticKind::InvalidTraitReference { reason, .. }
                        if *reason == "generic trait references require concrete type arguments"
                ))
        );
        let authoring_facts = analysis.to_unverified(&Default::default()).unwrap();
        let facts = authoring_facts.records().facts();
        for function in &facts.lowered.module.functions {
            let bound = function.generic_params[0].bounds[0].ty;
            assert_eq!(
                facts.typed.type_table().constraint(bound).is_some(),
                function.name == "good"
            );
        }
    }
}

#[test]
fn imported_trait_headers_keep_distinct_executable_identities() {
    let mut sources = SourceDatabase::default();
    let mut insert = |name: &str, text: &str| {
        sources
            .bind_module(
                name,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        sources.set(name, text.into(), SourceLayer::Base).unwrap()
    };
    let left = insert("left", "pub trait View {}");
    let right = insert("right", "pub trait View {}");
    insert("facade", "pub use pkg::left::View;");
    let text = "use pkg::facade::View as L; use pkg::right as r; struct Point {} impl L for Point {} impl r::View for Point {} fn bound<T: L>(x: T) {} fn good(x: i32) -> i32 { x }";
    let root = insert("root", text);
    let mut db = test_analysis();
    let signatures = db
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    let header = signatures.file(root).unwrap();
    let first = text.find("impl L").unwrap() + 5;
    let second = text.find("impl r::View").unwrap() + "impl r::".len();
    assert!(matches!(
        header.queries().type_at(first),
        Some(TypeId::Trait(_))
    ));
    assert_ne!(
        header.queries().type_at(first),
        header.queries().type_at(second)
    );
    let snapshot = analyze(&mut db, &sources);
    let analysis = snapshot.file(root).unwrap();
    assert_eq!(
        analysis
            .queries()
            .definition_at(first)
            .unwrap()
            .location
            .file,
        left
    );
    assert_eq!(
        analysis
            .queries()
            .definition_at(second)
            .unwrap()
            .location
            .file,
        right
    );
    assert_eq!(
        analysis
            .queries()
            .definition_at(text.find("T: L").unwrap() + 3)
            .unwrap()
            .location
            .file,
        left
    );
    assert!(
        !analysis
            .result()
            .diagnostics()
            .iter()
            .any(|d| d.kind.code() == "KG_TYPE_UNKNOWN_TRAIT")
    );
    assert!(analysis.result().clone().into_codegen().is_ok());
}

#[test]
fn callable_facts_survive_body_reuse_and_preserve_bound_navigation() {
    let text = r#"
struct Add {val value:i32}
impl Fn<(i32,)> for Add {type Output=i32; fn call(self,args:(i32,))->i32 {self.value+args[0]}}
fn apply<F: Fn(i32)->i32>(f:F)->i32 {f(1)}
fn callback()->fn(i32)->i32 {Add{value:41}}
fn untouched()->i32 {1}
"#;
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("callables.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    assert!(old.file(file).unwrap().result().diagnostics().is_empty());
    let definition = old
        .queries()
        .documentation_at(file, text.find("Fn(i32)").unwrap())
        .unwrap();
    assert_eq!(definition.declaration.name, "Fn");
    sources
        .set(
            "callables.kgr",
            text.replace("fn untouched()->i32 {1}", "fn untouched()->i32 {1+2}"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let updated = analyze(&mut db, &sources);
    let fresh = analyze(&mut test_analysis(), &sources);
    let authoring_facts = updated
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let _facts = authoring_facts.records().facts();
    let authoring_fresh = fresh
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let _fresh = authoring_fresh.records().facts();

    assert!(updated.file(file).unwrap().signatures_reused());
    assert!(
        updated
            .file(file)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
}
