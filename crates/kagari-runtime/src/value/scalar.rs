//! Cold scalar admission/materialization. Hot kernels consume complete payloads.
pub(crate) mod domain;
pub(crate) mod elements;
pub(crate) mod fields;
#[cfg(test)]
use crate::value::scalar::domain::admits_payload;
use crate::value::{
    carriers::{Representation, ScopedValue},
    scalar::domain::decode_char,
    view::ValueView,
};
use kagari_abi::representation::ValueType;
use kagari_contract::representation::builtin_representation;
use kagari_types::scalar::{BuiltinType, error::ScalarError};

/// Admit one exact script integer domain without widening managed transport.
/// Signed carriers preserve two's-complement bits; unsigned u64 never narrows
/// through i64. All invalid carriers/domains retain InvalidOperandDomain.
pub(crate) fn read_integer(ty: BuiltinType, value: ScopedValue<'_>) -> Result<i128, ScalarError> {
    read_integer_payload(ty, value.scalar_payload())
}

/// Apply the same domain admission to an inline scalar call view.
pub(crate) fn read_integer_view(
    ty: BuiltinType,
    value: ValueView<'_>,
) -> Result<i128, ScalarError> {
    read_integer_payload(ty, value.scalar_payload())
}

/// Match an edge-free payload against its exact builtin carrier and domain.
/// The same predicate serves scoped values and inline call completions.
pub(crate) fn matches_builtin_payload(ty: BuiltinType, payload: (ValueType, u64)) -> bool {
    if ty.integer_layout().is_some() {
        read_integer_payload(ty, Some(payload)).is_ok()
    } else {
        payload.0 == builtin_representation(ty)
            && domain::admits_payload(payload.0, Some(ty), payload.1)
    }
}

fn read_integer_payload(
    ty: BuiltinType,
    payload: Option<(ValueType, u64)>,
) -> Result<i128, ScalarError> {
    let invalid = ScalarError::InvalidOperandDomain;
    let (representation, bits) = payload.ok_or(invalid)?;
    if ty.integer_layout().is_none()
        || representation != builtin_representation(ty)
        || !domain::admits_payload(representation, Some(ty), bits)
    {
        return Err(invalid);
    }
    Ok(match representation {
        ValueType::I32 | ValueType::I64 => i128::from(bits as i64),
        ValueType::U64 => i128::from(bits),
        _ => return Err(invalid),
    })
}

pub(crate) fn encode(value: &ScopedValue<'_>) -> Option<u64> {
    value.scalar_payload().map(|(_, bits)| bits)
}

pub(crate) fn decode<'gc>(ty: ValueType, bits: u64) -> Option<ScopedValue<'gc>> {
    Some(ScopedValue::from_representation(match ty {
        ValueType::Unit => Representation::Unit,
        ValueType::Bool if bits <= 1 => Representation::Bool(bits != 0),
        ValueType::Char => Representation::Char(decode_char(bits)?),
        ValueType::I32 => Representation::I32(bits as i32),
        ValueType::I64 => Representation::I64(bits as i64),
        ValueType::U64 => Representation::U64(bits),
        ValueType::F32 => Representation::F32(f32::from_bits(bits as u32)),
        ValueType::F64 => Representation::F64(f64::from_bits(bits)),
        _ => return None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_payloads_require_canonical_unicode_scalars_and_exact_domains() {
        for ch in ['\0', 'a', 'é', '\u{d7ff}', '\u{e000}', '🙂', char::MAX] {
            let value = ScopedValue::from(ch);
            let bits = encode(&value).unwrap();
            assert_eq!(
                decode(ValueType::Char, bits).unwrap().scalar_payload(),
                value.scalar_payload()
            );
            assert!(admits_payload(
                ValueType::Char,
                Some(BuiltinType::Char),
                bits
            ));
            assert!(admits_payload(ValueType::Char, None, bits));
            assert!(!admits_payload(
                ValueType::I64,
                Some(BuiltinType::Char),
                bits
            ));
            assert!(!admits_payload(
                ValueType::Char,
                Some(BuiltinType::U32),
                bits
            ));
            assert!(!value.has_representation(ValueType::I64));
            assert!(!ScopedValue::from(bits as i64).has_representation(ValueType::Char));
        }
        for bits in [0xd800, 0xdfff, 0x110000, 0x1_0000_0061, u64::MAX] {
            assert!(decode(ValueType::Char, bits).is_none());
            assert!(!admits_payload(
                ValueType::Char,
                Some(BuiltinType::Char),
                bits
            ));
            assert!(!admits_payload(ValueType::Char, None, bits));
        }
    }
}
