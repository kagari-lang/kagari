//! Operator contracts and range representations are ordinary registered declarations.
use kagari_native_macros::native_module;

#[native_module("std::ops", runtime = crate)]
pub mod ops {
    use crate::native_value::{
        NativeResult, NativeValue,
        range::{Exclusive, From, Full, Inclusive, NativeRange, To, ToInclusive},
    };
    use std::ops::Bound as RustBound;

    #[native_trait]
    pub trait Add<Rhs: NativeValue> {
        type Output: NativeValue;
        fn add(&self, rhs: Rhs) -> NativeResult<Self::Output>;
    }
    #[native_trait]
    pub trait Sub<Rhs: NativeValue> {
        type Output: NativeValue;
        fn sub(&self, rhs: Rhs) -> NativeResult<Self::Output>;
    }
    #[native_trait]
    pub trait Mul<Rhs: NativeValue> {
        type Output: NativeValue;
        fn mul(&self, rhs: Rhs) -> NativeResult<Self::Output>;
    }
    #[native_trait]
    pub trait Div<Rhs: NativeValue> {
        type Output: NativeValue;
        fn div(&self, rhs: Rhs) -> NativeResult<Self::Output>;
    }
    #[native_trait]
    pub trait Rem<Rhs: NativeValue> {
        type Output: NativeValue;
        fn rem(&self, rhs: Rhs) -> NativeResult<Self::Output>;
    }
    #[native_trait]
    pub trait Neg {
        type Output: NativeValue;
        fn neg(&self) -> NativeResult<Self::Output>;
    }
    #[native_trait]
    pub trait Not {
        type Output: NativeValue;
        fn not(&self) -> NativeResult<Self::Output>;
    }
    #[native_trait]
    pub trait Index<Rhs: NativeValue> {
        type Output: NativeValue;
        fn index(&self, rhs: Rhs) -> NativeResult<Self::Output>;
    }
    #[native_trait]
    pub trait BitAnd<Rhs: NativeValue> {
        type Output: NativeValue;
        fn bitand(&self, rhs: Rhs) -> NativeResult<Self::Output>;
    }
    #[native_trait]
    pub trait BitOr<Rhs: NativeValue> {
        type Output: NativeValue;
        fn bitor(&self, rhs: Rhs) -> NativeResult<Self::Output>;
    }
    #[native_trait]
    pub trait BitXor<Rhs: NativeValue> {
        type Output: NativeValue;
        fn bitxor(&self, rhs: Rhs) -> NativeResult<Self::Output>;
    }
    #[native_trait]
    pub trait Shl<Rhs: NativeValue> {
        type Output: NativeValue;
        fn shl(&self, rhs: Rhs) -> NativeResult<Self::Output>;
    }
    #[native_trait]
    pub trait Shr<Rhs: NativeValue> {
        type Output: NativeValue;
        fn shr(&self, rhs: Rhs) -> NativeResult<Self::Output>;
    }

    #[native_type]
    pub type Range<T: NativeValue> = NativeRange<T, Exclusive>;
    #[native_type]
    pub type RangeInclusive<T: NativeValue> = NativeRange<T, Inclusive>;
    #[native_type]
    pub type RangeFrom<T: NativeValue> = NativeRange<T, From>;
    #[native_type]
    pub type RangeTo<T: NativeValue> = NativeRange<T, To>;
    #[native_type]
    pub type RangeToInclusive<T: NativeValue> = NativeRange<T, ToInclusive>;
    #[native_type]
    pub type RangeFull = NativeRange<(), Full>;
    #[native_type]
    pub type Bound<T: NativeValue> = RustBound<T>;

    #[native_trait]
    pub trait RangeBounds<T: NativeValue> {
        fn start_bound(&self) -> NativeResult<Bound<T>>;
        fn end_bound(&self) -> NativeResult<Bound<T>>;
    }
    #[native_trait]
    pub trait Fn<Args: NativeValue> {
        type Output: NativeValue;
        fn call(&self, args: Args) -> NativeResult<Self::Output>;
    }
}
