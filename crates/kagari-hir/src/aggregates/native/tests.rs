use crate::native::NativeBinding;
use crate::{
    aggregates::AggregateCatalog,
    analysis::{AnalysisDatabase, AnalysisSnapshot},
    builtin::traits::{self, StandardTraitSemantics},
    native::NativeTypeKind,
    typeck::FunctionImplementation,
    types::{TypeId, TypeSubstitution},
};
use kagari_abi::{
    scalar::BuiltinType,
    standard::{surface::StandardEnum, traits::StandardTrait},
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
fn native_capabilities_require_installed_impls_and_preserve_readonly_access() {
    let (snapshot, root) = snapshot("fn main() {}");
    let catalog = &snapshot.file(root).unwrap().result().facts().aggregates;
    let item = TypeId::Builtin(BuiltinType::I32);
    let mutable = TypeId::Array(Box::new(item.clone()), CollectionAccess::Mutable);
    let readonly = TypeId::Array(Box::new(item.clone()), CollectionAccess::ReadOnly);
    for (kind, writable) in [
        (StandardTrait::List, false),
        (StandardTrait::MutableList, true),
    ] {
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
        assert_eq!(
            catalog
                .engine_implementation(&interface, &readonly, &Default::default())
                .is_some(),
            !writable
        );
        assert_eq!(
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
                .is_some(),
            !writable
        );
    }
    assert!(
        traits::iteration_outputs(
            StandardTrait::Iterator,
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
    let mut interface = StandardTrait::FromIterator.nominal();
    interface
        .arguments
        .push(TypeId::Tuple(vec![key, TypeId::Builtin(BuiltinType::I32)]));
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
    let interface = StandardTrait::Iterator.nominal();
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
    assert!(!traits::intrinsic_applies(
        &interface,
        receiver,
        Some(&facts.aggregates),
        &Default::default()
    ));
    let outputs = traits::iteration_outputs(
        StandardTrait::Iterable,
        receiver,
        Some(&facts.aggregates),
        &Default::default(),
    )
    .unwrap();
    let iterable = StandardTrait::Iterable.nominal();
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
                FunctionImplementation::Native(NativeBinding::Engine(_))
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
