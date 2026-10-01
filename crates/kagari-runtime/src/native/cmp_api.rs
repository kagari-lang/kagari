//! Comparison protocols and their actual Rust scalar implementations.
// Expand real Rust impls before native_module derives their checked contracts.
macro_rules! comparison_module {
    (total: [$($total:ty),*], partial: [$($partial:ty),*]) => {
        #[kagari_native_macros::native_module("std::cmp", runtime = crate)]
        pub mod cmp {
            use std::cmp::{
                Ord as RustOrd, Ordering as RustOrdering,
                PartialEq as RustPartialEq, PartialOrd as RustPartialOrd,
            };

            /// The checked result of a total ordering comparison.
            #[native_type]
            pub type Ordering = RustOrdering;

            /// Equality, which need not be reflexive for floating-point values.
            #[native_trait]
            pub trait PartialEq {
                fn eq(&self, other: Self) -> bool;
            }
            /// Reflexive equality.
            #[native_trait]
            pub trait Eq: PartialEq {}
            /// Ordering that may leave two values incomparable.
            #[native_trait]
            pub trait PartialOrd: PartialEq {
                fn partial_cmp(&self, other: Self) -> Option<Ordering>;
            }
            /// A total ordering consistent with equality and partial ordering.
            #[native_trait]
            pub trait Ord: Eq + PartialOrd {
                fn cmp(&self, other: Self) -> Ordering;
            }

            $(
                #[native_impl]
                impl PartialEq for $total {
                    fn eq(&self, other: Self) -> bool { RustPartialEq::eq(self, &other) }
                }
                #[native_impl]
                impl Eq for $total {}
                #[native_impl]
                impl PartialOrd for $total {
                    fn partial_cmp(&self, other: Self) -> Option<Ordering> {
                        RustPartialOrd::partial_cmp(self, &other)
                    }
                }
                #[native_impl]
                impl Ord for $total {
                    fn cmp(&self, other: Self) -> Ordering { RustOrd::cmp(self, &other) }
                }
            )*
            $(
                #[native_impl]
                impl PartialEq for $partial {
                    fn eq(&self, other: Self) -> bool { RustPartialEq::eq(self, &other) }
                }
                #[native_impl]
                impl PartialOrd for $partial {
                    fn partial_cmp(&self, other: Self) -> Option<Ordering> {
                        RustPartialOrd::partial_cmp(self, &other)
                    }
                }
            )*
        }
    };
}

comparison_module! {
    total: [i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, bool, String, Ordering, ()],
    partial: [f32, f64]
}
