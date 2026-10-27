//! Safe byte codecs share scalar domains and preserve complete managed identities.
use crate::{
    gc::HeapObjectId,
    module::layout_storage::ScalarEncoding,
    value::{InterfaceObjectId, Value, scalar},
};

pub(super) fn read_scalar(bytes: &[u8], offset: usize, encoding: ScalarEncoding) -> Option<u64> {
    let width = encoding.width;
    let bytes = bytes.get(offset..offset + width)?;
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
    scalar::admits_payload(encoding.representation, Some(encoding.ty), bits).then_some(bits)
}

pub(super) fn write_scalar(
    bytes: &mut [u8],
    offset: usize,
    encoding: ScalarEncoding,
    bits: u64,
) -> Option<()> {
    if !scalar::admits_payload(encoding.representation, Some(encoding.ty), bits) {
        return None;
    }
    let width = encoding.width;
    let bytes = bytes.get_mut(offset..offset + width)?;
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

pub(super) fn read_managed(bytes: &[u8], offset: usize) -> Option<Value> {
    let bytes = bytes.get(offset..offset + 13)?;
    let id = HeapObjectId {
        owner: u32::from_ne_bytes(bytes[1..5].try_into().ok()?),
        slot: u32::from_ne_bytes(bytes[5..9].try_into().ok()?),
        generation: u32::from_ne_bytes(bytes[9..13].try_into().ok()?),
    };
    Some(match bytes[0] {
        1 => Value::Str(id),
        2 => Value::Tuple(id),
        3 => Value::Range(id),
        4 => Value::Array(id),
        7 => Value::Enum(id),
        8 => Value::Struct(id),
        9 => Value::GcHandle(id),
        10 => Value::Closure(id),
        11 => Value::Cell(id),
        12 => Value::Interface(InterfaceObjectId(id)),
        _ => return None,
    })
}

pub(super) fn write_managed(bytes: &mut [u8], offset: usize, value: Value) -> Option<()> {
    let (tag, id) = match value {
        Value::Str(id) => (1, id),
        Value::Tuple(id) => (2, id),
        Value::Range(id) => (3, id),
        Value::Array(id) => (4, id),

        Value::Enum(id) => (7, id),
        Value::Struct(id) => (8, id),
        Value::GcHandle(id) => (9, id),
        Value::Closure(id) => (10, id),
        Value::Cell(id) => (11, id),
        Value::Interface(InterfaceObjectId(id)) => (12, id),
        _ => return None,
    };
    let bytes = bytes.get_mut(offset..offset + 13)?;
    bytes[0] = tag;
    bytes[1..5].copy_from_slice(&id.owner.to_ne_bytes());
    bytes[5..9].copy_from_slice(&id.slot.to_ne_bytes());
    bytes[9..13].copy_from_slice(&id.generation.to_ne_bytes());
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_abi::representation::ValueType;
    use kagari_types::scalar::BuiltinType;

    #[test]
    fn char_fields_reject_invalid_scalars_before_reading_or_truncating_writes() {
        let encoding = ScalarEncoding {
            ty: BuiltinType::Char,
            representation: ValueType::Char,
            width: 4,
            signed: false,
        };
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
