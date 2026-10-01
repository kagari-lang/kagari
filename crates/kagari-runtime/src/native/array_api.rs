//! Rust contracts supply signatures, executable adapters and generated tooling views.
use kagari_native_macros::native_module;

#[native_module("std::array", runtime = crate, catalog)]
pub mod array {
    use crate::{
        native::{array, ops_api::ops::Index, sorting},
        native_value::{
            NativeCall, NativeResult, NativeValue,
            array::NativeArray,
            continuation::{NativeContinuation, NativeFn},
            selected::NativeSelected,
        },
    };
    use std::cmp::Ordering;

    /// Shared mutable array storage. Read-only List views share its identity.
    #[native_type]
    pub struct ArrayList<T: NativeValue>(NativeArray<T>);

    /// Shared read access with checked indexing.
    #[native_trait(parents("std::ops::Index"))]
    pub trait List<T: NativeValue>: Index<usize, Output = T> {
        /// Return the current slot count.
        fn len(&self) -> usize;
        /// Return whether there are no slots.
        fn is_empty(&self) -> bool;
        /// Return the addressed value, or None when index is outside the array.
        fn get(&self, index: usize) -> NativeResult<Option<T>>;
    }

    /// Mutable list access through declared methods.
    #[native_trait]
    pub trait MutableList<T: NativeValue>: List<T> {
        /// Replace a valid slot, trapping before mutation for an invalid index.
        fn set(&self, index: usize, value: T) -> NativeResult<()>;
    }

    #[native_impl]
    impl<T: NativeValue> ArrayList<T> {
        /// Allocate an empty array. The item type must be inferable from context.
        #[native(binding = "array_new")]
        pub fn new(#[context] call: &NativeCall) -> NativeResult<Self> {
            Ok(Self(NativeArray::new(call)?))
        }
        /// Return the current slot count.
        #[native(binding = "array_len")]
        pub fn len(&self) -> usize {
            self.0.len()
        }
        /// Return whether there are no slots.
        #[native]
        pub fn is_empty(&self) -> bool {
            self.0.is_empty()
        }
        /// Append a value. Allocation and iteration guards are checked before mutation.
        #[native(binding = "array_push", steps = 2)]
        pub fn push(&self, value: T) -> NativeResult<()> {
            self.0.push(value)
        }
        /// Build slots by calling make once per index in ascending order.
        #[native(binding = "array_from_fn")]
        pub fn from_fn(count: usize, make: NativeFn<(usize,), T>) -> NativeContinuation<Self> {
            array::from_fn(count, make)
        }
        /// Stable ordering; comparison failure leaves the original slots unchanged.
        #[native]
        pub fn sort_by(
            &self,
            compare: NativeFn<(T, T), Ordering>,
        ) -> NativeResult<NativeContinuation<()>> {
            sorting::stable_sort(&self.0, compare)
        }
        /// Stable ordering through the caller's checked Ord implementation.
        #[native]
        pub fn sort(
            &self,
            #[selected(T: std::cmp::Ord::cmp)] compare: NativeSelected<(T, T), Ordering>,
        ) -> NativeResult<NativeContinuation<()>> {
            sorting::stable_sort(&self.0, compare)
        }
    }

    impl<T: NativeValue> Index<usize> for ArrayList<T> {
        type Output = T;
        fn index(&self, index: usize) -> NativeResult<T> {
            self.0.index(index)
        }
    }

    #[native_impl]
    impl<T: NativeValue> List<T> for ArrayList<T> {
        #[native(binding = "array_list_len")]
        fn len(&self) -> usize {
            self.0.len()
        }
        #[native]
        fn is_empty(&self) -> bool {
            self.0.is_empty()
        }
        #[native(binding = "array_get")]
        fn get(&self, index: usize) -> NativeResult<Option<T>> {
            self.0.get(index)
        }
    }

    #[native_impl]
    impl<T: NativeValue> MutableList<T> for ArrayList<T> {
        #[native(binding = "array_set")]
        fn set(&self, index: usize, value: T) -> NativeResult<()> {
            self.0.set(index, value)
        }
    }
}
