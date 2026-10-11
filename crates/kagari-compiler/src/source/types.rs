//! Encode checked source types at the compiler boundary.
use kagari_contract::numeric::NumericOperation;
use kagari_hir::{hir::expr::ops::BinaryOp, native::NativeTypeKind};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeTypeConstructor},
    integer::IntegerOp,
    scalar::BuiltinType,
};

pub(crate) fn lower_native_kind(kind: NativeTypeKind) -> TypeDefKind {
    match kind {
        NativeTypeKind::Storage { layout, .. } => TypeDefKind::NativeStorage(layout),
        NativeTypeKind::String => TypeDefKind::Native(NativeTypeConstructor::String),
        NativeTypeKind::HashMap => TypeDefKind::Native(NativeTypeConstructor::Map),
        NativeTypeKind::HashSet => TypeDefKind::Native(NativeTypeConstructor::Set),
        NativeTypeKind::Iter => TypeDefKind::Native(NativeTypeConstructor::Iter),
        NativeTypeKind::Range(kind) => TypeDefKind::Native(NativeTypeConstructor::Range(kind)),
    }
}

pub(crate) fn lower_numeric_operation(
    op: BinaryOp,
    input: BuiltinType,
    rhs: BuiltinType,
) -> Option<NumericOperation> {
    input.integer_layout()?;
    let op = match op {
        BinaryOp::Add => IntegerOp::CheckedAdd,
        BinaryOp::Sub => IntegerOp::CheckedSub,
        BinaryOp::Mul => IntegerOp::CheckedMul,
        BinaryOp::Div => IntegerOp::CheckedDiv,
        BinaryOp::Rem => IntegerOp::CheckedRem,
        BinaryOp::BitAnd => IntegerOp::BitAnd,
        BinaryOp::BitOr => IntegerOp::BitOr,
        BinaryOp::BitXor => IntegerOp::BitXor,
        BinaryOp::Shl => IntegerOp::Shl,
        BinaryOp::Shr => IntegerOp::Shr,
        _ => return None,
    };
    Some(NumericOperation {
        op,
        input,
        rhs: Some(rhs),
    })
}
