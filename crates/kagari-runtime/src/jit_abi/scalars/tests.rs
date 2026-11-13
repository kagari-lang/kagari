use crate::jit_abi::scalars::decode_native_value;
use kagari_abi::{
    native_call::{JIT_VALUE_TAG_I32, JitValue},
    representation::ValueType,
};
use kagari_gc::{error::GcError, heap::Heap};

#[test]
fn native_scalar_results_preserve_domains_without_host_leases() -> Result<(), GcError> {
    let mut heap = Heap::default();
    for (encoded, representation, bits) in [
        (JitValue::unit(), ValueType::Unit, 0),
        (JitValue::bool(false), ValueType::Bool, 0),
        // The C ABI treats any nonzero Bool payload as true. The resulting
        // Runtime carrier is canonical, independently of that external spelling.
        (
            JitValue {
                payload: -7,
                ..JitValue::bool(true)
            },
            ValueType::Bool,
            1,
        ),
        (
            JitValue::i32(i32::MIN),
            ValueType::I32,
            i64::from(i32::MIN) as u64,
        ),
        (JitValue::i32(i32::MAX), ValueType::I32, i32::MAX as u64),
    ] {
        let (actual, value) = decode_native_value(encoded).unwrap();
        assert_eq!(actual, representation);
        heap.scope(|cx| {
            assert_eq!(
                value.get(cx)?.scalar_payload(),
                Some((representation, bits))
            );
            Ok::<_, GcError>(())
        })?;
    }
    for encoded in [
        JitValue {
            tag: u8::MAX,
            ..JitValue::unit()
        },
        JitValue {
            tag: JIT_VALUE_TAG_I32,
            payload: i64::from(i32::MIN) - 1,
            ..JitValue::unit()
        },
        JitValue {
            tag: JIT_VALUE_TAG_I32,
            payload: i64::from(i32::MAX) + 1,
            ..JitValue::unit()
        },
    ] {
        assert!(
            decode_native_value(encoded).is_none(),
            "invalid ABI domain: {encoded:?}"
        );
    }
    assert_eq!(heap.stats().root_registrations, 0);
    assert_eq!(heap.stats().objects, 0);
    Ok(())
}
