use crate::{
    aggregates::AggregateCatalog,
    analysis::{AnalysisDatabase, AnalysisSnapshot},
    language::semantics::{self as traits, ProtocolSemantics},
    native::{NativeBinding, NativeTypeKind},
    typeck::FunctionImplementation,
    types::{TypeId, TypeSubstitution},
};

use kagari_abi::{
    language::{self, Protocol},
    scalar::BuiltinType,
    standard::surface::StandardEnum,
};
use kagari_common::{
    collection::CollectionAccess,
    identity::FileId,
    source_database::{SourceDatabase, SourceLayer},
};

fn snapshot(text: &str) -> (AnalysisSnapshot, FileId) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("native-contracts.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    (snapshot, root)
}

#[test]
fn native_capabilities_require_installed_impls_and_declared_storage_access() {
    let (snapshot, root) = snapshot("fn main() {}");
    let catalog = &snapshot.file(root).unwrap().result().facts().aggregates;
    let item = TypeId::Builtin(BuiltinType::I32);
    let mutable = TypeId::Array(Box::new(item.clone()), CollectionAccess::Mutable);
    let readonly = TypeId::Array(Box::new(item.clone()), CollectionAccess::ReadOnly);
    for kind in [Protocol::List, Protocol::MutableList] {
        let mut interface = kind.nominal();
        interface.arguments.push(item.clone());
        assert!(
            AggregateCatalog::default()
                .engine_implementation(&interface, &mutable, &Default::default())
                .is_none()
        );
        assert!(!traits::intrinsic_applies(
            &interface,
            &mutable,
            None,
            &Default::default()
        ));
        assert!(
            catalog
                .engine_implementation(&interface, &mutable, &Default::default())
                .is_some()
        );
        // Readonly collection surfaces are declared trait views, not a second
        // physical ArrayList layout with another set of native implementations.
        assert!(
            catalog
                .engine_implementation(&interface, &readonly, &Default::default())
                .is_none()
        );
        assert!(
            catalog
                .concrete_interface_implementation(
                    &interface,
                    &readonly,
                    &Default::default(),
                    4096,
                    64,
                    &Default::default()
                )
                .unwrap()
                .is_none()
        );
    }
    assert!(
        traits::iteration_outputs(
            Protocol::Iterator,
            &TypeId::Iter(Box::new(item)),
            None,
            &Default::default()
        )
        .is_none()
    );
    let key = TypeId::Builtin(BuiltinType::F64);
    let target = TypeId::Map {
        key: Box::new(key.clone()),
        value: Box::new(TypeId::Builtin(BuiltinType::I32)),
        access: CollectionAccess::Mutable,
    };
    let mut interface = Protocol::Map.nominal();
    interface.arguments = vec![key, TypeId::Builtin(BuiltinType::I32)];
    assert!(
        catalog
            .engine_implementation_pattern(&interface, &target)
            .is_some()
    );
    assert!(
        catalog
            .engine_implementation(&interface, &target, &Default::default())
            .is_none()
    );
}

#[test]
fn algorithm_trait_names_are_ordinary_user_contracts() {
    let (snapshot, root) = snapshot(
        "trait Sum<T> { fn sum(self, value: T) -> T; } struct Values {} impl Sum<i32> for Values { fn sum(self, value: i32) -> i32 { value } }",
    );
    let facts = snapshot.file(root).unwrap().result().facts();
    let implementation = facts
        .aggregates
        .implementations()
        .find(|item| !item.engine_owned)
        .unwrap();
    let interface = &implementation.trait_type;
    assert!(Protocol::from_id(&interface.declaration).is_none());
    assert!(
        facts
            .aggregates
            .engine_implementation(interface, &implementation.for_type, &Default::default())
            .is_none()
    );
    assert!(
        facts
            .aggregates
            .concrete_interface_implementation(
                interface,
                &implementation.for_type,
                &Default::default(),
                4096,
                64,
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
                &wrong,
                &implementation.for_type,
                &Default::default(),
                4096,
                64,
                &Default::default()
            )
            .unwrap()
            .is_none()
    );
    for kind in [Protocol::Sum, Protocol::Product, Protocol::FromIterator] {
        let contract = facts
            .aggregates
            .trait_(&language::identity(kind))
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
    let facts = snapshot.file(root).unwrap().result().facts();
    let implementation = facts
        .aggregates
        .implementations()
        .find(|implementation| !implementation.engine_owned)
        .unwrap();
    let receiver = &implementation.for_type;
    let interface = Protocol::Iterator.nominal();
    assert!(
        facts
            .aggregates
            .concrete_interface_implementation(
                &interface,
                receiver,
                &Default::default(),
                4096,
                64,
                &Default::default()
            )
            .unwrap()
            .is_some()
    );
    assert!(
        facts
            .aggregates
            .engine_implementation(&interface, receiver, &Default::default())
            .is_none()
    );
    // Iteration is a language protocol, even when its selected body is script.
    assert!(traits::intrinsic_applies(
        &interface,
        receiver,
        Some(&facts.aggregates),
        &Default::default()
    ));
    let outputs = traits::iteration_outputs(
        Protocol::Iterable,
        receiver,
        Some(&facts.aggregates),
        &Default::default(),
    )
    .unwrap();
    let iterable = Protocol::Iterable.nominal();
    assert_eq!(
        outputs[&kagari_common::identity::associated_type_id(&iterable.declaration, "Item")],
        TypeId::Builtin(BuiltinType::I32)
    );
    assert_eq!(
        outputs[&kagari_common::identity::associated_type_id(&iterable.declaration, "Iter")],
        *receiver
    );
}

#[test]
fn every_native_signature_retains_resolved_public_types() {
    let signatures = AnalysisDatabase::default()
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
                        TypeId::Array(
                            Box::new(TypeId::Builtin(BuiltinType::I32)),
                            CollectionAccess::Mutable,
                        )
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
    let catalog = &snapshot.file(root).unwrap().result().facts().aggregates;
    for (kind, arity, expected) in [
        (StandardEnum::Option, 1, vec![("Some", 1), ("None", 0)]),
        (StandardEnum::Result, 2, vec![("Ok", 1), ("Err", 1)]),
        (
            StandardEnum::Ordering,
            0,
            vec![("Less", 0), ("Equal", 0), ("Greater", 0)],
        ),
    ] {
        let enumeration = catalog
            .enumerations()
            .find(|item| item.native_type == Some(NativeTypeKind::Enum(kind)))
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
