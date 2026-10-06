use super::*;

#[test]
fn integer_literals_keep_their_type_and_full_range_in_one_representation() {
    for (ty, min, max) in [
        (IntegerType::I8, i128::from(i8::MIN), i128::from(i8::MAX)),
        (IntegerType::I16, i128::from(i16::MIN), i128::from(i16::MAX)),
        (IntegerType::I32, i128::from(i32::MIN), i128::from(i32::MAX)),
        (IntegerType::I64, i128::from(i64::MIN), i128::from(i64::MAX)),
        (
            IntegerType::ISize,
            i128::from(i64::MIN),
            i128::from(i64::MAX),
        ),
        (IntegerType::U8, 0, i128::from(u8::MAX)),
        (IntegerType::U16, 0, i128::from(u16::MAX)),
        (IntegerType::U32, 0, i128::from(u32::MAX)),
        (IntegerType::U64, 0, i128::from(u64::MAX)),
        (IntegerType::USize, 0, i128::from(u64::MAX)),
    ] {
        for value in [min, max, 0, 42] {
            let literal = Literal {
                kind: LiteralKind::Number,
                text: value.abs().to_string(),
            };
            let expected = ScalarValue::Integer { value, ty };
            assert_eq!(
                ScalarValue::parse_expected(&literal, Some(ty.builtin_type()), value < 0),
                Ok(expected.clone()),
            );
            assert_eq!(expected.ty(), TypeId::Builtin(ty.builtin_type()));
            assert_eq!(ScalarValue::integer(value, ty), Ok(expected));
        }
        for value in [min - 1, max + 1] {
            assert!(ScalarValue::integer(value, ty).is_err(), "{ty:?}: {value}");
        }
    }
    assert_eq!(
        ScalarValue::parse(&Literal {
            kind: LiteralKind::Number,
            text: "42".into()
        }),
        ScalarValue::integer(42, IntegerType::I32),
    );
    for ty in [
        BuiltinType::Never,
        BuiltinType::Unit,
        BuiltinType::Bool,
        BuiltinType::F32,
        BuiltinType::F64,
        BuiltinType::String,
    ] {
        assert_eq!(IntegerType::from_builtin(ty), None);
    }
}

#[test]
fn integer_casts_preserve_wrapping_and_unsigned_precision() {
    let maximum = ScalarValue::integer(i128::from(u64::MAX), IntegerType::U64).unwrap();
    assert_eq!(
        maximum.clone().cast_numeric(BuiltinType::I32),
        ScalarValue::integer(-1, IntegerType::I32).ok(),
    );
    assert_eq!(
        maximum.clone().cast_numeric(BuiltinType::U64),
        Some(maximum)
    );
    assert_eq!(
        ScalarValue::integer(-1, IntegerType::I32)
            .unwrap()
            .cast_numeric(BuiltinType::U64),
        ScalarValue::integer(i128::from(u64::MAX), IntegerType::U64).ok(),
    );
    assert_eq!(
        ScalarValue::F64(f64::INFINITY).cast_numeric(BuiltinType::I32),
        ScalarValue::integer(i128::from(i32::MAX), IntegerType::I32).ok(),
    );
    assert_eq!(
        ScalarValue::Bool(true).cast_numeric(BuiltinType::I32),
        ScalarValue::integer(1, IntegerType::I32).ok(),
    );
}
