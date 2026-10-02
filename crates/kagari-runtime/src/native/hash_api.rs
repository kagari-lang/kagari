//! Hash declarations and scalar implementations owned by native registration.
macro_rules! hash_module {
    ($($scalar:ty),*) => {
        #[kagari_native_macros::native_module("std::hash", runtime = crate)]
        pub mod hash {
            use std::cmp::Ordering;
            use crate::native_value::{NativeCall, NativeResult, scalar_protocol::hash_scalar};
            /// A script hash consistent with equality, not a persistent identifier.
            #[native_trait]
            pub trait Hash {
                /// Return the runtime key hash of this value.
                fn hash(&self, #[context] call: &NativeCall) -> NativeResult<i64>;
            }
            $(
                #[native_impl]
                impl Hash for $scalar {
                    fn hash(&self, #[context] call: &NativeCall) -> NativeResult<i64> {
                        hash_scalar(call, self)
                    }
                }
            )*
        }
    };
}
hash_module!(
    i8,
    i16,
    i32,
    i64,
    isize,
    u8,
    u16,
    u32,
    u64,
    usize,
    bool,
    String,
    (),
    Ordering
);
