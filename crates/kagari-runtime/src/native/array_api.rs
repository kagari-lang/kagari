//! Declarative array registration; executable algorithms live in array.rs.
use crate::{NativeApi, RuntimeError, native::array, native_module};

pub(super) fn api() -> Result<NativeApi, RuntimeError> {
    native_module! {
        runtime = crate;
        module std::array;

        /// Shared mutable array storage. Read-only List views share its identity.
        type ArrayList<T> = native_array<T>;

        /// Shared read access with checked indexing.
        trait List<T>: std::ops::Index<usize, Output = T> {
            /// Return the current slot count.
            fn len(self) -> usize;
            /// Return the addressed value, or None when index is outside the array.
            fn get(self, index: usize) -> Option<T>;
        }

        /// Mutable list access through declared methods.
        trait MutableList<T>: List<T> {
            /// Replace a valid slot, trapping before mutation for an invalid index.
            fn set(self, index: usize, value: T);
        }

        impl<T> ArrayList<T> {
            /// Allocate an empty array. The item type must be inferable from context.
            fn new() -> ArrayList<T> => array::array_new;
            /// Return the current slot count.
            fn len(self) -> usize => array::array_len;
            /// Append a value. Allocation and iteration guards are checked before mutation.
            fn push(self, value: T) => array::array_push;
            /// Build slots by calling make once per index in ascending order.
            fn from_fn(count: usize, make: fn(usize) -> T) -> ArrayList<T> => array::array_from_fn;
        }

        impl<T> List<T> for ArrayList<T> {
            len => array::array_len;
            get => array::array_get;
        }

        impl<T> MutableList<T> for ArrayList<T> {
            set => array::array_set;
        }
    }
}
