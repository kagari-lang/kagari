use super::*;
use crate::{contracts::verify_intrinsic, representation::ValueType, standard::StandardIntrinsic};

#[test]
fn numeric_contracts_preserve_width_sign_and_compound_results() {
    for (receiver, signed) in [
        (BuiltinType::U8, BuiltinType::I8),
        (BuiltinType::U16, BuiltinType::I16),
        (BuiltinType::U32, BuiltinType::I32),
        (BuiltinType::U64, BuiltinType::I64),
        (BuiltinType::USize, BuiltinType::ISize),
    ] {
        let offset =
            IntegerMethodContract::new(IntegerMethod::WrappingAddSigned, receiver).unwrap();
        assert_eq!(offset.parameters(), [receiver, signed]);
        assert_eq!(offset.result(), AbiType::Builtin(receiver));
        assert!(IntegerMethodContract::new(IntegerMethod::WrappingAddSigned, signed).is_none());
        for ty in [receiver, signed] {
            let rotate = IntegerMethodContract::new(IntegerMethod::RotateLeft, ty).unwrap();
            assert_eq!(rotate.parameters(), [ty, BuiltinType::U32]);
            assert_eq!(rotate.result(), AbiType::Builtin(ty));
            let checked = IntegerMethodContract::new(IntegerMethod::CheckedAdd, ty).unwrap();
            assert_eq!(
                checked.result(),
                AbiType::StandardEnum {
                    kind: StandardEnum::Option,
                    args: vec![AbiType::Builtin(ty)]
                }
            );
            let overflowing =
                IntegerMethodContract::new(IntegerMethod::OverflowingMul, ty).unwrap();
            assert_eq!(
                overflowing.result(),
                AbiType::Tuple(vec![
                    AbiType::Builtin(ty),
                    AbiType::Builtin(BuiltinType::Bool)
                ])
            );
        }
    }
    for receiver in [
        BuiltinType::Never,
        BuiltinType::Unit,
        BuiltinType::Bool,
        BuiltinType::F32,
        BuiltinType::F64,
        BuiltinType::String,
    ] {
        assert!(IntegerMethodContract::new(IntegerMethod::WrappingAdd, receiver).is_none());
    }
}

#[test]
fn physical_verification_rejects_wrong_numeric_arity_representations_and_bindings() {
    let cases = [
        (
            IntegerMethod::WrappingAdd,
            BuiltinType::I8,
            [ValueType::I32, ValueType::I32],
            ValueType::I32,
        ),
        (
            IntegerMethod::RotateRight,
            BuiltinType::U64,
            [ValueType::U64, ValueType::I64],
            ValueType::U64,
        ),
        (
            IntegerMethod::WrappingAddSigned,
            BuiltinType::USize,
            [ValueType::U64, ValueType::I64],
            ValueType::U64,
        ),
        (
            IntegerMethod::CheckedDiv,
            BuiltinType::I64,
            [ValueType::I64, ValueType::I64],
            ValueType::HeapObject,
        ),
        (
            IntegerMethod::OverflowingSub,
            BuiltinType::U8,
            [ValueType::I64, ValueType::I64],
            ValueType::HeapObject,
        ),
    ];
    for (method, receiver, parameters, result) in cases {
        let binding = StandardIntrinsic::Integer(method, receiver);
        assert!(verify_intrinsic(Some(result), binding, &parameters).is_ok());
        assert!(verify_intrinsic(Some(ValueType::Bool), binding, &parameters).is_err());
        assert!(verify_intrinsic(Some(result), binding, &parameters[..1]).is_err());
        let mut extra = parameters.to_vec();
        extra.push(parameters[0]);
        assert!(verify_intrinsic(Some(result), binding, &extra).is_err());
        for index in 0..2 {
            let mut wrong = parameters;
            wrong[index] = ValueType::Bool;
            assert!(verify_intrinsic(Some(result), binding, &wrong).is_err());
        }
    }
    assert!(
        verify_intrinsic(
            Some(ValueType::I32),
            StandardIntrinsic::Integer(IntegerMethod::WrappingAddSigned, BuiltinType::I32),
            &[ValueType::I32, ValueType::I32]
        )
        .is_err()
    );
    assert!(
        verify_intrinsic(
            Some(ValueType::F64),
            StandardIntrinsic::Integer(IntegerMethod::WrappingAdd, BuiltinType::F64),
            &[ValueType::F64, ValueType::F64]
        )
        .is_err()
    );
}

#[test]
fn engine_operation_arity_is_checked_before_operand_access() {
    for (binding, args, result) in [
        (
            StandardIntrinsic::ArrayListNew,
            vec![],
            ValueType::HeapObject,
        ),
        (
            StandardIntrinsic::StringLenChars,
            vec![ValueType::Str],
            ValueType::U64,
        ),
        (
            StandardIntrinsic::StringContains,
            vec![ValueType::Str, ValueType::Str],
            ValueType::Bool,
        ),
        (
            StandardIntrinsic::StringSlice,
            vec![ValueType::Str, ValueType::U64, ValueType::U64],
            ValueType::HeapObject,
        ),
        (
            StandardIntrinsic::StringReplaceN,
            vec![
                ValueType::Str,
                ValueType::Str,
                ValueType::Str,
                ValueType::U64,
            ],
            ValueType::Str,
        ),
        (
            StandardIntrinsic::KeyMapInsert,
            vec![
                ValueType::HeapObject,
                ValueType::I64,
                ValueType::I64,
                ValueType::Str,
                ValueType::I32,
            ],
            ValueType::HeapObject,
        ),
    ] {
        assert!(
            verify_intrinsic(Some(result), binding, &args).is_ok(),
            "{binding:?}"
        );
        for length in 0..args.len() {
            assert!(
                verify_intrinsic(Some(result), binding, &args[..length]).is_err(),
                "{binding:?}"
            );
        }
        let mut extra = args;
        extra.push(ValueType::Unit);
        assert!(
            verify_intrinsic(Some(result), binding, &extra).is_err(),
            "{binding:?}"
        );
    }
}
