//! Zero-copy access to the finite scalar representations owned by Vec.
use crate::collections::vector::{VecStorage, buffer::SequenceStorage};
use kagari_runtime::{
    error::RuntimeError,
    native::{binding::NativeResult, context::CallContext, scalar::NativeScalar},
};
use std::any::Any;

mod sealed {
    pub trait Element {}
}

/// A scalar with a contiguous Vec representation. Rust usize/isize conveniences
/// are excluded because script u64/i64 retain their fixed width on every target.
pub trait NativeElement: NativeScalar + sealed::Element {
    #[doc(hidden)]
    fn slice(storage: &dyn Any) -> Option<&[Self]>;

    #[doc(hidden)]
    fn slice_mut(storage: &mut dyn Any) -> Option<&mut [Self]>;
}

/// Borrow a native-call Vec argument's scalar storage without creating an owning
/// handle. The call's argument roots protect the source; the runtime rejects
/// collection, script reentry and conflicting storage access during the callback.
/// A different payload or scalar representation returns a validation error.
pub fn with_argument_slice<E: NativeElement, R>(
    cx: &CallContext<'_>,
    argument: usize,
    read: impl FnOnce(&[E]) -> NativeResult<R>,
) -> NativeResult<R> {
    // SAFETY: E contains no script edges. The nonescaping shared slice cannot
    // mutate storage, and the argument remains rooted for the complete loan.
    unsafe {
        cx.with_payload::<VecStorage, _>(argument, |values| {
            let slice = E::slice(&values.values).ok_or_else(|| {
                RuntimeError::module_validation("Vec scalar view differs from its element type")
            })?;
            read(slice)
        })
    }
}

macro_rules! elements {
    ($($variant:ident:$rust:ty),+) => { $(
        impl sealed::Element for $rust {}

        impl NativeElement for $rust {
            fn slice(storage: &dyn Any) -> Option<&[Self]> {
                match storage.downcast_ref::<SequenceStorage>()? {
                    SequenceStorage::$variant(values) => Some(values),
                    _ => None,
                }
            }

            fn slice_mut(storage: &mut dyn Any) -> Option<&mut [Self]> {
                match storage.downcast_mut::<SequenceStorage>()? {
                    SequenceStorage::$variant(values) => Some(values),
                    _ => None,
                }
            }
        }
    )+ };
}

elements!(Unit:(), Bool:bool, Char:char, I8:i8, I16:i16, I32:i32, I64:i64,
    U8:u8, U16:u16, U32:u32, U64:u64, F32:f32, F64:f64);
