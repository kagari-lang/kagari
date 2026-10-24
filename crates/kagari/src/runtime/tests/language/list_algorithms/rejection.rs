use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;

#[test]
fn ordering_and_equality_bounds_are_required_at_the_method_call() {
    let engine = KagariEngine::default();
    for source in [
        "use std::collections::{List};\nstruct Item { val key: i32 } fn main() { val list: List<Item> = Vec::from([Item { key: 1 }]); list.sorted(); }",
        "use std::collections::{List};\nfn main() { val list: List<f32> = Vec::from([1.0f32]); list.distinct(); }",
    ] {
        assert!(
            engine
                .compile_to_artifact(SourceFile::new("bounds.kgr", source), Default::default())
                .is_err(),
            "{source}"
        );
    }
}
