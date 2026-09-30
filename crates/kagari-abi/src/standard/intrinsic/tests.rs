use super::*;
use crate::standard::surface::StandardEnum;
use kagari_common::collection::CollectionAccess;

fn scalar(kind: BuiltinType) -> AbiType {
    AbiType::Builtin(kind)
}

#[test]
fn numeric_and_callable_outputs_keep_exact_argument_contracts() {
    let cancel = CancellationToken::default();
    let lhs = scalar(BuiltinType::I16);
    let shift = applied(StandardTrait::Shl, vec![scalar(BuiltinType::U8)]);
    assert_eq!(requirements(&shift, &lhs, &cancel).unwrap(), Some(vec![]));
    let mismatched = applied(StandardTrait::Add, shift.arguments.clone());
    assert_eq!(requirements(&mismatched, &lhs, &cancel).unwrap(), None);
    let function = AbiType::Function {
        params: vec![],
        result: Box::new(lhs.clone()),
    };
    let mut callable = applied(StandardTrait::Fn, vec![scalar(BuiltinType::Unit)]);
    let output = associated_type_id(&callable.declaration, "Output");
    callable
        .associated_types
        .insert(output.clone(), lhs.clone());
    assert_eq!(
        requirements(&callable, &function, &cancel).unwrap(),
        Some(vec![])
    );
    callable.arguments[0] = AbiType::Tuple(vec![]);
    assert_eq!(requirements(&callable, &function, &cancel).unwrap(), None);
    callable.arguments[0] = scalar(BuiltinType::Unit);
    callable
        .associated_types
        .insert(output, scalar(BuiltinType::Bool));
    assert_eq!(requirements(&callable, &function, &cancel).unwrap(), None);
}

#[test]
fn reverse_conversion_preserves_error_binding_and_reverses_the_receiver() {
    let cancel = CancellationToken::default();
    let source = scalar(BuiltinType::I64);
    let target = scalar(BuiltinType::I8);
    let mut reverse = applied(StandardTrait::TryInto, vec![target.clone()]);
    let error = AbiType::StandardEnum {
        kind: StandardEnum::TryFromIntError,
        args: vec![],
    };
    reverse.associated_types.insert(
        associated_type_id(&reverse.declaration, "Error"),
        error.clone(),
    );
    let [bound] = requirements(&reverse, &source, &cancel)
        .unwrap()
        .unwrap()
        .try_into()
        .unwrap();
    let bound: GenericBoundAbi = bound;
    assert_eq!(bound.ty, target);
    let [ConstraintAbi::Trait(required)] = bound.constraints.as_slice() else {
        panic!("forward conversion obligation");
    };
    assert_eq!(required.arguments, vec![source]);
    assert_eq!(
        required
            .associated_types
            .get(&associated_type_id(&required.declaration, "Error")),
        Some(&error)
    );
    assert_eq!(
        requirements(required, &bound.ty, &cancel).unwrap(),
        Some(vec![])
    );
    reverse
        .associated_types
        .insert(associated_type_id(&reverse.declaration, "Other"), error);
    assert_eq!(
        requirements(&reverse, &scalar(BuiltinType::I64), &cancel).unwrap(),
        None
    );
}

#[test]
fn native_storage_requires_carried_implementations_instead_of_implicit_proofs() {
    let cancel = CancellationToken::default();
    let item = scalar(BuiltinType::I32);
    let destination = AbiType::Set(Box::new(item.clone()), CollectionAccess::Mutable);
    let collect = applied(StandardTrait::FromIterator, vec![item.clone()]);
    assert_eq!(requirements(&collect, &destination, &cancel).unwrap(), None);
    let wrapped = AbiType::StandardEnum {
        kind: StandardEnum::Option,
        args: vec![destination],
    };
    assert_eq!(requirements(&collect, &wrapped, &cancel).unwrap(), None);
    let iterator = applied(StandardTrait::Iterator, vec![]);
    let receiver = AbiType::Iter(Box::new(item));
    assert_eq!(requirements(&iterator, &receiver, &cancel).unwrap(), None);
}

#[test]
fn identity_iteration_requires_iterator_and_validates_outputs() {
    let cancel = CancellationToken::default();
    let item = scalar(BuiltinType::Bool);
    let receiver = AbiType::Iter(Box::new(item.clone()));
    let mut interface = applied(StandardTrait::Iterable, vec![]);
    interface.associated_types.insert(
        associated_type_id(&interface.declaration, "Iter"),
        receiver.clone(),
    );
    interface.associated_types.insert(
        associated_type_id(&interface.declaration, "Item"),
        item.clone(),
    );
    let required = requirements(&interface, &receiver, &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(required.len(), 1);
    let ConstraintAbi::Trait(iterator) = &required[0].constraints[0] else {
        panic!("iterator proof");
    };
    assert_eq!(requirements(iterator, &receiver, &cancel).unwrap(), None);
    for member in interface.associated_types.keys() {
        assert_eq!(
            associated_output(&interface, &receiver, member, &cancel).unwrap(),
            None
        );
    }
    interface
        .associated_types
        .insert(associated_type_id(&interface.declaration, "Iter"), item);
    assert_eq!(requirements(&interface, &receiver, &cancel).unwrap(), None);
    cancel.cancel();
    assert_eq!(
        requirements(&interface, &receiver, &cancel),
        Err(TypeTransformError::Cancelled)
    );
}
