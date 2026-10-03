//! Concrete numeric contracts retained through verification and artifact loading.

pub mod method;

use crate::{scalar::BuiltinType, standard::surface::StandardEnum, types::Ty};

use kagari_common::integer::IntegerOp;
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

pub fn lossless_from(source: BuiltinType, target: BuiltinType) -> bool {
    if source == target {
        return source.number_type().is_some() || source == BuiltinType::Bool;
    }
    if source == BuiltinType::Bool {
        return target.integer_layout().is_some();
    }
    match target {
        BuiltinType::F32 => matches!(
            source,
            BuiltinType::I8 | BuiltinType::I16 | BuiltinType::U8 | BuiltinType::U16
        ),
        BuiltinType::F64 => matches!(
            source,
            BuiltinType::I8
                | BuiltinType::I16
                | BuiltinType::I32
                | BuiltinType::U8
                | BuiltinType::U16
                | BuiltinType::U32
                | BuiltinType::F32
        ),
        BuiltinType::ISize => {
            matches!(source, BuiltinType::I8 | BuiltinType::I16 | BuiltinType::U8)
        }
        BuiltinType::USize => matches!(source, BuiltinType::U8 | BuiltinType::U16),
        _ if matches!(source, BuiltinType::ISize | BuiltinType::USize) => false,
        _ => match (source.integer_layout(), target.integer_layout()) {
            (Some((a, sa)), Some((b, sb))) => (sa == sb && a < b) || (!sa && sb && a < b),
            _ => false,
        },
    }
}

/// Error type of the portable checked scalar conversion contract.
pub fn conversion_error(source: BuiltinType, target: BuiltinType) -> Option<StandardEnum> {
    if lossless_from(source, target) {
        Some(StandardEnum::Infallible)
    } else if source.integer_layout().is_some() && target.integer_layout().is_some() {
        Some(StandardEnum::TryFromIntError)
    } else {
        None
    }
}

pub fn parsing_error(kind: BuiltinType) -> Option<StandardEnum> {
    (kind.number_type().is_some() || kind == BuiltinType::Bool).then_some(StandardEnum::ParseError)
}
