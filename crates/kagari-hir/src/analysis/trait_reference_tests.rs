use super::*;
use crate::{
    hir::{ids::HirOwner, ty::TypeKind},
    typeck::table::{ConstraintTarget, TypeTarget},
};
use kagari_abi::scalar::BuiltinType;
use kagari_common::{
    diagnostic::DiagnosticKind,
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};

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
    let authoring_facts = analysis.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.facts();
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
        "fn bad<Eq, T: Eq>(x: T) {}",
        "fn bad<OrderedNumber, T: OrderedNumber>(x: T) {}",
        "fn bad<SignedNumber, T: SignedNumber>(x: T) {}",
        "struct Point {} impl Eq for Point {}",
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
            analysis.result().diagnostics().iter().any(|d| matches!(
                d.kind.code(),
                "KG_TYPE_INVALID_TRAIT_REFERENCE" | "KG_TYPE_INVALID_TRAIT_IMPL"
            )),
            "{text}: {:?}",
            analysis.result().diagnostics()
        );
        assert!(analysis.result().clone().into_codegen().is_err());
    }
    let mut sources = SourceDatabase::default();
    let text = "trait Eq {} struct Point {} impl Eq for Point {} fn pass<T: Eq>(x: T) -> T { x }";
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
fn imported_generic_traits_require_arguments_and_keep_their_navigation_target() {
    for (path, declaration) in [
        ("core::language::Add", None),
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
        let file = sources
            .set("root", text.clone(), SourceLayer::Base)
            .unwrap();
        let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
        let analysis = snapshot.file(file).unwrap();
        let bad = text.find("T: Protocol>").unwrap() + 3;
        let good = text.find("T: Protocol<i32>").unwrap() + 3;
        assert_eq!(
            snapshot.definition_at(file, bad).unwrap().id,
            snapshot.definition_at(file, good).unwrap().id
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
        let facts = authoring_facts.facts();
        for function in &facts.lowered.module.functions {
            let bound = function.generic_params[0].bounds[0].ty;
            assert_eq!(
                facts.typed.type_table.constraint(bound).is_some(),
                function.name == "good"
            );
        }
    }
}

#[test]
fn imported_supertraits_resolve_native_associated_members_from_source_facts() {
    let mut sources = SourceDatabase::default();
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
        .set(
            "library",
            "use core::language::Iterator as Base; pub trait Stream: Base {}".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let text = "use pkg::library::Stream; fn identity<S: Stream>(source: S, item: S::Item) -> S::Item { item }";
    let root = sources.set("root", text.into(), SourceLayer::Base).unwrap();
    let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
    let file = snapshot.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let signature = file
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|function| function.name == "identity")
        .unwrap();
    let TypeId::Projection {
        interface,
        member,
        arguments,
        ..
    } = &signature.return_type
    else {
        panic!("inherited associated type must retain its declaring interface");
    };
    let interface_path = file
        .definitions()
        .resolve(interface.declaration)
        .unwrap()
        .to_path();
    let member_path = file.definitions().resolve(*member).unwrap().to_path();
    assert_eq!(interface_path.module.package.0, "kagari-core");
    assert_eq!(interface_path.path.last().unwrap().name, "Iterator");
    assert_eq!(member_path.path.last().unwrap().name, "Item");
    assert!(arguments.is_empty());
    assert_eq!(signature.params[1].ty, signature.return_type);
    let declaration = snapshot
        .declaration(&crate::declarations::DeclarationId::Definition(*member))
        .unwrap();
    assert_eq!(
        snapshot.source(declaration.location.file).unwrap().name(),
        "kagari://native/kagari-core/language.kgr"
    );
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
    assert!(analysis.result().clone().into_codegen().is_ok());
}

#[test]
fn imported_applied_trait_methods_validate_and_resolve_bound_calls() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    let valid_root = "use pkg::api::Echo; struct Holder { val number: i32 } impl Echo<i32> for Holder { fn get(self) -> i32 { self.number } } fn read<U: Echo<i32>>(x: U) -> i32 { x.get() } fn accepts(value: Echo<i32>) {} fn main() -> i32 { read(Holder { number: 7 }) }";
    for (name, text) in [
        ("api", "pub trait Echo<T> { fn get(self) -> T; }"),
        ("root", valid_root),
    ] {
        sources
            .bind_module(
                name,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let file = sources.set(name, text.into(), SourceLayer::Base).unwrap();
        if name == "root" {
            root = Some(file);
        }
    }
    let mut db = AnalysisDatabase::default();
    let snapshot = analyze(&mut db, &sources);
    let root = root.unwrap();
    let analysis = snapshot.file(root).unwrap();
    assert!(analysis.result().clone().into_codegen().is_ok());

    let invalid = "use pkg::api::Echo; struct Holder { val number: i32 } impl Echo<i32> for Holder { fn get(self) -> bool { true } } fn main() {}";
    sources
        .set("root", invalid.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analyze(&mut db, &sources);
    let analysis = snapshot.file(root).unwrap();
    assert!(
        analysis
            .result()
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(
                &diagnostic.kind,
                DiagnosticKind::TraitMethodMismatch { method_name, .. }
                    if method_name == "get"
            ))
    );
    assert!(analysis.result().clone().into_codegen().is_err());

    let missing = "use pkg::api::Echo; struct Holder { val number: i32 } impl Echo<i32> for Holder {} fn main() {}";
    sources
        .set("root", missing.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analyze(&mut db, &sources);
    let analysis = snapshot.file(root).unwrap();
    assert!(
        analysis
            .result()
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(
                &diagnostic.kind,
                DiagnosticKind::TraitMethodMismatch { method_name, reason, .. }
                    if method_name == "get" && reason == "missing impl method"
            ))
    );

    sources
        .set("root", valid_root.into(), SourceLayer::Base)
        .unwrap();
    assert!(
        analyze(&mut db, &sources)
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    sources
        .set(
            "api",
            "pub trait Echo<T> { fn get(self) -> bool; }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = analyze(&mut db, &sources);
    let analysis = snapshot.file(root).unwrap();
    assert!(
        analysis
            .result()
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(
                &diagnostic.kind,
                DiagnosticKind::TraitMethodMismatch { method_name, .. }
                    if method_name == "get"
            ))
    );
}

#[test]
fn imported_trait_parameter_bounds_reject_invalid_implementations() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "api",
            "pub trait Echo<T: Eq + Hash> { fn get(self) -> i32; }",
        ),
        (
            "root",
            "use pkg::api::Echo; struct Holder {} impl Echo<f32> for Holder { fn get(self) -> i32 { 1 } } fn main() {}",
        ),
    ] {
        sources
            .bind_module(
                name,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let file = sources.set(name, text.into(), SourceLayer::Base).unwrap();
        if name == "root" {
            root = Some(file);
        }
    }
    let mut db = AnalysisDatabase::default();
    let snapshot = analyze(&mut db, &sources);
    let analysis = snapshot.file(root.unwrap()).unwrap();
    assert!(
        analysis
            .result()
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.kind.code() == "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED" })
    );
    assert!(analysis.result().clone().into_codegen().is_err());
}

#[test]
fn imported_generic_method_accepts_interface_calls() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "api",
            "pub trait Mapper<T> { fn map<U>(self, value: U) -> T; }",
        ),
        (
            "root",
            "use pkg::api::Mapper; fn use_interface(value: Mapper<i32>) -> i32 { value.map(42) + value.map(\"key\") }",
        ),
    ] {
        sources
            .bind_module(
                name,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let file = sources.set(name, text.into(), SourceLayer::Base).unwrap();
        if name == "root" {
            root = Some(file);
        }
    }
    let mut db = AnalysisDatabase::default();
    let snapshot = analyze(&mut db, &sources);
    let analysis = snapshot.file(root.unwrap()).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
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
    let authoring_old_facts = old
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let old_facts = authoring_old_facts.facts();
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
    let authoring_facts = new_file.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.facts();
    assert!(facts.typed.type_table.type_ref(old_ref).is_none());
    let fresh = analyze(&mut AnalysisDatabase::default(), &sources);
    let authoring_fresh = fresh
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let fresh = authoring_fresh.facts();
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
    let mut db = AnalysisDatabase::default();
    let old = analyze(&mut db, &sources);
    assert!(old.file(file).unwrap().result().diagnostics().is_empty());
    let definition = old
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
    let fresh = analyze(&mut AnalysisDatabase::default(), &sources);
    let authoring_facts = updated
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let facts = authoring_facts.facts();
    let authoring_fresh = fresh
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let fresh = authoring_fresh.facts();
    facts.typed.type_table.assert_same_source_facts(
        &fresh.typed.type_table,
        facts.lowered.module.body.arena(),
        fresh.lowered.module.body.arena(),
    );
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
