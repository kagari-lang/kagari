//! Closed owned cursor data cannot contain untraced script handles or references.
use std::fmt::Debug;

mod sealed {
    pub trait Sealed {}
}

/// Idle algorithm data. Script values belong in checked capture fields.
/// Scalars, tuples and fixed-size arrays support counters and adapter phases.
pub trait NativeStateData: sealed::Sealed + Copy + Debug + 'static {}

macro_rules! scalar {
    ($($ty:ty),+) => {$(
        impl sealed::Sealed for $ty {}
        impl NativeStateData for $ty {}
    )+};
}
scalar!(
    (),
    bool,
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

impl<T: NativeStateData, const N: usize> sealed::Sealed for [T; N] {}
impl<T: NativeStateData, const N: usize> NativeStateData for [T; N] {}

macro_rules! tuple {
    ($($ty:ident),+) => {
        impl<$($ty: NativeStateData),+> sealed::Sealed for ($($ty,)+) {}
        impl<$($ty: NativeStateData),+> NativeStateData for ($($ty,)+) {}
    };
}
tuple!(A);
tuple!(A, B);
tuple!(A, B, C);
tuple!(A, B, C, D);
tuple!(A, B, C, D, E);
tuple!(A, B, C, D, E, F);
tuple!(A, B, C, D, E, F, G);
tuple!(A, B, C, D, E, F, G, H);
