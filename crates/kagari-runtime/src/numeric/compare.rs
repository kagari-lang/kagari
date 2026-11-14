//! Scalar comparison has no heap access or externally supplied behavior.
use crate::{error::RuntimeError, value::view::ValueView};
use kagari_abi::representation::ValueType;
use kagari_bytecode::instruction::BinaryOp;

pub(super) fn compare<'call>(
    op: BinaryOp,
    lhs: ValueView<'call>,
    rhs: ValueView<'call>,
) -> Result<ValueView<'call>, RuntimeError> {
    macro_rules! compare {
        ($a:expr, $b:expr) => {
            match op {
                BinaryOp::Eq => $a == $b,
                BinaryOp::NotEq => $a != $b,
                BinaryOp::Lt => $a < $b,
                BinaryOp::Le => $a <= $b,
                BinaryOp::Gt => $a > $b,
                BinaryOp::Ge => $a >= $b,
                _ => return Err(RuntimeError::module_validation("scalar comparison opcode")),
            }
        };
    }
    let equality = matches!(op, BinaryOp::Eq | BinaryOp::NotEq);
    let result = match (lhs.scalar_payload(), rhs.scalar_payload()) {
        (Some((ValueType::Unit, _)), Some((ValueType::Unit, _))) if equality => op == BinaryOp::Eq,
        (Some((ValueType::Bool, a)), Some((ValueType::Bool, b))) if equality => {
            (a == b) == (op == BinaryOp::Eq)
        }
        (Some((ValueType::Char, a)), Some((ValueType::Char, b))) => compare!(a, b),
        (Some((ValueType::I32 | ValueType::I64, a)), Some((kind, b)))
            if lhs.has_representation(kind) =>
        {
            compare!(a as i64, b as i64)
        }
        (Some((ValueType::U64, a)), Some((ValueType::U64, b))) => compare!(a, b),
        (Some((ValueType::F32, a)), Some((ValueType::F32, b))) => {
            compare!(f32::from_bits(a as u32), f32::from_bits(b as u32))
        }
        (Some((ValueType::F64, a)), Some((ValueType::F64, b))) => {
            compare!(f64::from_bits(a), f64::from_bits(b))
        }
        _ => {
            return Err(RuntimeError::module_validation(
                "scalar comparison operands",
            ));
        }
    };
    Ok(ValueView::from(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::scalar::NativeScalar;
    use crate::numeric::{scalar_binary, unary};
    use kagari_bytecode::instruction::UnaryOp;

    #[test]
    fn char_comparison_does_not_enable_numeric_operations() {
        let a = ValueView::from('é');
        let b = ValueView::from('🙂');
        for (op, expected) in [
            (BinaryOp::Eq, false),
            (BinaryOp::NotEq, true),
            (BinaryOp::Lt, true),
            (BinaryOp::Le, true),
            (BinaryOp::Gt, false),
            (BinaryOp::Ge, false),
        ] {
            assert_eq!(scalar_binary(op, a, b).unwrap().as_bool(), Some(expected));
            assert!(scalar_binary(op, a, i64::encode(233)).is_err());
        }
        assert!(scalar_binary(BinaryOp::Add, a, b).is_err());
        assert!(unary(UnaryOp::Neg, a).is_err());
        assert!(unary(UnaryOp::Not, a).is_err());
    }
}
