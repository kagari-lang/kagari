use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_hir::types::*;
use kagari_source::source_database::*;
use kagari_types::scalar::*;

#[test]
fn explicit_constructor_type_queries_follow_each_snapshot_after_neighbor_edits() {
    let text = "fn neighbor() -> i32 { 1 } struct Marker<T> { val value: i32 } enum Token<T> { Empty } fn make() { Marker<bool> { value: 7 }; Token<bool>::Empty; }";
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("explicit-constructors.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let old = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    old.check_program(root, &Default::default()).unwrap();
    let changed = text.replace("{ 1 }", "{ 2 + 3 }");
    sources
        .set(
            "explicit-constructors.kgr",
            changed.clone(),
            SourceLayer::Base,
        )
        .unwrap();
    let new = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    new.check_program(root, &Default::default()).unwrap();

    for (snapshot, source) in [(&old, text), (&new, changed.as_str())] {
        for (offset, _) in source.match_indices("bool") {
            assert_eq!(
                snapshot.file(root).unwrap().queries().type_at(offset),
                Some(TypeId::Builtin(BuiltinType::Bool))
            );
        }
    }
}
