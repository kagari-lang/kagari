//! Opaque return packets keep concrete script values in their prepared representation.
use crate::{frame::values::scalar, value::Value};
use kagari_abi::representation::ValueType;

pub struct ReturnValue(Contents);

enum Contents {
    General(Value),
    Scalar {
        representation: ValueType,
        bits: u64,
    },
}

impl ReturnValue {
    pub fn general(value: Value) -> Self {
        Self(Contents::General(value))
    }

    pub(crate) fn scalar(representation: ValueType, bits: u64) -> Self {
        Self(Contents::Scalar {
            representation,
            bits,
        })
    }

    pub(crate) fn payload(&self) -> Option<(ValueType, u64)> {
        match self.0 {
            Contents::Scalar {
                representation,
                bits,
            } => Some((representation, bits)),
            Contents::General(_) => None,
        }
    }

    pub(crate) fn materialize(self) -> Option<Value> {
        match self.0 {
            Contents::General(value) => Some(value),
            Contents::Scalar {
                representation,
                bits,
            } => scalar::decode(representation, bits),
        }
    }
}
