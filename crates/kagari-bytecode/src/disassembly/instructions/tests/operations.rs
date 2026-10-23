use crate::{
    disassembly::instructions::tests::{Fixture, r, t},
    instruction::{BinaryOp, BytecodeInstruction, UnaryOp},
};
use kagari_contract::{
    numeric::{NumericConversion, NumericOperation},
    operations::{IterOp, StringIterKind},
};
use kagari_types::{integer::IntegerOp, scalar::BuiltinType};

#[test]
fn unary_binary_and_numeric_operations_remain_distinct_and_typed() {
    let fixture = Fixture::new();
    for (op, name) in [(UnaryOp::Neg, "neg"), (UnaryOp::Not, "not")] {
        assert_eq!(
            fixture.render(&BytecodeInstruction::Unary {
                dst: r(0),
                op,
                operand: r(1)
            }),
            format!("unary.{name} s0, s1")
        );
    }
    for (op, name) in [
        (BinaryOp::Add, "add"),
        (BinaryOp::Sub, "sub"),
        (BinaryOp::Mul, "mul"),
        (BinaryOp::Div, "div"),
        (BinaryOp::Rem, "rem"),
        (BinaryOp::Eq, "eq"),
        (BinaryOp::NotEq, "not_eq"),
        (BinaryOp::IdentityEq, "identity_eq"),
        (BinaryOp::IdentityNotEq, "identity_not_eq"),
        (BinaryOp::Lt, "lt"),
        (BinaryOp::Gt, "gt"),
        (BinaryOp::Le, "le"),
        (BinaryOp::Ge, "ge"),
    ] {
        assert_eq!(
            fixture.render(&BytecodeInstruction::Binary {
                dst: r(0),
                op,
                lhs: r(1),
                rhs: r(2)
            }),
            format!("binary.{name} dst=s0, lhs=s1, rhs=s2")
        );
    }
    for (op, name) in [
        (IntegerOp::BitAnd, "bit_and"),
        (IntegerOp::BitOr, "bit_or"),
        (IntegerOp::BitXor, "bit_xor"),
        (IntegerOp::Shl, "shl"),
        (IntegerOp::Shr, "shr"),
        (IntegerOp::CheckedAdd, "checked_add"),
        (IntegerOp::CheckedSub, "checked_sub"),
        (IntegerOp::CheckedMul, "checked_mul"),
        (IntegerOp::CheckedDiv, "checked_div"),
        (IntegerOp::CheckedRem, "checked_rem"),
    ] {
        let operation = NumericOperation {
            op,
            input: BuiltinType::I16,
            rhs: Some(BuiltinType::U8),
        };
        assert_eq!(
            fixture.render(&BytecodeInstruction::Numeric {
                dst: r(0),
                operation,
                lhs: r(1),
                rhs: Some(r(2))
            }),
            format!("numeric.{name}(input=i16, rhs=u8) dst=s0, lhs=s1, rhs=s2")
        );
        assert_eq!(
            fixture.render(&BytecodeInstruction::Binary {
                dst: r(0),
                op: BinaryOp::Numeric(operation),
                lhs: r(1),
                rhs: r(2)
            }),
            format!("binary.numeric.{name}(input=i16, rhs=u8) dst=s0, lhs=s1, rhs=s2")
        );
    }
    let operation = NumericOperation {
        op: IntegerOp::BitNot,
        input: BuiltinType::U64,
        rhs: None,
    };
    assert_eq!(
        fixture.render(&BytecodeInstruction::Numeric {
            dst: r(0),
            operation,
            lhs: r(1),
            rhs: None
        }),
        "numeric.bit_not(input=u64, rhs=none) dst=s0, lhs=s1, rhs=none"
    );
    assert_eq!(
        fixture.render(&BytecodeInstruction::Convert {
            dst: r(0),
            src: r(1),
            conversion: NumericConversion {
                source: BuiltinType::F64,
                target: BuiltinType::I8
            }
        }),
        "convert s0, src=s1, source=f64, target=i8"
    );
}

#[test]
fn every_iterator_mode_and_optional_input_is_explicit() {
    let fixture = Fixture::new();
    for (op, name) in [
        (IterOp::New, "new"),
        (IterOp::Next, "next"),
        (IterOp::String(StringIterKind::Chars), "string.chars"),
        (IterOp::String(StringIterKind::Bytes), "string.bytes"),
        (
            IterOp::String(StringIterKind::CharIndices),
            "string.char_indices",
        ),
        (IterOp::String(StringIterKind::Split), "string.split"),
        (IterOp::String(StringIterKind::SplitN), "string.split_n"),
        (
            IterOp::String(StringIterKind::Whitespace),
            "string.whitespace",
        ),
        (IterOp::String(StringIterKind::Lines), "string.lines"),
    ] {
        assert_eq!(
            fixture.render(&BytecodeInstruction::Iter {
                dst: r(0),
                value: Some(r(1)),
                ty: t(),
                op
            }),
            format!("iter.{name} s0, value=s1, type=i32")
        );
    }
    assert_eq!(
        fixture.render(&BytecodeInstruction::Iter {
            dst: r(0),
            value: None,
            ty: t(),
            op: IterOp::New
        }),
        "iter.new s0, value=none, type=i32"
    );
}
