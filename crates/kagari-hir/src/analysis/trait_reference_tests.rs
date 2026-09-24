use super::*;
use crate::{
    hir::{HirOwner, TypeKind},
    typeck::{ConstraintTarget, TypeTarget},
    types::BuiltinType,
};
use kagari_common::{
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap()
}

#[test]
fn impl_headers_keep_trait_targets_arguments_and_exact_source_positions() {
    let text = "// 中文 😀\r\ntrait View {} struct Point {} impl View for Point {} impl View<Missing, Point> for Point {} fn good(x: i32) -> i32 { x }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("impl.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let signatures = db
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    let header = signatures.file(file).unwrap();
    assert!(matches!(
        header.type_at(text.find("impl View for").unwrap() + 5),
        Some(TypeId::Trait(_))
    ));
    assert_eq!(
        header.type_at(text.find("Missing").unwrap()),
        Some(TypeId::Error)
    );
    assert!(matches!(
        header.type_at(text.find(", Point>").unwrap() + 2),
        Some(TypeId::Struct(_))
    ));
    let snapshot = analyze(&mut db, &sources);
    let analysis = snapshot.file(file).unwrap();
    let facts = analysis.result().facts();
    let valid = facts.lowered.module.impls[0].trait_ref.as_ref().unwrap();
    let applied = facts.lowered.module.impls[1].trait_ref.as_ref().unwrap();
    assert_eq!(valid.ty.owner(), HirOwner::Declaration);
    assert_eq!(applied.ty.owner(), HirOwner::Declaration);
    let nominal = facts.typed.type_table.type_ref(valid.ty).unwrap();
    assert_eq!(
        nominal.target,
        Some(TypeTarget::Trait(facts.lowered.module.traits[0].id))
    );
    assert!(matches!(
        facts.typed.type_table.constraint(valid.ty),
        Some(ConstraintTarget::Trait(_))
    ));
    let invalid = facts.typed.type_table.type_ref(applied.ty).unwrap();
    assert_eq!(invalid.ty, TypeId::Error);
    assert_eq!(invalid.target, nominal.target);
    assert!(facts.typed.type_table.constraint(applied.ty).is_none());
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
        analysis.definition_at(span.start),
        analysis.definition_at(facts.lowered.source_map.type_span(valid.ty).start)
    );
    assert_eq!(
        analysis.type_at(text.rfind(" x }").unwrap() + 1),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert!(analysis.result().clone().into_codegen().is_err());
}

#[test]
fn generic_binders_and_explicit_traits_shadow_standard_constraint_names() {
    for text in [
        "trait View {} struct Point {} impl<View> View for Point {}",
        "trait View {} fn bad<View, T: View>(x: T) {}",
        "fn bad<HashKey, T: HashKey>(x: T) {}",
        "struct Point {} impl HashKey for Point {}",
        "trait View {} struct Point {} impl View<> for Point {}",
        "trait View<T> {} struct Point {} impl View for Point {}",
    ] {
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("bad.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
        let analysis = snapshot.file(file).unwrap();
        assert!(
            analysis
                .result()
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_TYPE_INVALID_TRAIT_REFERENCE"),
            "{text}: {:?}",
            analysis.result().diagnostics()
        );
        assert!(analysis.result().clone().into_codegen().is_err());
    }
    let mut sources = SourceDatabase::default();
    let text = "trait HashKey {} struct Point {} impl HashKey for Point {} fn pass<T: HashKey>(x: T) -> T { x }";
    let file = sources
        .set("valid.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
    let facts = snapshot.file(file).unwrap().result();
    assert!(facts.diagnostics().is_empty(), "{:?}", facts.diagnostics());
    let function = facts
        .facts()
        .lowered
        .module
        .functions
        .iter()
        .find(|f| f.name == "pass")
        .unwrap();
    assert!(matches!(
        facts
            .facts()
            .typed
            .type_table
            .constraint(function.generic_params[0].bounds[0].ty),
        Some(ConstraintTarget::Trait(_))
    ));
}

#[test]
fn imported_trait_headers_keep_nominal_navigation_even_before_execution_support() {
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
    let mut db = AnalysisDatabase::default();
    let signatures = db
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    let header = signatures.file(root).unwrap();
    let first = text.find("impl L").unwrap() + 5;
    let second = text.find("impl r::View").unwrap() + "impl r::".len();
    assert!(matches!(header.type_at(first), Some(TypeId::Trait(_))));
    assert_ne!(header.type_at(first), header.type_at(second));
    let snapshot = analyze(&mut db, &sources);
    let analysis = snapshot.file(root).unwrap();
    assert_eq!(analysis.definition_at(first).unwrap().location.file, left);
    assert_eq!(analysis.definition_at(second).unwrap().location.file, right);
    assert_eq!(
        analysis
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
    assert_eq!(
        analysis
            .result()
            .diagnostics()
            .iter()
            .filter(|d| d.kind.code() == "KG_TYPE_INVALID_TRAIT_REFERENCE")
            .count(),
        3
    );
    assert!(analysis.result().clone().into_codegen().is_err());
}

#[test]
fn body_edits_rebase_impl_trait_references_in_signature_cache() {
    let text = "trait View {} struct Point {} impl View for Point { fn value(self) -> i32 { 1 } }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("cache.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let old = analyze(&mut db, &sources);
    let old_facts = old.file(file).unwrap().result().facts();
    let old_ref = old_facts.lowered.module.impls[0]
        .trait_ref
        .as_ref()
        .unwrap()
        .ty;
    let edited = text.replace("{ 1 }", "{ val x = 2; x }");
    sources
        .set("cache.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let new = analyze(&mut db, &sources);
    let new_file = new.file(file).unwrap();
    assert!(new_file.signatures_reused());
    let facts = new_file.result().facts();
    assert!(facts.typed.type_table.type_ref(old_ref).is_none());
    let fresh = analyze(&mut AnalysisDatabase::default(), &sources);
    let fresh = fresh.file(file).unwrap().result().facts();
    facts.typed.type_table.assert_same_source_facts(
        &fresh.typed.type_table,
        facts.lowered.module.body.arena(),
        fresh.lowered.module.body.arena(),
    );
    sources
        .set(
            "cache.kgr",
            edited.replace("impl View for", "impl View<i32> for"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let invalid = analyze(&mut db, &sources);
    assert!(!invalid.file(file).unwrap().signatures_reused());
    assert!(
        invalid
            .file(file)
            .unwrap()
            .result()
            .clone()
            .into_codegen()
            .is_err()
    );
    assert!(matches!(
        old_facts.typed.type_table.type_ref(old_ref).unwrap().ty,
        TypeId::Trait(_)
    ));
}
