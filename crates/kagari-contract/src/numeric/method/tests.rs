use super::*;

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
        assert_eq!(offset.result(), Ty::Builtin(receiver));
        assert!(IntegerMethodContract::new(IntegerMethod::WrappingAddSigned, signed).is_none());
        for ty in [receiver, signed] {
            let rotate = IntegerMethodContract::new(IntegerMethod::RotateLeft, ty).unwrap();
            assert_eq!(rotate.parameters(), [ty, BuiltinType::U32]);
            assert_eq!(rotate.result(), Ty::Builtin(ty));
            let checked = IntegerMethodContract::new(IntegerMethod::CheckedAdd, ty).unwrap();
            assert_eq!(
                checked.result(),
                Ty::StandardEnum {
                    kind: StandardEnum::Option,
                    args: vec![Ty::Builtin(ty)]
                }
            );
            let overflowing =
                IntegerMethodContract::new(IntegerMethod::OverflowingMul, ty).unwrap();
            assert_eq!(
                overflowing.result(),
                Ty::Tuple(vec![Ty::Builtin(ty), Ty::Builtin(BuiltinType::Bool)])
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
