use crate::{engine::KagariEngine, error::EmbeddingError};
use kagari_source::source::SourceFile;

#[test]
fn readonly_views_do_not_grant_mutators_or_implicit_storage_construction() {
    let engine = KagariEngine::default();
    for source in [
        "fn main() { val xs: std::collections::List<i32> = Vec::from([1]); xs[0] = 2; }",
        "use std::collections::{MutableList};\nfn main() { val xs: MutableList<i32> = Vec::from([1]); xs[true] = 2; }",
        "use std::collections::{MutableList};\nfn main() { val xs: MutableList<i32> = Vec::from([1]); xs[0i32] = 2; }",
        "use std::collections::{List, MutableList};\nfn main() { val xs: List<i32> = Vec::from([1]); val ys: MutableList<i32> = xs; }",
        "use std::collections::{Map};\nfn main() { val xs: Map<i32,i32> = Map::new(); }",
        "use std::collections::{HashSet, Set};\nfn main() { val xs: Set<f64> = HashSet::new(); }",
        "use std::collections::List; use std::hash::Hash; fn main() { val xs: List<i32> = Vec::from([1]); xs.hash(); }",
        "use std::collections::{List, HashSet}; fn main() { val keys: HashSet<List<i32>> = HashSet::new(); }",
        "trait MutableList: std::ops::Index<u64, Output = i32> { fn set(self, index: u64, value: i32); } fn edit(xs: MutableList) { xs[0] = 2; }",
    ] {
        assert!(
            engine
                .compile_to_artifact(SourceFile::new("rejected.kgr", source), Default::default())
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn collection_interfaces_do_not_inherit_receiver_equality() {
    let engine = KagariEngine::default();
    for interface in [
        "List<i32>",
        "MutableList<i32>",
        "Set<i32>",
        "MutableSet<i32>",
        "Map<i32,i32>",
        "MutableMap<i32,i32>",
    ] {
        for expression in [
            "a == b",
            "a != b",
            "a.eq(b)",
            "same(a, b)",
            "(a,) == (b,)",
            "a === b",
        ] {
            let source = format!(
                "use std::collections::{{List, MutableList, Set, MutableSet, Map, MutableMap}}; fn same<T: PartialEq>(a: T, b: T) -> bool {{ a == b }} fn compare(a: {interface}, b: {interface}) -> bool {{ {expression} }}"
            );
            assert!(
                matches!(
                    engine.compile_to_artifact(
                        SourceFile::new("interface-comparison.kgr", source),
                        Default::default()
                    ),
                    Err(EmbeddingError::Diagnostics { .. })
                ),
                "{interface}: {expression}"
            );
        }
    }
}

#[test]
fn collection_content_requires_hash_and_nested_comparison_bounds() {
    let engine = KagariEngine::default();
    for source in [
        "use std::hash::Hash; fn main() { Vec::from([0.0f32]).hash(); }",
        "use std::collections::HashSet; fn main() { val xs: HashSet<Vec<f32>> = HashSet::new(); }",
        "use std::collections::HashMap; use std::hash::Hash; fn main() { val m: HashMap<i32,f32> = HashMap::new(); m.hash(); }",
    ] {
        assert!(
            engine
                .compile_to_artifact(SourceFile::new("rejected.kgr", source), Default::default())
                .is_err(),
            "{source}"
        );
    }
    for (bound, declarations, value) in [
        (
            "PartialEq",
            "trait Opaque {} impl Opaque for i32 {} enum Item { Value(Opaque) } fn item() -> Item { val value: Opaque = 1; Item::Value(value) }",
            "item()",
        ),
        ("Eq", "", "Vec::from([0.0f32])"),
    ] {
        let source = format!(
            "{declarations}\ntrait Marker {{}} struct Wrapped<T> {{ val value: T }} impl<T: {bound}> Marker for Wrapped<T> {{}} fn require<T: Marker>(value: T) {{}} fn main() {{ require(Wrapped {{ value: ({value}, 1) }}); }}"
        );
        let result = engine.compile_to_artifact(
            SourceFile::new("nested-bound.kgr", source),
            Default::default(),
        );
        let Err(EmbeddingError::Diagnostics { diagnostics }) = result else {
            panic!("nested {bound} failure must be rejected by source checking");
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED")
        );
    }
}
