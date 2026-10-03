//! Small callback argument packs stay on the Rust stack.
use crate::native::{binding::NativeResult, scalar::NativeScalar};
use crate::value::Value;
use kagari_abi::types::AbiType;

mod sealed {
    pub trait Arguments {}
}

pub trait CallArguments: sealed::Arguments {
    fn matches(params: &[AbiType]) -> bool;

    fn with_values<R>(self, call: impl FnOnce(&[Value]) -> NativeResult<R>) -> NativeResult<R>;
}

impl CallArguments for () {
    fn matches(params: &[AbiType]) -> bool {
        params.is_empty()
    }

    fn with_values<R>(self, call: impl FnOnce(&[Value]) -> NativeResult<R>) -> NativeResult<R> {
        call(&[])
    }
}

impl sealed::Arguments for () {}

macro_rules! arguments {
    ($($ty:ident:$local:ident),+) => {
        impl<$($ty: NativeScalar),+> CallArguments for ($($ty,)+) {
            fn matches(params: &[AbiType]) -> bool { params == [$($ty::abi_type()),+] }

            fn with_values<R>(self, call: impl FnOnce(&[Value]) -> NativeResult<R>) -> NativeResult<R> {
                let ($($local,)+) = self;
                let values = [$($local.encode()),+];
                call(&values)
            }
        }

        impl<$($ty: NativeScalar),+> sealed::Arguments for ($($ty,)+) {}
    };
}

arguments!(A:a);
arguments!(A:a, B:b);
arguments!(A:a, B:b, C:c);
