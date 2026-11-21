use crate::tests::standard::standard_modules;
use crate::{
    aggregates::AggregateCatalog,
    analysis::AnalysisDatabase,
    language::semantics::{ProtocolSemantics, intrinsic_holds},
    types::TypeId,
};
use crate::{native::adoption::adopt, tests::standard::verified_core};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, map::DefinitionContext, table::DefinitionId},
};
use kagari_contract::types::trait_contract;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::identity as library;
use kagari_types::{
    language::Protocol,
    scalar::BuiltinType,
    ty::{NominalTy, Ty, inheritance},
};

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn portable_language_catalog_has_complete_contracts() {
    let modules = standard_modules();
    for metadata in &modules {
        let module = metadata.records();
        let definitions = metadata.definitions();
        module
            .validate(
                &|ty| namespaces::receiver_owner(ty, definitions),
                definitions,
            )
            .expect("valid language declarations");
        let items: Vec<_> = module
            .traits
            .iter()
            .cloned()
            .map(PublicItem::Trait)
            .chain(module.types.iter().cloned().map(PublicItem::Type))
            .collect();
        verify::validate(
            &items,
            &module.identity,
            &CancellationToken::default(),
            definitions,
        )
        .unwrap();
    }
    let definitions = modules[0].definitions();
    let traits: Vec<_> = modules
        .iter()
        .flat_map(|module| &module.records().traits)
        .collect();
    let types: Vec<_> = modules
        .iter()
        .flat_map(|module| &module.records().types)
        .collect();
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
            Some(t.declaration)
                == definitions.lookup_child(
                    Protocol::Iterable.definition(definitions).unwrap(),
                    DefinitionKind::AssociatedType,
                    "Iter",
                    0,
                )
        })
        .unwrap();
    assert!(
        matches!(&iter.bounds[..], [required] if Protocol::from_definition(&required.declaration, definitions) == Some(Protocol::Iterator))
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
                .map(|interface| Protocol::from_definition(&interface.declaration, definitions))
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
        .map(|module| {
            declaration_source(module, &standard_modules())
                .unwrap()
                .text
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(generated.contains("type Iter: Iterator<Item ="));
    assert!(generated.contains("trait FromIterator<T>"));
    assert!(generated.contains("fn from_iter<M>(source: M)"));
    assert!(generated.contains("fn sorted_by_key<M>"));
    assert!(generated.contains("fn retain("));
}
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn explicitly_installed_language_records_are_available() {
    let mut sources = SourceDatabase::default();
    sources
        .set(
            "language.kgr",
            "fn value(x: Option<i32>) -> Option<i32> { x }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(standard_modules());
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
            .declaration(&crate::declarations::DeclarationId::Definition(
                Protocol::Iterable
                    .definition(core.declarations().definitions())
                    .unwrap()
            ))
            .is_some()
    );
    let array = standard_modules()
        .into_iter()
        .flat_map(|module| module.into_parts().1.types)
        .find(|t| t.name == "Vec")
        .unwrap();
    assert!(matches!(
        array.generic_params[0].as_type(),
        Ty::Parameter { position: 0, .. }
    ));
}
*/

#[test]
fn portable_collection_view_preserves_concrete_iterator_proofs() {
    let context = DefinitionContext::new().unwrap();
    let core = verified_core(&context);
    let modules = adopt(&standard_modules(), &context, &Default::default()).unwrap();
    let definitions = &context.snapshot();
    let lookup = |id: &DefinitionId| {
        let view = definitions.resolve(*id).ok()?;
        if let Some(module) = core
            .program()
            .modules
            .iter()
            .find(|m| &m.identity == view.module())
        {
            return trait_contract(&module.identity, &module.public_items, &[], id, definitions);
        }
        modules
            .iter()
            .find(|module| &module.records().identity == view.module())
            .and_then(|module| {
                module.records().traits.iter().find(|contract| {
                    definitions.lookup(&library::trait_id(&contract.name)) == Some(*id)
                })
            })
    };
    let list = NominalTy {
        declaration: definitions.lookup(&library::trait_id("List")).unwrap(),
        arguments: vec![Ty::Builtin(BuiltinType::I32)],
        associated_types: Default::default(),
    };
    let receiver = Ty::Trait(list.clone());
    let cancel = CancellationToken::default();
    let original = inheritance::trait_closure(&list, &receiver, &cancel, &lookup).unwrap();
    let views =
        inheritance::interface_views(&list, &receiver, &cancel, definitions, &lookup).unwrap();
    let iter = definitions
        .lookup_child(
            Protocol::Iterable.definition(definitions).unwrap(),
            DefinitionKind::AssociatedType,
            "Iter",
            0,
        )
        .unwrap();
    let original = original
        .iter()
        .find(|parent| {
            Protocol::from_definition(&parent.declaration, definitions) == Some(Protocol::Iterable)
        })
        .unwrap();
    assert!(!original.associated_types.contains_key(&iter));
    let view = views
        .iter()
        .find(|parent| {
            Protocol::from_definition(&parent.declaration, definitions) == Some(Protocol::Iterable)
        })
        .unwrap();
    assert!(matches!(&view.associated_types[&iter], Ty::Trait(iterator)
        if Protocol::from_definition(&iterator.declaration, definitions) == Some(Protocol::Iterator)
            && iterator.associated_types[&definitions.lookup_child(iterator.declaration, DefinitionKind::AssociatedType, "Item", 0).unwrap()] == Ty::Builtin(BuiltinType::I32)));
    let mut actual = original.clone();
    actual
        .associated_types
        .insert(iter, Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))));
    let receiver = Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32)));
    assert_eq!(
        inheritance::interface_views(&actual, &receiver, &cancel, definitions, &lookup).unwrap()[0],
        actual
    );
}

fn diagnostics(text: &str) -> Vec<String> {
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("contracts.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(standard_modules());
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

/* Temporarily paused: stdlib restoration (NC01-NC06).
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
            val data: MutableList<i32> = Vec::from([1, 2]);
            data.push(3);
            val view: std::collections::List<i32> = data;
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
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
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
*/

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

/* Temporarily paused: stdlib restoration (NC01-NC06).

*/

/* Temporarily paused: stdlib restoration (NC01-NC06).

*/

#[test]
fn builtin_arrays_type_check_without_installed_library_declarations() {
    let mut sources = SourceDatabase::default();
    let file = sources.set("arrays-only.kgr", "fn first<T>(values: [T]) -> T { values[0] } fn main() -> i32 { val empty: [u8] = []; var data: [i32] = [1, 2]; val alias = data; alias[0] = 42; data = [9]; first(alias) }".into(), SourceLayer::Base).unwrap();
    let mut analysis = AnalysisDatabase::default();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let errors = snapshot.file(file).unwrap().result().diagnostics();
    assert!(errors.is_empty(), "{errors:#?}");
    snapshot.check_program(file, &Default::default()).unwrap();
}

#[test]
fn intrinsic_equality_without_providers_does_not_fabricate_nominal_views() {
    let definitions = DefinitionContext::new().unwrap().snapshot();
    let catalog = AggregateCatalog::default();
    let bounds = Default::default();
    let integer = TypeId::Builtin(BuiltinType::I32);
    let float = TypeId::Builtin(BuiltinType::F64);
    for source in [None, Some(&catalog)] {
        for protocol in [Protocol::Eq, Protocol::Hash, Protocol::PartialEq] {
            assert!(intrinsic_holds(
                &definitions,
                protocol,
                &integer,
                source,
                &bounds
            ));
            assert!(intrinsic_holds(
                &definitions,
                protocol,
                &TypeId::Tuple(vec![integer.clone()]),
                source,
                &bounds
            ));
            assert_eq!(
                intrinsic_holds(&definitions, protocol, &float, source, &bounds),
                protocol == Protocol::PartialEq
            );
            assert_eq!(protocol.nominal(&definitions), None);
        }
    }
}
