use super::*;
use crate::{
    declarations::DeclarationId,
    resolver::ResolvedName,
    typeck::{CallTarget, ConstraintTarget},
};
use kagari_common::{
    identity::DefinitionId,
    source_database::{SourceDatabase, SourceLayer},
};

const SOURCE: &str = "trait Get { fn get(self) -> i32; } struct Point { val n: i32 } impl Get for Point { fn get(self) -> i32 { self.n } } fn read<T: Get>(p: T) -> i32 { p.get() } fn main() -> i32 { read(Point { n: 7 }) }";

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap()
}

fn targets(facts: &AnalyzedModule) -> (DefinitionId, DefinitionId, TypeId) {
    let definition = facts
        .declarations
        .definition(ResolvedName::Trait(facts.lowered.module.traits[0].id))
        .unwrap()
        .clone();
    let method = facts
        .declarations
        .definition(ResolvedName::Function(
            facts.lowered.module.traits[0].methods[0].function,
        ))
        .unwrap()
        .clone();
    let point = TypeId::Struct(crate::types::NominalType {
        associated_types: Default::default(),
        declaration: facts
            .aggregates
            .structures()
            .find(|s| s.declaration.name == "Point")
            .unwrap()
            .id
            .clone(),
        arguments: Vec::new(),
    });
    (definition, method, point)
}

fn interface(id: &DefinitionId) -> crate::types::NominalType {
    crate::types::NominalType {
        associated_types: Default::default(),
        declaration: id.clone(),
        arguments: Vec::new(),
    }
}

#[test]
fn matching_local_slots_from_other_modules_never_match_trait_contracts() {
    let mut sources = SourceDatabase::default();
    let left = sources
        .set("left.kgr", SOURCE.into(), SourceLayer::Base)
        .unwrap();
    let right = sources
        .set("right.kgr", SOURCE.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
    let a = snapshot.file(left).unwrap();
    let b = snapshot.file(right).unwrap();
    for file in [a, b] {
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
    }
    let fa = a.result().facts();
    let fb = b.result().facts();
    assert_eq!(
        fa.lowered.module.traits[0].id,
        fb.lowered.module.traits[0].id
    );
    assert_eq!(
        fa.lowered.module.traits[0].methods[0].function,
        fb.lowered.module.traits[0].methods[0].function
    );
    let (ta, ma, pa) = targets(fa);
    let (tb, mb, pb) = targets(fb);
    assert_ne!(ta, tb);
    assert_ne!(ma, mb);
    for (facts, own_trait, own_method, point, foreign_trait, foreign_method) in
        [(fa, &ta, &ma, &pa, &tb, &mb), (fb, &tb, &mb, &pb, &ta, &ma)]
    {
        let table = &facts.typed.type_table;
        assert!(table.implements(&interface(own_trait), point));
        assert!(!table.implements(&interface(foreign_trait), point));
        assert!(
            table
                .implementation_method(own_method, &interface(own_trait), point)
                .is_some()
        );
        assert!(
            table
                .implementation_method(foreign_method, &interface(foreign_trait), point)
                .is_none()
        );
        let read = facts
            .lowered
            .module
            .functions
            .iter()
            .find(|f| f.name == "read")
            .unwrap();
        assert_eq!(
            table.constraint(read.generic_params[0].bounds[0].ty),
            Some(ConstraintTarget::Trait(interface(own_trait)))
        );
        let method = facts
            .lowered
            .module
            .body
            .expressions()
            .find_map(|(id, _)| match table.call_resolution(id)?.target {
                CallTarget::TraitMethod { method, .. } => Some(method),
                _ => None,
            })
            .unwrap();
        assert_eq!(&method, own_method);
    }
    let offset = SOURCE.find("p.get").unwrap() + 2;
    assert_eq!(
        a.definition_at(offset).unwrap().id,
        DeclarationId::Definition(ma)
    );
    assert_eq!(
        b.definition_at(offset).unwrap().id,
        DeclarationId::Definition(mb)
    );
}

#[test]
fn declaration_reordering_preserves_trait_and_method_identities() {
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("reorder.kgr", SOURCE.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let old = analyze(&mut db, &sources);
    let before = old.file(file).unwrap().result().facts();
    let (trait_id, method_id, point) = targets(before);
    let previous_impl = before
        .typed
        .type_table
        .implementation_method(&method_id, &interface(&trait_id), &point)
        .unwrap();
    let edited = format!("trait Earlier {{ fn other(self); }} fn earlier() {{}} {SOURCE}");
    sources
        .set("reorder.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let new = analyze(&mut db, &sources);
    let after = new.file(file).unwrap();
    assert!(
        after.result().diagnostics().is_empty(),
        "{:?}",
        after.result().diagnostics()
    );
    let facts = after.result().facts();
    let table = &facts.typed.type_table;
    assert!(table.implements(&interface(&trait_id), &point));
    let next_impl = table
        .implementation_method(&method_id, &interface(&trait_id), &point)
        .unwrap();
    assert_ne!(previous_impl, next_impl);
    let read = facts
        .lowered
        .module
        .functions
        .iter()
        .find(|f| f.name == "read")
        .unwrap();
    assert_eq!(
        table.constraint(read.generic_params[0].bounds[0].ty),
        Some(ConstraintTarget::Trait(interface(&trait_id)))
    );
    assert_eq!(
        after
            .definition_at(edited.find("p.get").unwrap() + 2)
            .unwrap()
            .id,
        DeclarationId::Definition(method_id.clone())
    );
    assert_eq!(
        old.file(file)
            .unwrap()
            .definition_at(SOURCE.find("p.get").unwrap() + 2)
            .unwrap()
            .id,
        DeclarationId::Definition(method_id)
    );
    assert_eq!(
        before.typed.type_table.implementation_method(
            &targets(before).1,
            &interface(&trait_id),
            &point
        ),
        Some(previous_impl)
    );
}

#[test]
fn cached_body_queries_rebase_receivers_without_changing_nominal_method_targets() {
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("cache.kgr", SOURCE.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let old = analyze(&mut db, &sources);
    let before = old.file(file).unwrap().result().facts();
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
    let fresh = AnalysisDatabase::default()
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    reused.type_table().assert_same_source_facts(
        fresh.type_table(),
        reused.lowered().module.body.arena(),
        fresh.lowered().module.body.arena(),
    );
}
