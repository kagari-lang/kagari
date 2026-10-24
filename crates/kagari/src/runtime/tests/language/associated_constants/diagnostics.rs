use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;

#[test]
fn invalid_constant_contracts_fail_before_execution() {
    for source in [
        "trait Limit { const VALUE: i32; } struct N {} impl Limit for N {}",
        "trait Limit { const VALUE: i32; } struct N {} impl Limit for N { const VALUE: bool = true; }",
        "trait Limit { const VALUE: i32 = 1; } struct N {} impl Limit for N { const EXTRA: i32 = 1; }",
        "trait Limit { const VALUE: i32; } struct N {} impl Limit for N { const VALUE: i32; }",
        "trait Limit { const VALUE: i32 = 1; const VALUE: i32 = 2; }",
        "use std::collections::List; trait Limit { const VALUE: List<i32> = Vec::from([1]); }",
        "trait Limit { const VALUE: i32 = 2147483647 + 1; }",
        "fn f() -> i32 { 1 } trait Limit { const VALUE: i32 = f(); }",
        "trait Limit { const VALUE: i32 = 1; } fn dynamic(x: Limit) {}",
        "trait Limit { const VALUE: i32 = 1; } trait Child: Limit {} fn dynamic(x: Child) {}",
        "trait Left { const VALUE: i32 = 1; } trait Right { const VALUE: i32 = 2; } fn f<T: Left + Right>(x: T) -> i32 { T::VALUE }",
    ] {
        assert!(
            KagariEngine::default()
                .compile_to_artifact(SourceFile::new("invalid.kgr", source), Default::default())
                .is_err(),
            "accepted {source}"
        );
    }
}
