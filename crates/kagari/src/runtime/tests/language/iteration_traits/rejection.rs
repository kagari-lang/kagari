use crate::engine::KagariEngine;
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
        "fn main(){[1].chain([true]);}",
        "fn main(){[1].zip(42);}",
        "fn main(){[1].filter_map(|x|x);}",
        "fn main(){[1].for_each(|x|x);}",
    ] {
        assert!(
            KagariEngine::default()
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
            KagariEngine::default()
                .compile_to_artifact(
                    SourceFile::new("removed-iterator-api.kgr", source),
                    Default::default(),
                )
                .is_err(),
            "{source}"
        );
    }
}
