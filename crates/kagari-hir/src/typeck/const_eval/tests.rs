use super::*;

#[test]
fn checked_constants_share_integer_semantics_at_every_width() {
    for ty in [
        IntegerType::I8,
        IntegerType::I16,
        IntegerType::I32,
        IntegerType::I64,
        IntegerType::ISize,
        IntegerType::U8,
        IntegerType::U16,
        IntegerType::U32,
        IntegerType::U64,
        IntegerType::USize,
    ] {
        let scalar = |value| ScalarValue::integer(value, ty).unwrap();
        let (min, max) = ty.bounds();
        for (op, lhs, rhs, expected) in [
            (BinaryOp::Add, 40, 2, Ok(scalar(42))),
            (BinaryOp::Sub, 44, 2, Ok(scalar(42))),
            (BinaryOp::Mul, 6, 7, Ok(scalar(42))),
            (BinaryOp::Div, 84, 2, Ok(scalar(42))),
            (BinaryOp::Rem, 43, 2, Ok(scalar(1))),
            (BinaryOp::Add, max, 1, Err("integer overflow")),
            (BinaryOp::Sub, min, 1, Err("integer overflow")),
            (BinaryOp::Mul, max, 2, Err("integer overflow")),
            (BinaryOp::Div, 1, 0, Err("integer division by zero")),
            (BinaryOp::Rem, 1, 0, Err("integer remainder by zero")),
            (BinaryOp::BitAnd, 43, 42, Ok(scalar(42))),
            (BinaryOp::Shl, 21, 1, Ok(scalar(42))),
            (BinaryOp::Shr, 84, 1, Ok(scalar(42))),
            (BinaryOp::Eq, max, max, Ok(ScalarValue::Bool(true))),
            (BinaryOp::Lt, min, max, Ok(ScalarValue::Bool(true))),
        ] {
            assert_eq!(
                binary(op, scalar(lhs), scalar(rhs)),
                Some(expected),
                "{ty:?} {op:?}"
            );
        }
        let (bits, signed) = ty.layout();
        assert_eq!(
            binary(BinaryOp::Shl, scalar(1), scalar(i128::from(bits))),
            Some(Err("integer shift out of range"))
        );
        assert_eq!(
            integer_value(IntegerOp::BitNot, 0, 0, ty),
            Ok(scalar(if signed { -1 } else { max }))
        );
        if signed {
            for op in [BinaryOp::Div, BinaryOp::Rem] {
                assert_eq!(
                    binary(op, scalar(min), scalar(-1)),
                    Some(Err("integer overflow"))
                );
            }
            assert_eq!(
                integer_value(IntegerOp::CheckedSub, 0, min, ty),
                Err("integer overflow")
            );
        }
    }
}
