use crate::analysis::*;
use crate::types::*;
use crate::{declarations::DeclarationId, tests::test_analysis};
use kagari_common::{
    identity::{DefinitionKind, ModuleIdentity, PackageId},
    span::Span,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::language::Protocol;

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn generic_binders_and_explicit_traits_use_ordinary_constraint_names() {
    for name in ["SignedNumber", "OrderedNumber"] {
        let mut sources = SourceDatabase::default();
        let file = sources
            .set(
                "unknown.kgr",
                format!("fn take<T: {name}>(value: T) {{}}"),
                SourceLayer::Base,
            )
            .unwrap();
        let snapshot = analyze(&mut test_analysis(), &sources);
        let result = snapshot.file(file).unwrap().result();
        assert!(result.diagnostics().iter().any(|diagnostic| matches!(
            &diagnostic.kind,
            DiagnosticKind::UnknownTrait { trait_name } if trait_name == name
        )));
        assert!(result.clone().into_codegen().is_err());
    }
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
        let snapshot = analyze(&mut test_analysis(), &sources);
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
    let text = "trait Eq {} trait SignedNumber {} trait OrderedNumber {} struct Point {} impl Eq for Point {} impl SignedNumber for Point {} impl OrderedNumber for Point {} fn pass<T: Eq>(x: T) -> T { x } fn signed<T: SignedNumber>(x: T) -> T { x } fn ordered<T: OrderedNumber>(x: T) -> T { x } fn identity<Eq>(value: Eq) -> Eq { value }";
    let file = sources
        .set("valid.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analyze(&mut test_analysis(), &sources);
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
    assert!(
        facts
            .facts()
            .typed
            .type_table
            .constraint(
                facts
                    .facts()
                    .lowered
                    .module
                    .generic_parameter(function.generic_params[0])
                    .bounds[0]
                    .ty
            )
            .is_some()
    );
}

#[test]
fn imported_supertraits_resolve_authored_associated_members_from_source_facts() {
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
            "use core::iter::Iterator as Base; pub trait Stream: Base {}".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let text = "use pkg::library::Stream; fn identity<S: Stream>(source: S, item: S::Item) -> S::Item { item }";
    sources
        .bind_module(
            "root",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["consumer".into()],
            },
        )
        .unwrap();
    let root = sources.set("root", text.into(), SourceLayer::Base).unwrap();
    let snapshot = analyze(&mut test_analysis(), &sources);
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
    assert_eq!(
        Some(interface.declaration),
        Protocol::Iterator.definition(file.definitions())
    );
    assert_eq!(
        Some(*member),
        file.definitions().lookup_child(
            interface.declaration,
            DefinitionKind::AssociatedType,
            "Item",
            0
        )
    );
    assert!(arguments.is_empty());
    assert_eq!(signature.params[1].ty, signature.return_type);
    let declaration = snapshot
        .declaration(&DeclarationId::Definition(*member))
        .unwrap();
    let authored = &snapshot
        .inputs()
        .core_modules()
        .iter()
        .find(|input| *input.source.module_identity() == Protocol::Iterator.module())
        .unwrap()
        .source;
    let start = authored.text().find("type Item;").unwrap() + "type ".len();
    let site = authored
        .span(Span::new(start, start + "Item".len()))
        .unwrap();
    assert_eq!(declaration.source_location(), Some(site));
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
    let mut db = test_analysis();
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
            "use std::hash::Hash; pub trait Echo<T: Eq + Hash> { fn get(self) -> i32; }",
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
    let mut db = test_analysis();
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
    let mut db = test_analysis();
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
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    let authoring_old_facts = old
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let old_facts = authoring_old_facts.records().facts();
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
    let facts = authoring_facts.records().facts();
    assert!(facts.typed.type_table.type_ref(old_ref).is_none());
    let fresh = analyze(&mut test_analysis(), &sources);
    let authoring_fresh = fresh
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let fresh = authoring_fresh.records().facts();
    facts.typed.type_table.assert_same_source_facts(
        authoring_facts.definitions(),
        &fresh.typed.type_table,
        authoring_fresh.definitions(),
        facts.lowered.module.nodes.arena(),
        fresh.lowered.module.nodes.arena(),
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
fn imported_interface_eligibility_checks_method_bounds() {
    for (argument, accepted) in [("Self", false), ("i32", true)] {
        let mut sources = SourceDatabase::default();
        let mut root = None;
        for (name, text) in [
            (
                "api",
                format!(
                    "pub trait Related<T> {{}} pub trait View {{ fn read<T: Related<{argument}>>(self, value: T); }}"
                ),
            ),
            (
                "root",
                "use pkg::api::View; fn accept(value: View) {}".into(),
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
            let file = sources.set(name, text, SourceLayer::Base).unwrap();
            if name == "root" {
                root = Some(file);
            }
        }
        let mut db = test_analysis();
        let snapshot = analyze(&mut db, &sources);
        let analysis = snapshot.file(root.unwrap()).unwrap();
        assert_eq!(
            analysis.result().diagnostics().is_empty(),
            accepted,
            "{argument}: {:?}",
            analysis.result().diagnostics()
        );
        assert_eq!(analysis.result().clone().into_codegen().is_ok(), accepted);
    }
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
    let facts = authoring_facts.records().facts();
    let authoring_fresh = fresh
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let fresh = authoring_fresh.records().facts();
    facts.typed.type_table.assert_same_source_facts(
        authoring_facts.definitions(),
        &fresh.typed.type_table,
        authoring_fresh.definitions(),
        facts.lowered.module.nodes.arena(),
        fresh.lowered.module.nodes.arena(),
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
