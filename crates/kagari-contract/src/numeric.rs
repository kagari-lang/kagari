//! Concrete numeric contracts retained through verification and artifact loading.

pub mod method;

use kagari_types::{integer::IntegerOp, scalar::BuiltinType, ty::Ty};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumericOperation {
    pub op: IntegerOp,
    pub input: BuiltinType,
    pub rhs: Option<BuiltinType>,
}

impl NumericOperation {
    pub fn contract(self) -> Option<(Ty, Option<Ty>, Ty)> {
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
            Ty::Builtin(self.input),
            self.rhs.map(Ty::Builtin),
            Ty::Builtin(self.input),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumericConversion {
    pub source: BuiltinType,
    pub target: BuiltinType,
}

impl NumericConversion {
    pub fn contract(self) -> Option<(Ty, Ty)> {
        self.source
            .can_cast_to(self.target)
            .then_some((Ty::Builtin(self.source), Ty::Builtin(self.target)))
    }
}
