use super::compile;

#[test]
fn cycles_missing_parents_and_distinct_parent_methods_are_diagnostics() {
    for source in [
        "trait A: A {} fn main() {}",
        "trait A: B {} trait B: A {} fn main() {}",
        "use std::collections::{List};\ntrait A<T>: A<List<T>> {} fn main() {}",
        "trait Bound {} trait Parent<T: Bound> {} trait Child<T>: Parent<T> {} fn main() {}",
        "trait Parent<T> { fn read(self) -> T; } trait Child: Parent<Self> {} fn bad(x: Child) {} fn main() {}",
        "trait Parent { type Item; } trait Child: Parent {} fn bad(x: Child) {} fn main() {}",
        "trait Parent { fn clone(self) -> Self; } trait Child: Parent {} fn bad(x: Child) {} fn main() {}",
        "trait Parent {} trait Child: Parent {} struct S {} impl Child for S {} fn main() {}",
        "trait A { fn read(self) -> i32; } trait B { fn read(self) -> i32; } trait C: A + B {} fn read<T: C>(x: T) -> i32 { x.read() } fn main() {}",
    ] {
        assert!(compile(source).is_err(), "accepted {source}");
    }
}
