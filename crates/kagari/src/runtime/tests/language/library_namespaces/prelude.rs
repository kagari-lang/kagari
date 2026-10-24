use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;

#[test]
fn explicit_prelude_requires_imports_and_local_names_shadow_defaults() {
    let engine = KagariEngine::new(Default::default());
    for name in [
        "Hash",
        "Debug",
        "Display",
        "Add<i32>",
        "HashMap<i32, i32>",
        "HashSet<i32>",
        "List<i32>",
    ] {
        let source = format!("fn missing(value: {name}) {{}}");
        assert!(
            engine
                .compile_to_artifact(SourceFile::new("missing.kgr", source), Default::default())
                .is_err(),
            "{name}"
        );
    }
    for source in [
        "fn identity(value: Vec<i32>) -> Option<Vec<i32>> { Some(value) }",
        "fn future(value: Future<i32>) -> std::future::Future<i32> { val core: core::future::Future<i32> = value; core }",
        "struct Vec { val value: i32 } fn main() -> i32 { Vec { value: 42 }.value }",
        "use std::ops::Add; fn plus<T: Add<T>>(a: T, b: T) -> T::Output { a + b }",
        "use std::iter::Iterable; fn source<T: Iterable>(value: T) -> T::Iter { value.iter() }",
    ] {
        engine
            .compile_to_artifact(SourceFile::new("prelude.kgr", source), Default::default())
            .unwrap();
    }
    for source in [
        "use core::language::Add; fn main() {}",
        "fn main() { val values: ArrayList<i32> = []; }",
        "mod std {} fn main(value: std::vec::Vec<i32>) {}",
    ] {
        assert!(
            engine
                .compile_to_artifact(SourceFile::new("retired.kgr", source), Default::default())
                .is_err()
        );
    }
}
