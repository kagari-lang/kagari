use crate::{AnalyzerQueries, query_tests::test_analysis};

#[test]
fn constructor_binders_and_cached_queries_follow_the_latest_signature() {
    use kagari_source::source_database::{SourceDatabase, SourceLayer};
    let text = "trait Family { type Item<T>; fn make<T>(self, value:T)->Self::Item<T>; } struct N {} impl Family for N { type Item<U> = U; fn make<V>(self, value:V)->V { value } } fn edit()->i32 { 1 } fn main()->i32 { val value: <N as Family>::Item<i32> = N {}.make(42); value }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("gat-cache.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let old = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let old_file = old.file(file).unwrap();
    assert!(old_file.result().diagnostics().is_empty());
    let use_offset = text.find("= U;").unwrap() + 2;
    let target = old_file.queries().definition_at(use_offset).unwrap();
    assert_eq!(
        target.source_location().unwrap().range.start,
        text.find("Item<U>").unwrap() + 5
    );
    let edited = text.replace("{ 1 }", "{ val x = 2; x }");
    sources
        .set("gat-cache.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let unchanged = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(
        unchanged
            .file(file)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    let broken = edited.replace("type Item<U> = U", "type Item<U> = bool");
    sources
        .set("gat-cache.kgr", broken, SourceLayer::Overlay)
        .unwrap();
    let latest = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(!latest.file(file).unwrap().result().diagnostics().is_empty());
    assert!(old_file.result().diagnostics().is_empty());
}
