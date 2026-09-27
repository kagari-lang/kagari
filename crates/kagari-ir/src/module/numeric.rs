//! Concrete numeric contracts retained through verification and artifact loading.
use super::abi::AbiType;
use kagari_common::integer::IntegerOp;
use kagari_hir::types::BuiltinType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumericOperation {
    pub op: IntegerOp,
    pub input: BuiltinType,
    pub rhs: Option<BuiltinType>,
}

impl NumericOperation {
    pub fn binary(
        op: kagari_hir::hir::BinaryOp,
        input: BuiltinType,
        rhs: BuiltinType,
    ) -> Option<Self> {
        input.integer_layout()?;
        use kagari_hir::hir::BinaryOp;
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
        Some(Self {
            op,
            input,
            rhs: Some(rhs),
        })
    }

    pub fn contract(self) -> Option<(AbiType, Option<AbiType>, AbiType)> {
        self.input.integer_layout()?;
        if self.op == IntegerOp::BitNot {
            if self.rhs.is_some() {
                return None;
            }
        } else {
            let rhs = self.rhs?;
            rhs.integer_layout()?;
            if !matches!(self.op, IntegerOp::Shl | IntegerOp::Shr) && rhs != self.input {
                return None;
            }
        }
        Some((
            AbiType::Builtin(self.input),
            self.rhs.map(AbiType::Builtin),
            AbiType::Builtin(self.input),
        ))
    }
}
