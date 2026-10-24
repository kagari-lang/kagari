//! Static capability rejection, separate from successful execution fixtures.
use crate::{engine::KagariEngine, error::EmbeddingError};
use kagari_source::source::SourceFile;

#[test]
fn char_does_not_acquire_numeric_or_traversal_capabilities() {
    let engine = KagariEngine::default();
    for source in [
        "fn main()->char { 'a' + 'b' }",
        "fn main()->char { 'a' & 'b' }",
        "fn main()->u32 { 'a' as u32 }",
        "fn main()->char { 65u32 as char }",
        "fn main()->char { char::from(65u32) }",
        "fn main()->char { val value: char = 65u32; value }",
        "fn main()->bool { 'a' == 97u32 }",
        "use core::iter::Sum; fn needs<T: Sum<T>>(value:T) {} fn main() { needs('a'); }",
        "use core::iter::Product; fn needs<T: Product<T>>(value:T) {} fn main() { needs('a'); }",
        "use core::str::FromStr; fn needs<T: FromStr>(value:T) {} fn main() { needs('a'); }",
        "fn main() { for value in 'a'..'z' {} }",
        "fn main() { for value in 'a'..='z' {} }",
        "fn main() { for value in 'a'.. {} }",
    ] {
        let error = engine
            .compile_to_artifact(
                SourceFile::new("char-capabilities.kgr", source),
                Default::default(),
            )
            .unwrap_err();
        assert!(
            matches!(error, EmbeddingError::Diagnostics { .. }),
            "{source}: {error:?}"
        );
    }
}

#[test]
fn invalid_standard_trait_uses_report_semantic_diagnostics() {
    for source in [
        "fn needs<T: Eq>(x:T) {} fn main() { needs(1.5); }",
        "use std::hash::{Hash};\nfn needs<T: Hash>(x:T) {} fn main() { needs(1.5); }",
        "use std::collections::{HashMap};\nenum Key { Good(i32), Bad(f64) } fn main() { val map: HashMap<Key, i32> = HashMap::new(); }",
        "use std::hash::{Hash};\nenum Key { Good(i32), Bad(f64) } fn needs<T: Eq + Hash>(x:T) {} fn main() { needs(Key::Good(1)); }",
        "fn needs<T: Eq<i32>>(x:T) {} fn main() {}",
        "struct Item {} impl Eq for Item {} fn main() {}",
        "use std::hash::{Hash};\nstruct Item {} impl Hash for Item { fn hash(self)->i64 { 1 } } fn main() {}",
        "use std::fmt::{Debug};\nimpl Debug for i32 { fn debug(self)->String { \"x\" } } fn main() {}",
        "use std::fmt::{Debug};\nstruct Item {} impl Debug for Item { fn debug(self)->i32 { 1 } } fn main() {}",
        "use std::fmt::{Display};\nstruct Item {} impl Display for Item {} fn main() {}",
        "trait Eq {} fn f<T: Eq>(x:T)->bool { x == x } fn main() {}",
        "use std::collections::{HashSet};\ntrait Hash {} fn f<T: Eq + Hash>(x:T)->HashSet<T> { HashSet::new() } fn main() {}",
        "fn f<T: HashKey>(x:T) {} fn main() {}",
    ] {
        let error = KagariEngine::default()
            .compile_to_artifact(
                SourceFile::new("bad-traits.kgr", source),
                Default::default(),
            )
            .unwrap_err();
        assert!(
            matches!(error, EmbeddingError::Diagnostics { .. }),
            "{source}: {error:?}"
        );
    }
}

#[test]
fn identity_operators_reject_value_types() {
    for expression in [
        "1 === 1",
        "true !== false",
        "\"a\" === \"a\"",
        "(1,2) === (1,2)",
        "Some(1) === Some(1)",
    ] {
        let source = format!("fn main()->bool {{ {expression} }}");
        assert!(matches!(
            KagariEngine::default().compile_to_artifact(
                SourceFile::new("bad-identity.kgr", source),
                Default::default()
            ),
            Err(EmbeddingError::Diagnostics { .. })
        ));
    }
}

#[test]
fn comparison_only_types_do_not_inherit_identity_hashing() {
    for tail in [
        "use std::collections::{HashSet};\nfn main(){val set:HashSet<Key> = HashSet::new();}",
        "use std::collections::{HashSet};\nfn main(){val set:HashSet<(Key,i32)> = HashSet::new();}",
        "use std::collections::{HashSet};\nenum E {Value(Key)} fn main(){val set:HashSet<E> = HashSet::new();}",
        "use std::hash::{Hash};\nfn needs<T:Eq+Hash>(v:T){} fn main(){needs(Key{});}",
        "fn main(){Key{}.hash();}",
    ] {
        let source = format!(
            "struct Key {{}} impl PartialEq for Key {{fn eq(self,other:Self)->bool {{true}}}} impl Eq for Key {{}} {tail}"
        );
        assert!(
            matches!(
                KagariEngine::default().compile_to_artifact(
                    SourceFile::new("bad-key.kgr", source),
                    Default::default()
                ),
                Err(EmbeddingError::Diagnostics { .. })
            ),
            "{tail}"
        );
    }
}

#[test]
fn equality_requires_an_explicit_or_conditional_static_implementation() {
    let engine = KagariEngine::default();
    for source in [
        "struct Item {} fn main() -> bool { val a = Item {}; a == a }",
        "enum Item { One } fn main() -> bool { Item::One == Item::One }",
        "struct Item {} fn main() -> bool { val a = Vec::from([Item {}]); a == a }",
        "struct Item {} fn main() -> bool { val a = [Item {}]; a == a }",
        "struct Item {} fn main() -> bool { val a = Some(Item {}); a == a }",
        "struct Item {} fn main() -> bool { val a = (Item {},); a == a }",
        "fn same<T>(a: T, b: T) -> bool { a == b }",
        "trait Read {} struct Item {} impl Read for Item {} impl PartialEq for Item { fn eq(self, other: Self) -> bool { true } } fn main() -> bool { val a: Read = Item {}; a == a }",
        "struct Item {} fn main() -> bool { Item {}.eq(Item {}) }",
        "fn main() -> i64 { [1].hash() }",
    ] {
        assert!(
            matches!(
                engine.compile_to_artifact(
                    SourceFile::new("comparison-required.kgr", source),
                    Default::default()
                ),
                Err(EmbeddingError::Diagnostics { .. })
            ),
            "{source}"
        );
    }
}
