use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;

#[test]
fn invalid_numeric_literals_are_rejected_before_execution() {
    for expression in [
        "256u8",
        "128i8",
        "-129i8",
        "-1u8",
        "18446744073709551616u64",
        "1.0u8",
        "1wat",
        "1e999f64",
    ] {
        let engine =
            KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap())
                .unwrap();
        assert!(
            engine
                .compile_source(SourceFile::new(
                    "invalid-number.kgr",
                    format!("fn main() {{ val x = {expression}; }}")
                ))
                .is_err(),
            "{expression}"
        );
    }
}

#[test]
fn invalid_explicit_arguments_and_unresolved_holes_are_rejected() {
    for source in [
        "fn identity<T>(x: T) -> T { x } fn main() { identity::<i32>(true); }",
        "fn identity<T>(x: T) -> T { x } fn main() { identity::<i32, bool>(1); }",
        "fn identity<T>(x: T) -> T { x } fn main() { identity::<>(1); }",
        "fn identity<T>(x: T) -> T { x } fn main() { identity::<Missing>(1); }",
        "fn identity<T>(x: T) -> T { x } fn main() { identity::<T = i32>(1); }",
        "fn main() { val f = |x: i32| x; f::<i32>(1); }",
        "fn main() { val values: Vec<_> = Vec::from([]); }",
        "fn main(x: _) {}",
        "struct Bad { val field: _ } fn main() {}",
        "fn main() { val x: i64 = Some(1).map::<i32>(|x| x).unwrap(); }",
        "fn main() { val x: Result<bool, String> = Ok::<i32, String>(1); }",
    ] {
        assert!(
            KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap())
                .unwrap()
                .compile_source(SourceFile::new("invalid-inference.kgr", source))
                .is_err(),
            "unexpectedly accepted: {source}"
        );
    }
}
