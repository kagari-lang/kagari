use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;

#[test]
fn invalid_family_declarations_projections_and_dynamic_interfaces_are_rejected() {
    for source in [
        "trait Family { type Item<T>; } struct N {} impl Family for N {}",
        "trait Family { type Item<T>; } struct N {} impl Family for N { type Item = i32; }",
        "trait Family { type Item<T>; } struct N {} impl Family for N { type Item<U, V> = U; }",
        "trait Family { type Item<T>; } struct N {} impl Family for N { type Item<U> = Unknown; }",
        "trait Family { type Item<T>; } struct N {} impl Family for N { type Item<U> = Self::Item<U>; }",
        "trait Family { type Item<T>; } struct N {} impl Family for N { type Item<U> = Self::Other<U>; type Other<U> = U; }",
        "trait Family { type Item<T>; } fn f<T: Family>(x: T) -> T::Item { 0 }",
        "trait Family { type Item<T>; } fn f<T: Family>(x: T) -> T::Item<i32, bool> { 0 }",
        "trait Family { type Item<T>; } fn f(x: Family) {}",
        "trait Family { type Item<T>; } trait Child: Family {} fn f(x: Child) {}",
        "trait Family { type Item<T>; } fn f<T: Family<Item=i32>>(x: T) {}",
        "use std::hash::Hash; trait Family { type Item<T: PartialEq>; } struct N {} impl Family for N { type Item<U: PartialEq + Eq + Hash> = U; }",
        "trait Family { type Item<T>: PartialEq; } struct N {} impl Family for N { type Item<U> = U; }",
        "use std::hash::{Hash};\ntrait Family { type Item<T: Eq + Hash>; } struct N {} impl Family for N { type Item<U> = U; } fn f(x: <N as Family>::Item<f32>) {}",
        "trait Family { type Item<T> = T; }",
        "use std::hash::{Hash};\ntrait Family { type Item<T: Eq + Hash>; } struct N {} impl Family for N { type Item<U> = i32; } fn main()->i32 { val x: <N as Family>::Item<f32> = 42; x }",
        "trait Family { type A<T>; type B<T>; } struct N {} impl Family for N { type A<U> = Self::B<U>; type B<U> = Self::A<U>; }",
        "trait Family { type Item<'a>; }",
        "trait Family { type Item<const N: i32>; }",
        "trait Family { type Item<T, T>; }",
    ] {
        assert!(
            KagariEngine::default()
                .compile_to_artifact(SourceFile::new("invalid.kgr", source), Default::default())
                .is_err(),
            "accepted {source}"
        );
    }
}
