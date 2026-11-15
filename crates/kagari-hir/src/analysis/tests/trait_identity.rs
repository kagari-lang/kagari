use crate::analysis::*;
use crate::{declarations::DeclarationId, tests::test_analysis, typeck::table::CallTarget};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

const SOURCE: &str = "trait Get { fn get(self) -> i32; } struct Point { val n: i32 } impl Get for Point { fn get(self) -> i32 { self.n } } fn read<T: Get>(p: T) -> i32 { p.get() } fn main() -> i32 { read(Point { n: 7 }) }";

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn cached_body_queries_rebase_receivers_without_changing_nominal_method_targets() {
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("cache.kgr", SOURCE.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    let authoring_before = old
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let before = authoring_before.records().facts();
    let read = before
        .declarations
        .iter()
        .find(|d| d.name == "read")
        .unwrap();
    let DeclarationId::Definition(owner) = &read.id else {
        panic!("function declaration");
    };
    let original = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    let old_call = original
        .lowered()
        .module
        .body
        .expressions()
        .find_map(|(expr, _)| {
            original
                .type_table()
                .call_resolution(expr)
                .filter(|call| matches!(call.target, CallTarget::TraitMethod { .. }))
                .map(|call| (expr, call))
        })
        .unwrap();
    sources
        .set(
            "cache.kgr",
            SOURCE.replace("{ self.n }", "{ val n = self.n; n }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let reused = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(reused.reused_bodies(), 1);
    assert!(reused.type_table().call_resolution(old_call.0).is_none());
    let new_call = reused
        .lowered()
        .module
        .body
        .expressions()
        .find_map(|(id, _)| {
            reused
                .type_table()
                .call_resolution(id)
                .filter(|call| matches!(call.target, CallTarget::TraitMethod { .. }))
        })
        .unwrap();
    assert_eq!(old_call.1.target, new_call.target);
    assert_ne!(
        old_call.1.receiver.unwrap().arena(),
        new_call.receiver.unwrap().arena()
    );
    let mut fresh_db = test_analysis();
    let fresh_declarations = fresh_db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let query = original.definitions().resolve(*owner).unwrap().to_path();
    let fresh_owner = fresh_declarations
        .file(file)
        .unwrap()
        .declarations()
        .definitions()
        .lookup(&query)
        .unwrap();
    let fresh = fresh_db
        .body(sources.snapshot(), &fresh_owner, &Default::default())
        .unwrap()
        .unwrap();
    reused.type_table().assert_same_source_facts(
        reused.definitions(),
        fresh.type_table(),
        fresh.definitions(),
        reused.lowered().module.body.arena(),
        fresh.lowered().module.body.arena(),
    );
}
