//! Closed integer-method contracts shared by verification and native execution.

use kagari_types::integer::IntegerMethod;
use kagari_types::{language::binding, scalar::BuiltinType, ty::Ty};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegerMethodContract {
    receiver: BuiltinType,
    rhs: BuiltinType,
    method: IntegerMethod,
}

impl IntegerMethodContract {
    pub fn new(method: IntegerMethod, receiver: BuiltinType) -> Option<Self> {
        receiver.integer_layout()?;
        let rhs = match method {
            IntegerMethod::RotateLeft | IntegerMethod::RotateRight => BuiltinType::U32,
            IntegerMethod::WrappingAddSigned => match receiver {
                BuiltinType::U8 => BuiltinType::I8,
                BuiltinType::U16 => BuiltinType::I16,
                BuiltinType::U32 => BuiltinType::I32,
                BuiltinType::U64 => BuiltinType::I64,
                BuiltinType::USize => BuiltinType::ISize,
                _ => return None,
            },
            IntegerMethod::WrappingAdd
            | IntegerMethod::WrappingSub
            | IntegerMethod::WrappingMul
            | IntegerMethod::CheckedAdd
            | IntegerMethod::CheckedSub
            | IntegerMethod::CheckedMul
            | IntegerMethod::CheckedDiv
            | IntegerMethod::CheckedRem
            | IntegerMethod::OverflowingAdd
            | IntegerMethod::OverflowingSub
            | IntegerMethod::OverflowingMul
            | IntegerMethod::SaturatingAdd
            | IntegerMethod::SaturatingSub
            | IntegerMethod::SaturatingMul => receiver,
        };
        Some(Self {
            receiver,
            rhs,
            method,
        })
    }

    pub fn parameters(self) -> [BuiltinType; 2] {
        [self.receiver, self.rhs]
    }

    pub fn rhs(self) -> BuiltinType {
        self.rhs
    }

    pub fn result(self) -> Ty {
        let value = Ty::Builtin(self.receiver);
        if self.method.checked() {
            binding::option(value)
        } else if self.method.overflowing() {
            Ty::Tuple(vec![value, Ty::Builtin(BuiltinType::Bool)])
        } else {
            value
        }
    }
}
