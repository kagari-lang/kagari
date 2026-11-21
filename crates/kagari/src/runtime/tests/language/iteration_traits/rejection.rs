use crate::engine::KagariEngine;
use crate::test_support::standard::standard_library;
use kagari_source::source::SourceFile;

#[test]
fn invalid_iterator_outputs_and_overrides_are_diagnostics() {
    for source in [
        "fn removed(value: Cursor<i32>) {}",
        "struct C{} impl Iterator for C {type Item=i32;fn next(self)->i32 {42}}",
        "struct C{} impl Iterable for C {type Item=i32;type Iter=i32;fn iter(self)->i32 {42}}",
        "struct C{} impl Iterator for C {type Item=i32;fn next(self)->Option<i32>{None}} impl Iterable for C {type Item=i32;type Iter=C;fn iter(self)->C {self}}",
        "struct C{} impl Iterator for C {type Item=String;fn next(self)->Option<String>{None}} struct R{} impl Iterable for R {type Item=i32;type Iter=C;fn iter(self)->C{C{}}}",
        "fn take<T:Iterable<Item=String>>(x:T){} fn main(){take([42]);}",
        "fn bad(it:Iterator<Item=i32>,other:Iterator<Item=bool>){it.chain(other);}",
        "fn bad(it:Iterator<Item=i32>){it.zip(42);}",
        "fn bad(it:Iterator<Item=i32>){it.filter_map(|x|x);}",
        "fn bad(it:Iterator<Item=i32>){it.for_each(|x|x);}",
    ] {
        assert!(
            KagariEngine::with_native_modules(
                Default::default(),
                standard_library().modules().unwrap()
            )
            .unwrap()
            .compile_to_artifact(
                SourceFile::new("invalid-iterator.kgr", source),
                Default::default()
            )
            .is_err(),
            "{source}"
        );
    }
}

#[test]
fn removed_iterator_apis_are_rejected() {
    for source in [
        "fn main(){[1].into_iter();}",
        "fn consume<I:IntoIterator>(values:I){}",
    ] {
        assert!(
            KagariEngine::with_native_modules(
                Default::default(),
                standard_library().modules().unwrap()
            )
            .unwrap()
            .compile_to_artifact(
                SourceFile::new("removed-iterator-api.kgr", source),
                Default::default(),
            )
            .is_err(),
            "{source}"
        );
    }
}
