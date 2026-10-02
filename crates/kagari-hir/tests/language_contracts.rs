use kagari_abi::language::catalog as language;
use kagari_abi::{
    language::primitive,
    language::{self as identities, Protocol},
    scalar::BuiltinType,
    types::{AbiType, ConstraintAbi, PublicAbiItem, inheritance, verify},
};
use kagari_common::{
    cancellation::CancellationToken,
    collection::CollectionAccess,
    identity::{DefinitionId, associated_type_id},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_hir::analysis::AnalysisDatabase;

#[test]
fn portable_language_catalog_has_complete_contracts() {
    let module = language::declarations();
    module.validate().expect("valid language declarations");
    let items: Vec<_> = module
        .traits
        .iter()
        .cloned()
        .map(PublicAbiItem::Trait)
        .chain(module.types.iter().cloned().map(PublicAbiItem::Type))
        .collect();
    verify::validate(&items, &module.identity, &CancellationToken::default()).unwrap();
    for protocol in Protocol::ALL {
        assert_eq!(
            module
                .traits
                .iter()
                .filter(|contract| contract.name == protocol.name())
                .count(),
            1
        );
    }
    let iterable = module.traits.iter().find(|t| t.name == "Iterable").unwrap();
    let iter = iterable
        .associated_types
        .iter()
        .find(|t| {
            t.declaration == associated_type_id(&identities::identity(Protocol::Iterable), "Iter")
        })
        .unwrap();
    assert!(
        matches!(&iter.bounds[..], [ConstraintAbi::Trait(required)] if Protocol::from_id(&required.declaration) == Some(Protocol::Iterator))
    );
    for kind in ["Map", "Set", "MutableMap", "MutableSet"] {
        assert!(
            module
                .traits
                .iter()
                .find(|t| t.name == kind)
                .unwrap()
                .bounds
                .is_empty()
        );
    }
    for kind in ["HashMap", "HashSet"] {
        assert_eq!(
            module.types.iter().find(|t| t.name == kind).unwrap().bounds[0]
                .constraints
                .iter()
                .map(|bound| match bound {
                    ConstraintAbi::Trait(interface) => Protocol::from_id(&interface.declaration),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![
                Some(Protocol::Eq),
                Some(Protocol::Hash),
                Some(Protocol::PartialEq)
            ]
        );
    }
    let generated = module.declaration_source().unwrap();
    assert!(generated.text.contains("type Iter: Iterator<Item ="));
    assert!(generated.text.contains("trait FromIterator<T0>"));
    assert!(generated.text.contains("fn from_iter<M0>(source: M0)"));
    assert!(!generated.text.contains("fn sort"));
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
        .find(|file| file.source().module_identity() == &identities::module_identity())
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
        .types
        .into_iter()
        .find(|t| t.name == "ArrayList")
        .unwrap();
    assert!(matches!(
        array.generic_params[0].as_type(),
        AbiType::Parameter { position: 0, .. }
    ));
}

#[test]
fn portable_collection_view_preserves_concrete_iterator_proofs() {
    let module = language::declarations();
    let lookup = |id: &DefinitionId| {
        module.traits.iter().find(|contract| {
            Protocol::from_id(id).is_some_and(|protocol| protocol.name() == contract.name)
        })
    };
    let list = primitive::applied(Protocol::List, vec![AbiType::Builtin(BuiltinType::I32)]);
    let receiver = AbiType::Trait(list.clone());
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
    assert!(
        matches!(&view.associated_types[&iter], AbiType::Trait(iterator)
        if Protocol::from_id(&iterator.declaration) == Some(Protocol::Iterator)
            && iterator.associated_types[&associated_type_id(&iterator.declaration, "Item")] == AbiType::Builtin(BuiltinType::I32))
    );
    let mut actual = original.clone();
    actual.associated_types.insert(
        iter,
        AbiType::Iter(Box::new(AbiType::Builtin(BuiltinType::I32))),
    );
    let receiver = AbiType::Array(
        Box::new(AbiType::Builtin(BuiltinType::I32)),
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
        .snapshot(
            sources.snapshot(),
            Default::default(),
            &CancellationToken::default(),
        )
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
        r#"
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
    let errors = diagnostics("fn bad(value: HashMap<f64, i32>) { }");
    assert!(
        errors
            .iter()
            .any(|error| error.contains("StandardConstraintNotSatisfied")
                && error.contains("f64")
                && error.contains("Eq + Hash")),
        "default hash storage did not reject its concrete key bound: {errors:#?}"
    );
    let errors = diagnostics("fn valid<T>(value: Map<T, i32>) { }");
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
