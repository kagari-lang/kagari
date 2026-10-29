//! Packed codecs preserve scalar domains and direct pointer provenance.
use crate::{
    gc::reference::GcRef,
    module::layout_storage::{ScalarEncoding, StoredField},
    value::{InterfaceObjectId, Value, scalar},
};

/// Bytes contain initialized, traced runtime fields, including pointer provenance.
pub(super) unsafe fn read_field(bytes: &[u8], field: StoredField) -> Option<Value> {
    match field {
        StoredField::Scalar { offset, encoding } => scalar::decode(
            encoding.representation,
            read_scalar(bytes, offset, encoding)?,
        ),
        StoredField::Struct { offset } => {
            let bytes = bytes.get(offset..offset + size_of::<GcRef>())?;
            // SAFETY: the exact declared struct field contains an initialized
            // pointer carrier. Unaligned typed loads preserve its provenance.
            Some(Value::Struct(unsafe {
                bytes.as_ptr().cast::<GcRef>().read_unaligned()
            }))
        }
        StoredField::Managed { offset } => {
            // SAFETY: the enclosing field belongs to initialized traced storage.
            unsafe { read_managed(bytes, offset) }
        }
    }
}

pub(super) fn write_field(bytes: &mut [u8], field: StoredField, value: Value) -> Option<()> {
    match field {
        StoredField::Scalar { offset, encoding } => {
            if !value.has_representation(encoding.representation) {
                return None;
            }
            write_scalar(bytes, offset, encoding, scalar::encode(&value)?)
        }
        StoredField::Struct { offset } => {
            let Value::Struct(reference) = value else {
                return None;
            };
            let bytes = bytes.get_mut(offset..offset + size_of::<GcRef>())?;
            // SAFETY: exclusive bounded destination for a nonowning Copy carrier.
            unsafe {
                bytes
                    .as_mut_ptr()
                    .cast::<GcRef>()
                    .write_unaligned(reference)
            };
            Some(())
        }
        StoredField::Managed { offset } => write_managed(bytes, offset, value),
    }
}

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

/// All managed fields retain a tag and the original allocation provenance.
unsafe fn read_managed(bytes: &[u8], offset: usize) -> Option<Value> {
    let bytes = bytes.get(offset..offset + 1 + size_of::<GcRef>())?;
    let construct = match bytes[0] {
        1 => Value::Str,
        2 => Value::Tuple,
        3 => Value::Range,
        4 => Value::Array,
        9 => Value::GcHandle,
        7 => Value::Enum,
        10 => Value::Closure,
        11 => Value::Cell,
        12 => |reference| Value::Interface(InterfaceObjectId(reference)),
        _ => return None,
    };
    // SAFETY: the validated direct tag names an initialized, traced pointer carrier.
    // A typed unaligned load retains the allocation provenance, unlike integer bits.
    Some(construct(unsafe {
        bytes.as_ptr().add(1).cast::<GcRef>().read_unaligned()
    }))
}

pub(super) fn write_managed(bytes: &mut [u8], offset: usize, value: Value) -> Option<()> {
    let tag = match value {
        Value::Str(_) => 1,
        Value::Tuple(_) => 2,
        Value::Range(_) => 3,
        Value::Array(_) => 4,
        Value::Enum(_) => 7,
        Value::GcHandle(_) => 9,
        Value::Closure(_) => 10,
        Value::Cell(_) => 11,
        Value::Interface(_) => 12,
        _ => return None,
    };
    let bytes = bytes.get_mut(offset..offset + 1 + size_of::<GcRef>())?;
    bytes.fill(0);
    bytes[0] = tag;
    let reference = value.direct_reference()?;
    // SAFETY: the bounded destination holds one nonowning pointer carrier.
    unsafe {
        bytes
            .as_mut_ptr()
            .add(1)
            .cast::<GcRef>()
            .write_unaligned(reference)
    };
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Runtime, native::storage::NativeStorage, test_support::layouts::allocation_owner};
    use kagari_abi::representation::ValueType;
    use kagari_types::scalar::BuiltinType;
    use kagari_types::ty::Ty;

    #[test]
    fn packed_native_reference_keeps_provenance_through_take_and_collection() {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let heap = runtime.gc();
        let native = NativeStorage::takeable::<i32>()
            .prepare_payload(heap, &Ty::Builtin(BuiltinType::I32), 42, &owner)
            .unwrap();
        let reference = heap.alloc_native(native).unwrap();
        let root = heap.root_value(Value::GcHandle(reference)).unwrap();
        let mut bytes = [0_u8; 2 + size_of::<GcRef>()];
        let field = StoredField::Managed { offset: 1 };
        write_field(&mut bytes, field, Value::GcHandle(reference)).unwrap();
        assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
        // SAFETY: the exact tagged field was initialized above and the root keeps
        // its original pointer alive, including after extraction changes dispatch.
        let copied = unsafe { read_field(&bytes, field) }.unwrap();
        assert_eq!(copied, Value::GcHandle(reference));
        assert_eq!(
            heap.with_native::<i32, _>(reference, |value| Ok(*value))
                .unwrap(),
            42
        );
        assert_eq!(heap.take_native::<i32>(reference).unwrap(), 42);
        assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
        assert_eq!(unsafe { read_field(&bytes, field) }, Some(copied));
        assert!(heap.root_value(copied).is_none());
        drop(root);
        assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
        assert_eq!(heap.stats().current_retained_bytes, 0);
    }

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
