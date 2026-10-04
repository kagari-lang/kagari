//! Concrete numeric contracts retained through verification and artifact loading.
use kagari_types::conversion::conversion_error;

pub mod method;

use kagari_types::{integer::IntegerOp, scalar::BuiltinType, surface::StandardEnum, ty::Ty};
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
    pub checked: bool,
    pub source: BuiltinType,
    pub target: BuiltinType,
}

impl NumericConversion {
    pub fn contract(self) -> Option<(Ty, Ty)> {
        if self.checked {
            let error = conversion_error(self.source, self.target)?;
            return Some((
                Ty::Builtin(self.source),
                Ty::StandardEnum {
                    kind: StandardEnum::Result,
                    args: vec![
                        Ty::Builtin(self.target),
                        Ty::StandardEnum {
                            kind: error,
                            args: vec![],
                        },
                    ],
                },
            ));
        }
        self.source
            .can_cast_to(self.target)
            .then_some((Ty::Builtin(self.source), Ty::Builtin(self.target)))
    }
}
