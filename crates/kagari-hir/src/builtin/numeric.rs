//! Built-in conversions follow Rust's portable From matrix.
use crate::types::TypeId;
use kagari_abi::numeric;
use kagari_abi::scalar::BuiltinType;

pub fn try_error(source: BuiltinType, target: BuiltinType) -> Option<TypeId> {
    let kind = numeric::conversion_error(source, target)?;
    Some(TypeId::StandardEnum { kind, args: vec![] })
}
