use super::*;

const INTEGERS: [BuiltinType; 10] = [
    BuiltinType::I8,
    BuiltinType::I16,
    BuiltinType::I32,
    BuiltinType::I64,
    BuiltinType::ISize,
    BuiltinType::U8,
    BuiltinType::U16,
    BuiltinType::U32,
    BuiltinType::U64,
    BuiltinType::USize,
];

#[test]
fn prepared_integer_operations_keep_domains_and_failures() {
    let ops = [
        IntegerOp::CheckedAdd,
        IntegerOp::CheckedSub,
        IntegerOp::CheckedMul,
        IntegerOp::CheckedDiv,
        IntegerOp::CheckedRem,
        IntegerOp::BitAnd,
        IntegerOp::BitOr,
        IntegerOp::BitXor,
        IntegerOp::BitNot,
    ];
    for ty in INTEGERS {
        let (bits, signed) = ty.integer_layout().unwrap();
        let (min, max) = ty.integer_bounds().unwrap();
        for op in ops {
            let kernel = integer_kernel(
                op,
                ty,
                if op == IntegerOp::BitNot {
                    None
                } else {
                    Some(ty)
                },
            )
            .unwrap();
            for lhs in [min, min + 1, 0, 1, max] {
                for rhs in [min, 0, 1, max] {
                    let expected =
                        integer::integer_operation(op, lhs, rhs, bits, signed).map(|v| v as u64);
                    assert_eq!(
                        kernel(lhs as u64, rhs as u64),
                        expected,
                        "{ty:?}/{op:?}/{lhs}/{rhs}"
                    );
                }
            }
        }
        for rhs_ty in INTEGERS {
            let (_, rhs_max) = rhs_ty.integer_bounds().unwrap();
            for op in [IntegerOp::Shl, IntegerOp::Shr] {
                let kernel = integer_kernel(op, ty, Some(rhs_ty)).unwrap();
                for rhs in [0, 1, i128::from(bits - 1), i128::from(bits), rhs_max] {
                    let expected =
                        integer::integer_operation(op, max, rhs, bits, signed).map(|v| v as u64);
                    assert_eq!(
                        kernel(max as u64, rhs as u64),
                        expected,
                        "{ty:?}/{rhs_ty:?}/{op:?}/{rhs}"
                    );
                }
            }
        }
    }
    let invalid = Err("invalid numeric operand type or range");
    assert_eq!(
        integer_kernel(
            IntegerOp::CheckedAdd,
            BuiltinType::I8,
            Some(BuiltinType::I8)
        )
        .unwrap()(128, 0),
        invalid
    );
    assert_eq!(
        integer_kernel(IntegerOp::Shr, BuiltinType::I32, Some(BuiltinType::U8)).unwrap()(
            1,
            u64::MAX
        ),
        invalid
    );
    assert_eq!(
        conversion_kernel(BuiltinType::I8, BuiltinType::I8).unwrap()(128, 0),
        invalid
    );
}

#[test]
fn prepared_casts_preserve_full_payload_and_float_edges() {
    let mut types = INTEGERS.to_vec();
    types.extend([BuiltinType::F32, BuiltinType::F64]);
    for &source in &types {
        let values = match source {
            BuiltinType::F32 => [f32::NAN, f32::NEG_INFINITY, f32::INFINITY, -0.0, 255.9]
                .map(Number::F32)
                .to_vec(),
            BuiltinType::F64 => [f64::NAN, f64::NEG_INFINITY, f64::INFINITY, -0.0, 255.9]
                .map(Number::F64)
                .to_vec(),
            _ => {
                let (min, max) = source.integer_bounds().unwrap();
                [min, 0, 1, max].map(Number::Integer).to_vec()
            }
        };
        for &target in &types {
            let kernel = conversion_kernel(source, target).unwrap();
            for &value in &values {
                assert_eq!(
                    kernel(bits(value), 0).unwrap(),
                    bits(numeric::cast(value, target.number_type().unwrap())),
                    "{source:?}/{target:?}/{value:?}"
                );
            }
        }
    }
    for payload in [0x8000_0000_0000_0000, 0x7ff0_0000_0000_0001, u64::MAX] {
        assert_eq!(
            conversion_kernel(BuiltinType::F64, BuiltinType::F64).unwrap()(payload, 0).unwrap(),
            payload
        );
    }
    assert_eq!(
        conversion_kernel(BuiltinType::Bool, BuiltinType::U64).unwrap()(1, 0),
        Ok(1)
    );
}

fn bits(value: Number) -> u64 {
    match value {
        Number::Integer(v) => v as u64,
        Number::F32(v) => u64::from(v.to_bits()),
        Number::F64(v) => v.to_bits(),
    }
}
