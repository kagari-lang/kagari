use super::*;
use crate::{scalar::BuiltinType, standard::intrinsic};

fn scalar() -> AbiType {
    AbiType::Builtin(BuiltinType::I32)
}

#[test]
fn storage_adapters_preserve_access_and_invariant_arguments() {
    let readonly = AbiType::Array(Box::new(scalar()), CollectionAccess::ReadOnly);
    let mutable = AbiType::Array(Box::new(scalar()), CollectionAccess::Mutable);
    let list = intrinsic::applied(StandardTrait::List, vec![scalar()]);
    let writable = intrinsic::applied(StandardTrait::MutableList, vec![scalar()]);
    assert!(interface_applies(&list, &readonly));
    assert!(interface_applies(&list, &mutable));
    assert!(!interface_applies(&writable, &readonly));
    assert!(interface_applies(&writable, &mutable));
    assert!(!interface_applies(
        &intrinsic::applied(
            StandardTrait::List,
            vec![AbiType::Builtin(BuiltinType::Bool)]
        ),
        &mutable,
    ));
    assert!(!interface_applies(&list, &AbiType::Trait(list.clone())));
}

#[test]
fn iteration_and_index_adapters_reject_forged_payloads_and_bindings() {
    let storage = AbiType::Array(Box::new(scalar()), CollectionAccess::ReadOnly);
    let mut iterable = intrinsic::applied(StandardTrait::Iterable, vec![]);
    iterable
        .associated_types
        .insert(associated_type_id(&iterable.declaration, "Item"), scalar());
    iterable.associated_types.insert(
        associated_type_id(&iterable.declaration, "Iter"),
        AbiType::Iter(Box::new(scalar())),
    );
    assert!(interface_applies(&iterable, &storage));
    iterable.associated_types.insert(
        associated_type_id(&iterable.declaration, "Item"),
        AbiType::Builtin(BuiltinType::Bool),
    );
    assert!(!interface_applies(&iterable, &storage));
    let mut index = intrinsic::applied(
        StandardTrait::Index,
        vec![AbiType::Builtin(BuiltinType::U64)],
    );
    index
        .associated_types
        .insert(associated_type_id(&index.declaration, "Output"), scalar());
    assert!(interface_applies(&index, &storage));
    index.arguments[0] = AbiType::Builtin(BuiltinType::F64);
    assert!(!interface_applies(&index, &storage));
    index.arguments[0] = AbiType::Builtin(BuiltinType::I32);
    index
        .associated_types
        .insert(associated_type_id(&index.declaration, "Unknown"), scalar());
    assert!(!interface_applies(&index, &storage));
    let iterator = AbiType::Iter(Box::new(scalar()));
    assert!(!interface_applies(
        &intrinsic::applied(StandardTrait::Iterator, vec![]),
        &iterator
    ));
}
