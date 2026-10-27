use crate::tests::native_type;
use crate::{
    aggregates::{AggregateCatalog, implementations::ImplementationSearchLimits},
    analysis::AnalysisSnapshot,
    language::{semantics as traits, semantics::ProtocolSemantics},
    native::NativeBinding,
    tests::test_analysis,
    typeck::FunctionImplementation,
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_common::identity::{DefinitionKind, table::DefinitionTable};
use kagari_source::{
    identity::FileId,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::identity as library;
use kagari_types::{language::Protocol, scalar::BuiltinType};

fn foundation_interface(definitions: &DefinitionTable, name: &str) -> NominalType {
    NominalType {
        declaration: definitions.lookup(&library::trait_id(name)).unwrap(),
        arguments: vec![],
        associated_types: Default::default(),
    }
}

fn snapshot(text: &str) -> (AnalysisSnapshot, FileId) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("native-contracts.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    (snapshot, root)
}

#[test]
fn native_receivers_require_installed_impls_and_exact_host_access() {
    let (snapshot, root) = snapshot("fn main() {}");
    let authoring_catalog = snapshot
        .file(root)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let definitions = authoring_catalog.definitions();
    let catalog = &authoring_catalog.records().facts().aggregates;
    let item = TypeId::Builtin(BuiltinType::I32);
    let mutable = TypeId::NativeObject(NominalType {
        declaration: definitions
            .lookup(&library::type_id("Vec", DefinitionKind::AssociatedType))
            .unwrap(),
        arguments: vec![item.clone()],
        associated_types: Default::default(),
    });
    let array = TypeId::Array(Box::new(item.clone()));
    for kind in ["List", "MutableList"] {
        let mut interface = foundation_interface(definitions, kind);
        interface.arguments.push(item.clone());
        assert!(
            AggregateCatalog::default()
                .engine_implementation(definitions, &interface, &mutable, &Default::default())
                .is_none()
        );
        assert!(!traits::intrinsic_applies(
            definitions,
            &interface,
            &mutable,
            None,
            &Default::default()
        ));
        assert!(
            catalog
                .engine_implementation(definitions, &interface, &mutable, &Default::default())
                .is_some()
        );
        // Fixed arrays cannot acquire List/MutableList from shared buffer layout.
        assert!(
            catalog
                .engine_implementation(definitions, &interface, &array, &Default::default())
                .is_none()
        );
        assert!(
            catalog
                .concrete_interface_implementation(
                    definitions,
                    &interface,
                    &array,
                    &Default::default(),
                    ImplementationSearchLimits {
                        max_checks: 4096,
                        max_depth: 64
                    },
                    &Default::default()
                )
                .unwrap()
                .is_none()
        );
    }
    assert!(
        traits::iteration_outputs(
            definitions,
            Protocol::Iterator,
            &TypeId::Iter(Box::new(item)),
            None,
            &Default::default()
        )
        .is_none()
    );
    let key = TypeId::Builtin(BuiltinType::F64);
    let target = native_type(
        definitions,
        "HashMap",
        vec![key.clone(), TypeId::Builtin(BuiltinType::I32)],
    );
    let mut interface = foundation_interface(definitions, "Map");
    interface.arguments = vec![key, TypeId::Builtin(BuiltinType::I32)];
    assert!(
        catalog
            .engine_implementation_pattern(&interface, &target)
            .is_some()
    );
    assert!(
        catalog
            .engine_implementation(definitions, &interface, &target, &Default::default())
            .is_none()
    );
}

#[test]
fn algorithm_trait_names_are_ordinary_user_contracts() {
    let (snapshot, root) = snapshot(
        "trait Sum<T> { fn sum(self, value: T) -> T; } struct Values {} impl Sum<i32> for Values { fn sum(self, value: i32) -> i32 { value } }",
    );
    let authoring_facts = snapshot
        .file(root)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let definitions = authoring_facts.definitions();
    let facts = authoring_facts.records().facts();
    let implementation = facts
        .aggregates
        .implementations()
        .find(|item| !item.engine_owned)
        .unwrap();
    let interface = &implementation.trait_type;
    assert!(Protocol::from_definition(&interface.declaration, definitions).is_none());
    assert!(
        facts
            .aggregates
            .engine_implementation(
                definitions,
                interface,
                &implementation.for_type,
                &Default::default()
            )
            .is_none()
    );
    assert!(
        facts
            .aggregates
            .concrete_interface_implementation(
                definitions,
                interface,
                &implementation.for_type,
                &Default::default(),
                ImplementationSearchLimits {
                    max_checks: 4096,
                    max_depth: 64
                },
                &Default::default()
            )
            .unwrap()
            .is_some()
    );
    let mut wrong = interface.clone();
    wrong.arguments[0] = TypeId::Builtin(BuiltinType::Bool);
    assert!(
        facts
            .aggregates
            .concrete_interface_implementation(
                definitions,
                &wrong,
                &implementation.for_type,
                &Default::default(),
                ImplementationSearchLimits {
                    max_checks: 4096,
                    max_depth: 64
                },
                &Default::default()
            )
            .unwrap()
            .is_none()
    );
    for kind in ["Sum", "Product", "FromIterator"] {
        let contract = facts
            .aggregates
            .trait_(&definitions.lookup(&library::trait_id(kind)).unwrap())
            .expect("foundation construction contract");
        assert_eq!(contract.methods.len(), 1);
        assert_eq!(contract.methods[0].generic_params.len(), 2);
    }
}

#[test]
fn source_iterator_implementation_is_not_native_dispatch() {
    let (snapshot, root) = snapshot(
        r#"
struct Cursor {}
impl Iterator for Cursor {
    type Item = i32;
    fn next(self) -> Option<i32> { None }
}
fn main() {}
"#,
    );
    let authoring_facts = snapshot
        .file(root)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let definitions = authoring_facts.definitions();
    let facts = authoring_facts.records().facts();
    let implementation = facts
        .aggregates
        .implementations()
        .find(|implementation| !implementation.engine_owned)
        .unwrap();
    let receiver = &implementation.for_type;
    let interface = Protocol::Iterator.nominal(definitions).unwrap();
    assert!(
        facts
            .aggregates
            .concrete_interface_implementation(
                definitions,
                &interface,
                receiver,
                &Default::default(),
                ImplementationSearchLimits {
                    max_checks: 4096,
                    max_depth: 64
                },
                &Default::default()
            )
            .unwrap()
            .is_some()
    );
    assert!(
        facts
            .aggregates
            .engine_implementation(definitions, &interface, receiver, &Default::default())
            .is_none()
    );
    // Iteration is a language protocol, even when its selected body is script.
    assert!(traits::intrinsic_applies(
        definitions,
        &interface,
        receiver,
        Some(&facts.aggregates),
        &Default::default()
    ));
    let outputs = traits::iteration_outputs(
        definitions,
        Protocol::Iterable,
        receiver,
        Some(&facts.aggregates),
        &Default::default(),
    )
    .unwrap();
    let iterable = Protocol::Iterable.nominal(definitions).unwrap();
    assert_eq!(
        outputs[&definitions
            .lookup_child(
                iterable.declaration,
                DefinitionKind::AssociatedType,
                "Item",
                0
            )
            .unwrap()],
        TypeId::Builtin(BuiltinType::I32)
    );
    assert_eq!(
        outputs[&definitions
            .lookup_child(
                iterable.declaration,
                DefinitionKind::AssociatedType,
                "Iter",
                0
            )
            .unwrap()],
        *receiver
    );
}

#[test]
fn every_native_signature_retains_resolved_public_types() {
    let signatures = test_analysis()
        .signatures(SourceDatabase::default().snapshot(), &Default::default())
        .unwrap();
    let mut count = 0;
    for source in signatures.declaration_snapshot().files() {
        let file = signatures.file(source.source().id()).unwrap();
        assert!(
            file.diagnostics().is_empty(),
            "{}: {:?}",
            source.source().name(),
            file.diagnostics()
        );
        for function in file.signatures().facts().functions() {
            if !matches!(
                function.implementation,
                FunctionImplementation::Native(NativeBinding::Entry(_))
            ) {
                continue;
            }
            let substitution: TypeSubstitution = function
                .generic_params
                .iter()
                .map(|parameter| {
                    let ty = if parameter.name == "I" {
                        TypeId::Array(Box::new(TypeId::Builtin(BuiltinType::I32)))
                    } else {
                        TypeId::Builtin(BuiltinType::I32)
                    };
                    (parameter.clone(), ty)
                })
                .collect();
            for parameter in &function.params {
                assert!(
                    !parameter.ty.instantiate(&substitution).is_unresolved(),
                    "{}::{}",
                    function.name,
                    parameter.name
                );
            }
            assert!(
                !function
                    .return_type
                    .instantiate(&substitution)
                    .is_unresolved(),
                "{}",
                function.name
            );
            count += 1;
        }
    }
    assert!(count > 0);
}

#[test]
fn checked_native_enum_signatures_preserve_slots_and_payloads() {
    let (snapshot, root) = snapshot("fn main() {}");
    let authoring_catalog = snapshot
        .file(root)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let definitions = authoring_catalog.definitions();
    let catalog = &authoring_catalog.records().facts().aggregates;
    for (kind, arity, expected) in [
        ("Option", 1, vec![("Some", 1), ("None", 0)]),
        ("Result", 2, vec![("Ok", 1), ("Err", 1)]),
        (
            "Ordering",
            0,
            vec![("Less", 0), ("Equal", 0), ("Greater", 0)],
        ),
    ] {
        let enumeration = catalog
            .enumerations()
            .find(|item| {
                definitions
                    .resolve(item.id)
                    .unwrap()
                    .last()
                    .is_some_and(|part| part.name == kind)
            })
            .unwrap();
        assert_eq!(enumeration.generic_params.len(), arity);
        assert_eq!(
            enumeration
                .variants
                .iter()
                .map(|variant| (variant.name.as_str(), variant.payload.len()))
                .collect::<Vec<_>>(),
            expected
        );
        for (slot, variant) in enumeration.variants.iter().enumerate() {
            assert_eq!(variant.slot, slot);
        }
    }
}
