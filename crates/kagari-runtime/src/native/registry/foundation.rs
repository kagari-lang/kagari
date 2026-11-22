//! Implementation-only entries for private declarations owned by packaged core source.
use crate::native::binding::{Codec, NativeBinding};
use kagari_contract::native_import::foundation::FoundationNative;

pub(super) fn binding(entry: FoundationNative) -> NativeBinding {
    match entry {
        FoundationNative::RangeStart => NativeBinding::new([Codec::Value], Codec::Value, |call| {
            let value = call.argument(0)?;
            let range = call.argument_type_argument(0)?;
            let bound = call.result_type_argument()?;
            call.range_bound(&value, &range, &bound, false)
        }),
    }
}
