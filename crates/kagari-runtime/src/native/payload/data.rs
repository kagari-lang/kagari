//! Fixed-size Rust data that cannot retain script or executable graph edges.
use crate::{native::storage::NativePayload, value::Value};
use std::{fmt::Debug, mem::size_of};

/// Data supported by direct native editing. Prefer `native_data!` for structs.
/// Owned strings/containers and script handles require controlled setters instead.
///
/// # Safety
/// Every field, recursively, must be NativeData. Implementations must contain no
/// runtime handles, roots, script values, callable/metadata identities, references,
/// pointers or interior mutable state. Copying and arbitrary mutation must preserve
/// the absence of graph edges and a fixed allocation size. Manual implementations
/// are a trusted storage boundary; an empty trace function is not sufficient proof.
pub unsafe trait NativeData: Copy + Debug + Send + 'static {}

macro_rules! scalar_data {
    ($($ty:ty),* $(,)?) => {$(
        // SAFETY: These scalar types have no references or indirect storage.
        unsafe impl NativeData for $ty {}
    )*};
}
scalar_data!(
    (),
    bool,
    char,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    f32,
    f64
);

// SAFETY: All elements satisfy the recursive fixed-data contract.
unsafe impl<T: NativeData, const N: usize> NativeData for [T; N] {}

macro_rules! tuple_data {
    ($($name:ident),+) => {
        // SAFETY: Every field satisfies the recursive fixed-data contract.
        unsafe impl<$($name: NativeData),+> NativeData for ($($name,)+) {}
    };
}
tuple_data!(A);
tuple_data!(A, B);
tuple_data!(A, B, C);
tuple_data!(A, B, C, D);
tuple_data!(A, B, C, D, E);
tuple_data!(A, B, C, D, E, F);
tuple_data!(A, B, C, D, E, F, G);
tuple_data!(A, B, C, D, E, F, G, H);

impl<T: NativeData> NativePayload for T {
    fn trace<'payload>(&'payload self, _: &mut dyn FnMut(&'payload Value)) {}

    fn units(&self) -> usize {
        size_of::<T>().div_ceil(size_of::<usize>())
    }
}

/// Define a named data struct and check every field before granting direct-edit
/// capability. The macro supplies Copy, Clone and Debug, plus empty tracing and
/// constant accounting through NativeData. Register with `NativeStorage::data`.
///
/// ```
/// use kagari_runtime::native_data;
/// native_data! {
///     /// Native game state with no script references.
///     pub struct PlayerState { pub hp: i32, pub position: [f32; 3] }
/// }
/// ```
///
/// Even an otherwise copyable heap identity is not supported data:
/// ```compile_fail
/// use kagari_runtime::{gc::HeapObjectId, native_data};
/// native_data! { struct Invalid { child: HeapObjectId } }
/// ```
#[macro_export]
macro_rules! native_data {
    ($(#[$attribute:meta])* $visibility:vis struct $name:ident {
        $($(#[$field_attribute:meta])* $field_visibility:vis $field:ident: $ty:ty),* $(,)?
    }) => {
        $(#[$attribute])*
        #[derive(Clone, Copy, Debug)]
        $visibility struct $name {
            $($(#[$field_attribute])* $field_visibility $field: $ty),*
        }
        const _: () = {
            fn check_fields() {
                fn check<T: $crate::native::payload::data::NativeData>() {}
                $(check::<$ty>();)*
            }
            // Reference the function without executing it; its body checks every
            // field even when the struct or a field is conditionally unused.
            let _ = check_fields;
        };
        // SAFETY: The macro defines the entire struct and checks every field.
        unsafe impl $crate::native::payload::data::NativeData for $name {}
    };
}
