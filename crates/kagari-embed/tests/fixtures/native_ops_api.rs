//! An application owns range/enum aliases through the same native representation adapter.
use kagari_native_macros::native_module;

#[native_module("game::shapes")]
pub mod shapes {
    use kagari_runtime::native_value::{
        NativeResult, NativeValue,
        range::{Exclusive, From, Full, Inclusive, NativeRange, To, ToInclusive},
    };
    use std::ops::Bound;

    /// Application-owned range spelling.
    #[native_type]
    pub type Span<T: NativeValue> = NativeRange<T, Exclusive>;
    #[native_type]
    pub type Closed<T: NativeValue> = NativeRange<T, Inclusive>;
    #[native_type]
    pub type Tail<T: NativeValue> = NativeRange<T, From>;
    #[native_type]
    pub type Head<T: NativeValue> = NativeRange<T, To>;
    #[native_type]
    pub type ClosedHead<T: NativeValue> = NativeRange<T, ToInclusive>;
    #[native_type]
    pub type Whole = NativeRange<(), Full>;
    /// Checked bound payloads may contain arbitrary rooted script values.
    #[native_type]
    pub type Edge<T: NativeValue> = Bound<T>;

    #[native]
    pub fn edges<T: NativeValue>(value: Span<T>) -> NativeResult<(Edge<T>, Edge<T>)> {
        Ok((value.start_bound()?, value.end_bound()?))
    }
    #[native]
    pub fn closed_edges<T: NativeValue>(value: Closed<T>) -> NativeResult<(Edge<T>, Edge<T>)> {
        Ok((value.start_bound()?, value.end_bound()?))
    }
    #[native]
    pub fn tail<T: NativeValue>(value: Tail<T>) -> NativeResult<Edge<T>> {
        value.start_bound()
    }
    #[native]
    pub fn head<T: NativeValue>(value: Head<T>) -> NativeResult<Edge<T>> {
        value.end_bound()
    }
    #[native]
    pub fn closed_head<T: NativeValue>(value: ClosedHead<T>) -> NativeResult<Edge<T>> {
        value.end_bound()
    }
    #[native]
    pub fn full(value: Whole) -> NativeResult<bool> {
        Ok(matches!(value.start_bound()?, Bound::Unbounded)
            && matches!(value.end_bound()?, Bound::Unbounded))
    }
    #[native]
    pub fn identity<T: NativeValue>(value: Edge<T>) -> Edge<T> {
        value
    }
    #[native]
    pub fn span_identity<T: NativeValue>(value: Span<T>) -> Span<T> {
        value
    }
}
