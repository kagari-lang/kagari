//! Compact scalar fields preserve full domains before byte reads and writes.
use crate::value::scalar::domain::admits_payload;
use kagari_abi::representation::ValueType;
use kagari_contract::representation::builtin_representation;
use kagari_types::scalar::BuiltinType;

fn scalar_width(ty: BuiltinType) -> Option<usize> {
    Some(match ty {
        BuiltinType::Unit => 0,
        BuiltinType::Bool => 1,
        BuiltinType::Char | BuiltinType::F32 => 4,
        BuiltinType::F64 => 8,
        _ => (ty.integer_layout()?.0 / 8) as usize,
    })
}

/// Width/sign are derived once from the verified semantic scalar domain.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScalarEncoding {
    pub(crate) ty: BuiltinType,
    pub(crate) representation: ValueType,
    pub(crate) width: usize,
    pub(crate) signed: bool,
}

impl ScalarEncoding {
    pub(crate) fn new(ty: BuiltinType) -> Option<Self> {
        Some(Self {
            ty,
            representation: builtin_representation(ty),
            width: scalar_width(ty)?,
            signed: ty.integer_layout().is_some_and(|(_, signed)| signed),
        })
    }
}

pub(crate) fn read_scalar(bytes: &[u8], offset: usize, encoding: ScalarEncoding) -> Option<u64> {
    let width = encoding.width;
    let bytes = bytes.get(offset..offset.checked_add(width)?)?;
    let bits = match width {
        0 => 0,
        1 => u64::from(bytes[0]),
        2 => u64::from(u16::from_ne_bytes(bytes.try_into().ok()?)),
        4 => u64::from(u32::from_ne_bytes(bytes.try_into().ok()?)),
        8 => u64::from_ne_bytes(bytes.try_into().ok()?),
        _ => return None,
    };
    let bits = if encoding.signed && width < 8 {
        ((bits << (64 - width * 8)) as i64 >> (64 - width * 8)) as u64
    } else {
        bits
    };
    admits_payload(encoding.representation, Some(encoding.ty), bits).then_some(bits)
}

pub(crate) fn write_scalar(
    bytes: &mut [u8],
    offset: usize,
    encoding: ScalarEncoding,
    bits: u64,
) -> Option<()> {
    if !admits_payload(encoding.representation, Some(encoding.ty), bits) {
        return None;
    }
    let width = encoding.width;
    let bytes = bytes.get_mut(offset..offset.checked_add(width)?)?;
    match width {
        0 => {}
        1 => bytes[0] = bits as u8,
        2 => bytes.copy_from_slice(&(bits as u16).to_ne_bytes()),
        4 => bytes.copy_from_slice(&(bits as u32).to_ne_bytes()),
        8 => bytes.copy_from_slice(&bits.to_ne_bytes()),
        _ => return None,
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_types::scalar::BuiltinType;

    #[test]
    fn char_fields_reject_invalid_scalars_before_reading_or_truncating_writes() {
        let encoding = ScalarEncoding::new(BuiltinType::Char).unwrap();
        for bits in [0xd800_u32, 0xdfff, 0x110000, u32::MAX] {
            assert_eq!(read_scalar(&bits.to_ne_bytes(), 0, encoding), None);
        }
        let mut bytes = u32::from('é').to_ne_bytes();
        for bits in [0xd800, 0x110000, 0x1_0000_0061, u64::MAX] {
            assert_eq!(write_scalar(&mut bytes, 0, encoding, bits), None);
            assert_eq!(
                read_scalar(&bytes, 0, encoding),
                Some(u64::from(u32::from('é')))
            );
        }
        write_scalar(&mut bytes, 0, encoding, 0x10ffff).unwrap();
        assert_eq!(read_scalar(&bytes, 0, encoding), Some(0x10ffff));
    }
}
