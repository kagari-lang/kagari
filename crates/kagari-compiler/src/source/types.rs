//! Encode checked source types at the compiler boundary.
use kagari_common::identity::table::{DefinitionId, DefinitionTable};
use kagari_contract::numeric::NumericOperation;
use kagari_hir::hir::expr::ops::BinaryOp;
use kagari_types::{
    declaration::{
        TypeDefKind,
        native::{NativeStorageLayout, NativeTypeConstructor, language_type},
    },
    integer::IntegerOp,
    language::binding,
    scalar::BuiltinType,
};

/// The compiler owns executable representation; HIR carries no storage classification.
pub(crate) fn native_representation(
    id: DefinitionId,
    definitions: &DefinitionTable,
) -> TypeDefKind {
    if let Some(constructor) = NativeTypeConstructor::from_definition(id, definitions) {
        return TypeDefKind::Native(constructor);
    }
    let layout = if binding::matches(&id, &binding::future_declaration(), definitions) {
        NativeStorageLayout::Future
    } else if language_type(id, definitions, "kagari-core", "task", "Task") {
        NativeStorageLayout::Task
    } else if language_type(id, definitions, "kagari-core", "task", "TaskScope") {
        NativeStorageLayout::TaskScope
    } else {
        NativeStorageLayout::Opaque
    };
    TypeDefKind::NativeStorage(layout)
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
