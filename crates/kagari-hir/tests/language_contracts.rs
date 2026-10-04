use kagari_contract::library;
use kagari_contract::library::catalog as language;
use kagari_contract::{
    language::{self as identities, Protocol},
    library::namespaces,
    scalar::BuiltinType,
    types::{Constraint, PublicItem, Ty, inheritance, verify},
};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_hir::native::render::declaration_source;
use {
    kagari_common::{
        cancellation::CancellationToken,
        collection::CollectionAccess,
        identity::{DefinitionPath, associated_type_id},
    },
    kagari_source::source_database::{SourceDatabase, SourceLayer},
};

#[test]
fn portable_language_catalog_has_complete_contracts() {
    let modules = language::declarations();
    for module in &modules {
        module.validate().expect("valid language declarations");
        let items: Vec<_> = module
            .traits
            .iter()
            .cloned()
            .map(PublicItem::Trait)
            .chain(module.types.iter().cloned().map(PublicItem::Type))
            .collect();
        verify::validate(&items, &module.identity, &CancellationToken::default()).unwrap();
    }
    let traits: Vec<_> = modules.iter().flat_map(|module| &module.traits).collect();
    let types: Vec<_> = modules.iter().flat_map(|module| &module.types).collect();
    for protocol in Protocol::ALL {
        assert_eq!(
            traits
                .iter()
                .filter(|contract| contract.name == protocol.name())
                .count(),
            1
        );
    }
    let iterable = traits.iter().find(|t| t.name == "Iterable").unwrap();
    let iter = iterable
        .associated_types
        .iter()
        .find(|t| {
            t.declaration == associated_type_id(&identities::identity(Protocol::Iterable), "Iter")
        })
        .unwrap();
    assert!(
        matches!(&iter.bounds[..], [Constraint::Trait(required)] if Protocol::from_id(&required.declaration) == Some(Protocol::Iterator))
    );
    for kind in ["Map", "Set", "MutableMap", "MutableSet"] {
        assert!(
            traits
                .iter()
                .find(|t| t.name == kind)
                .unwrap()
                .bounds
                .is_empty()
        );
    }
    for kind in ["HashMap", "HashSet"] {
        assert_eq!(
            types.iter().find(|t| t.name == kind).unwrap().bounds[0]
                .constraints
                .iter()
                .map(|bound| match bound {
                    Constraint::Trait(interface) => Protocol::from_id(&interface.declaration),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![
                Some(Protocol::Eq),
                Some(Protocol::PartialEq),
                Some(Protocol::Hash)
            ]
        );
    }
    let generated = modules
        .iter()
        .map(|module| declaration_source(module).unwrap().text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(generated.contains("type Iter: Iterator<Item ="));
    assert!(generated.contains("trait FromIterator<T0>"));
    assert!(generated.contains("fn from_iter<M0>(source: M0)"));
    assert!(generated.contains("fn sorted_by_key<M0>"));
    assert!(generated.contains("fn retain("));
}

#[test]
fn language_records_are_available_without_native_modules() {
    let mut sources = SourceDatabase::default();
    sources
        .set(
            "language.kgr",
            "fn value(x: Option<i32>) -> Option<i32> { x }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![]);
    let snapshot = analysis
        .declarations(sources.snapshot(), &CancellationToken::default())
        .unwrap();
    let core = snapshot
        .files()
        .find(|file| {
            file.source().module_identity() == &identities::identity(Protocol::Iterator).module
        })
        .unwrap();
    assert!(core.diagnostics().is_empty(), "{:?}", core.diagnostics());
    assert!(
        snapshot
            .declaration(&kagari_hir::declarations::DeclarationId::Definition(
                identities::identity(Protocol::Iterable)
            ))
            .is_some()
    );
    let array = language::declarations()
        .into_iter()
        .flat_map(|module| module.types)
        .find(|t| t.name == "Vec")
        .unwrap();
    assert!(matches!(
        array.generic_params[0].as_type(),
        Ty::Parameter { position: 0, .. }
    ));
}

#[test]
fn portable_collection_view_preserves_concrete_iterator_proofs() {
    let modules = language::declarations();
    let lookup = |id: &DefinitionPath| {
        modules
            .iter()
            .find(|module| module.identity == id.module)
            .and_then(|module| {
                module.traits.iter().find(|contract| {
                    module.definition(
                        kagari_common::identity::DefinitionKind::Trait,
                        &contract.name,
                    ) == *id
                })
            })
    };
    let list = library::applied("List", vec![Ty::Builtin(BuiltinType::I32)]);
    let receiver = Ty::Trait(list.clone());
    let cancel = CancellationToken::default();
    let original = inheritance::trait_closure(&list, &receiver, &cancel, &lookup).unwrap();
    let views = inheritance::interface_views(&list, &receiver, &cancel, &lookup).unwrap();
    let iter = associated_type_id(&identities::identity(Protocol::Iterable), "Iter");
    let original = original
        .iter()
        .find(|parent| Protocol::from_id(&parent.declaration) == Some(Protocol::Iterable))
        .unwrap();
    assert!(!original.associated_types.contains_key(&iter));
    let view = views
        .iter()
        .find(|parent| Protocol::from_id(&parent.declaration) == Some(Protocol::Iterable))
        .unwrap();
    assert!(matches!(&view.associated_types[&iter], Ty::Trait(iterator)
        if Protocol::from_id(&iterator.declaration) == Some(Protocol::Iterator)
            && iterator.associated_types[&associated_type_id(&iterator.declaration, "Item")] == Ty::Builtin(BuiltinType::I32)));
    let mut actual = original.clone();
    actual
        .associated_types
        .insert(iter, Ty::Iter(Box::new(Ty::Builtin(BuiltinType::I32))));
    let receiver = Ty::Array(
        Box::new(Ty::Builtin(BuiltinType::I32)),
        CollectionAccess::Mutable,
    );
    assert_eq!(
        inheritance::interface_views(&actual, &receiver, &cancel, &lookup).unwrap()[0],
        actual
    );
}

fn diagnostics(text: &str) -> Vec<String> {
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("contracts.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![]);
    let snapshot = analysis
        .snapshot(sources.snapshot(), &CancellationToken::default())
        .unwrap();
    snapshot
        .file(id)
        .unwrap()
        .result()
        .diagnostics()
        .iter()
        .map(|diagnostic| format!("{:?}", diagnostic.kind))
        .collect()
}

#[test]
fn default_containers_and_user_iterator_type_check_without_an_optional_module() {
    let errors = diagnostics(
        r#"use std::collections::{HashMap, HashSet, MutableList};

        struct Cursor { var current: i32 }
        impl Iterator for Cursor {
            type Item = i32;
            fn next(self) -> Option<i32> {
                if self.current < 3 { self.current = self.current + 1; Some(self.current) } else { None }
            }
        }
        fn main() -> i32 {
            val data: MutableList<i32> = [1, 2];
            data.push(3);
            val view: [i32] = data;
            val map: HashMap<i32, i32> = HashMap::new();
            map.insert(1, 2);
            val set: HashSet<i32> = HashSet::new();
            set.insert(3);
            var total = 0;
            val cursor = Cursor { current: 0 };
            for item in cursor { total = total + item; }
            for item in 0..3 { total = total + item; }
            for item in view { total = total + item; }
            total + view[0]
        }
    "#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn readonly_mutation_and_unhashable_default_keys_are_rejected() {
    let errors = diagnostics("fn bad() { val data: [i32] = [1]; data.push(2); }");
    assert!(
        errors.iter().any(|error| error.contains("push")),
        "{errors:#?}"
    );
    let errors =
        diagnostics("use std::collections::{HashMap};\nfn bad(value: HashMap<f64, i32>) { }");
    assert!(
        errors
            .iter()
            .any(|error| error.contains("StandardConstraintNotSatisfied")
                && error.contains("f64")
                && error.contains("Eq + Hash")),
        "default hash storage did not reject its concrete key bound: {errors:#?}"
    );
    let errors = diagnostics("use std::collections::{Map};\nfn valid<T>(value: Map<T, i32>) { }");
    assert!(
        errors.is_empty(),
        "Map incorrectly imposed Hash/Eq: {errors:#?}"
    );
}

#[test]
fn iterable_requires_a_real_iterator_with_the_same_item() {
    let errors = diagnostics(
        r#"
        struct Items {}
        impl Iterable for Items {
            type Item = i32;
            type Iter = i32;
            fn iter(self) -> i32 { 0 }
        }
    "#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("InvalidAssociatedType")
                && error.contains("Iter")
                && error.contains("declared bound")),
        "{errors:#?}"
    );
    let errors = diagnostics(
        r#"
        struct Cursor {}
        impl Iterator for Cursor {
            type Item = bool;
            fn next(self) -> Option<bool> { None }
        }
        struct Items {}
        impl Iterable for Items {
            type Item = i32;
            type Iter = Cursor;
            fn iter(self) -> Cursor { Cursor {} }
        }
    "#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("InvalidAssociatedType")
                && error.contains("Iter")
                && error.contains("declared bound")),
        "{errors:#?}"
    );
}

#[test]
fn list_method_navigation_uses_language_owned_declarations() {
    let module = language::declarations()
        .into_iter()
        .find(|module| module.identity == namespaces::module("std", "collections"))
        .unwrap();
    let generated = declaration_source(&module).unwrap();
    let source = "use std::collections::{List};\nfn main() { val values: List<i32> = [2,1]; values.sorted_by_key(|value| value); }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("list-navigation.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![]);
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let offset = source.find("sorted_by_key").unwrap();
    let target = snapshot.definition_at(file, offset).unwrap();
    let declaration = snapshot.source(target.location.file).unwrap();
    assert_eq!(declaration.name(), generated.uri);
    assert_eq!(
        &declaration.text()[target.location.range.start..target.location.range.end],
        "sorted_by_key"
    );
    let docs = snapshot.documentation_at(file, offset).unwrap();
    assert!(docs.documentation.contains("during comparisons"));
    assert!(docs.written_signature.contains("Ord"));
}

#[test]
fn string_method_docs_completion_and_navigation_share_the_language_catalog() {
    let generated = declaration_source(
        &language::declarations()
            .into_iter()
            .find(|module| module.identity == namespaces::type_owner("String"))
            .unwrap(),
    )
    .unwrap();
    let source = "fn main() { val text = \"é🙂\"; text.slice(0usize, 2usize); text. }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("string-navigation.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![]);
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let offset = source.find("slice").unwrap();
    let target = snapshot.definition_at(file, offset).unwrap();
    let declaration = snapshot.source(target.location.file).unwrap();
    assert_eq!(declaration.name(), generated.uri);
    assert_eq!(
        &declaration.text()[target.location.range.start..target.location.range.end],
        "slice"
    );
    let docs = snapshot.documentation_at(file, offset).unwrap();
    assert!(docs.documentation.contains("UTF-8 boundaries"));
    assert!(docs.written_signature.contains("usize"));
    let candidates = snapshot
        .file(file)
        .unwrap()
        .method_completions(source.rfind("text. }").unwrap() + 5);
    for name in [
        "len",
        "is_empty",
        "contains",
        "starts_with",
        "ends_with",
        "find",
        "slice",
        "trim",
        "trim_start",
        "trim_end",
        "replace",
        "split",
    ] {
        let candidate = candidates
            .iter()
            .find(|candidate| candidate.name == name)
            .expect(name);
        let declaration = snapshot.declaration(&candidate.declaration).unwrap();
        assert_eq!(
            snapshot.source(declaration.location.file).unwrap().name(),
            generated.uri
        );
    }
}
