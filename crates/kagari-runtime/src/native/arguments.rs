//! Small callback argument packs stay on the Rust stack.
use crate::native::{binding::NativeResult, scalar::NativeScalar};
use crate::value::view::ValueView;
use kagari_types::ty::Ty;

mod sealed {
    pub trait Arguments {}
}

pub trait CallArguments: sealed::Arguments {
    fn matches<'ty>(params: impl ExactSizeIterator<Item = &'ty Ty>) -> bool;

    fn with_values<R>(
        self,
        call: impl FnOnce(&[ValueView<'static>]) -> NativeResult<R>,
    ) -> NativeResult<R>;
}

impl CallArguments for () {
    fn matches<'ty>(params: impl ExactSizeIterator<Item = &'ty Ty>) -> bool {
        params.len() == 0
    }

    fn with_values<R>(
        self,
        call: impl FnOnce(&[ValueView<'static>]) -> NativeResult<R>,
    ) -> NativeResult<R> {
        call(&[])
    }
}

impl sealed::Arguments for () {}

macro_rules! arguments {
    ($($ty:ident:$local:ident),+) => {
        impl<$($ty: NativeScalar),+> CallArguments for ($($ty,)+) {
            fn matches<'ty>(params: impl ExactSizeIterator<Item = &'ty Ty>) -> bool { params.eq([$(Ty::Builtin($ty::abi_type())),+].iter()) }

            fn with_values<R>(self, call: impl FnOnce(&[ValueView<'static>]) -> NativeResult<R>) -> NativeResult<R> {
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
