use crate::engine::KagariEngine;
use crate::test_support::standard::standard_library;
use kagari_source::source::SourceFile;

#[test]
fn callable_errors_are_reported_before_codegen() {
    for source in [
        "fn apply<F: Fn(i32) -> i32>(f:F) {} fn main(){apply(|x: String| 1);}",
        "fn apply<F: Fn(i32) -> i32>(f:F) {} fn main(){apply(|x: i32| true);}",
        "struct X {} fn main(){val f:fn(i32)->i32=X{};}",
        "trait Other<T>{type Output;} fn bad<F: Other(i32)->i32>(f:F){}",
        "fn bad<F: Fn(i32)->i32>(f:F){ f(); }",
        "fn bad<F: Fn(i32)->i32>(f:F){ f(1,2); }",
        "fn bad<F: Fn(i32)->i32>(f:F){ f(true); }",
    ] {
        let engine = KagariEngine::with_native_modules(
            Default::default(),
            standard_library().modules().unwrap(),
        )
        .unwrap();
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-callable.kgr", source),
                    Default::default()
                )
                .is_err(),
            "{source}"
        );
    }
}
