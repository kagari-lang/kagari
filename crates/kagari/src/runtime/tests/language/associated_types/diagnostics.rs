use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;

#[test]
fn invalid_associated_type_contracts_are_rejected_before_execution() {
    let engine = KagariEngine::default();
    for source in [
        "trait Read { type Item; } struct N {} impl Read for N {}",
        "trait Read { type Item; type Item; }",
        "trait Read { type Item = i32; }",
        "trait Read { type Item; } struct N {} impl Read for N { type Item; }",
        "trait Read { type Item; } struct N {} impl Read for N { type Item = i32; type Item = i32; }",
        "trait Read { type Item; } struct N {} impl Read for N { type Other = i32; }",
        "struct N {} impl N { type Item = i32; }",
        "struct Box<T> { val x: T } fn bad() { val x = Box<Item = i32> { x: 0 }; }",
        "trait Read { type Item; } struct N {} fn bad() -> <N as Read>::Item { 0 }",
        "trait Bound {} trait Read { type Item; } struct N<T> { val value: T } impl<T: Bound> Read for N<T> { type Item = i32; } fn bad() -> <N<i32> as Read>::Item { 0 }",
        "trait Read { type Item; } struct N {} impl Read for N { type Item = Self::Item; }",
        "trait Read { type Item; fn read(self) -> Self::Item; } fn bad(r: Read) {}",
        "trait Read { type Item; } fn bad(r: Read<Other = i32>) {}",
        "trait Read { type Item; } fn bad(r: Read<Item = i32, Item = String>) {}",
        "trait Read<T> { type Item; } fn bad(r: Read<Item = i32, i32>) {}",
        "trait Read { type Item; } fn bad<R: Read>(r: R) -> R::Missing { 0 }",
        "trait Read { type Item; } fn bad<R: Read>(r: R) -> <R as Read<Item = i32>>::Item { 0 }",
        "trait Left { type Item; } trait Right { type Item; } fn bad<R: Left + Right>(r: R) -> R::Item { 0 }",
        "trait Read { type Item; fn read(self) -> Self::Item; } struct N {} impl Read for N { type Item = i32; fn read(self) -> String { \"wrong\" } }",
        "trait Read { type Item; } struct N {} impl Read for N { type Item = i32; } impl Read for N { type Item = String; }",
        "trait Bound {} trait Read { type Item: Bound; } struct N {} impl Read for N { type Item = i32; }",
        "trait Bound {} trait Read { type Item: Bound; } fn bad(r: Read<Item = i32>) {}",
        "trait Read { type Item; } fn bad(r: Read<Item = String>) { val other: Read<Item = i32> = r; }",
        "trait Read { type Item; } struct N {} impl Read for N { type Item = String; } fn require<R: Read<Item = i32>>(r: R) {} fn bad() { require(N {}); }",
        "trait Bound {} trait Read { type Item; fn read(self) -> Self::Item; } struct N {} impl Read for N { type Item = i32; fn read(self) -> i32 { 42 } } fn require<R: Read>(r: R) where R::Item: Bound {} fn bad() { require(N {}); }",
    ] {
        let source = format!("{source} fn main() -> i32 {{ 42 }}");
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-associated.kgr", &source),
                    Default::default()
                )
                .is_err(),
            "accepted invalid contract: {source}"
        );
    }
}

#[test]
fn generic_interface_conversion_checks_implementation_bounds() {
    let engine = KagariEngine::default();
    for source in [
        "use std::hash::{Hash};\ntrait Read {} struct Holder<T> { val value: T } impl<T: Eq + Hash> Read for Holder<T> {} fn main() { val reader: Read = Holder { value: 1.5 }; }",
        "use std::hash::{Hash};\ntrait Read {} struct Holder<T> { val value: T } impl<T: Eq + Hash> Read for Holder<T> {} fn boxed<T>(value: Holder<T>) -> Read { value } fn main() {}",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-generic-interface.kgr", source),
                    Default::default()
                )
                .is_err(),
            "accepted {source}"
        );
    }
}
