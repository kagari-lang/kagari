use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_common::{cancellation::*, identity::table::*};
use kagari_hir::*;
use kagari_hir::{
    aggregates::implementations::{
        ImplementationSearchContext, ImplementationSearchError, ImplementationSearchLimits,
    },
    declarations::DeclarationId,
    resolver::resolved::ResolvedName,
    typeck::table::CallTarget,
};
use kagari_hir::{analysis::*, types::*};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

const SOURCE: &str = "trait Get { fn get(self) -> i32; } struct Point { val n: i32 } impl Get for Point { fn get(self) -> i32 { self.n } } fn read<T: Get>(p: T) -> i32 { p.get() } fn main() -> i32 { read(Point { n: 7 }) }";

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

fn targets(facts: &AnalyzedModule) -> (DefinitionId, DefinitionId, TypeId) {
    let definition = *facts
        .declarations
        .definition(ResolvedName::Trait(facts.lowered.module.traits[0].id))
        .unwrap();
    let method = *facts
        .declarations
        .definition(ResolvedName::Function(
            facts.lowered.module.traits[0].methods[0].function,
        ))
        .unwrap();
    let point = TypeId::Struct(kagari_hir::types::NominalType {
        associated_types: Default::default(),
        declaration: facts
            .aggregates
            .structures()
            .find(|s| s.declaration.name == "Point")
            .unwrap()
            .id,
        arguments: Vec::new(),
    });
    (definition, method, point)
}

fn interface(id: &DefinitionId) -> kagari_hir::types::NominalType {
    kagari_hir::types::NominalType {
        associated_types: Default::default(),
        declaration: *id,
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
    let snapshot = analyze(&mut test_analysis(), &sources);
    let a = snapshot.file(left).unwrap();
    let b = snapshot.file(right).unwrap();
    for file in [a, b] {
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
    }
    let authoring_fa = a.to_unverified(&Default::default()).unwrap();
    let fa = authoring_fa.records().facts();
    let authoring_fb = b.to_unverified(&Default::default()).unwrap();
    let fb = authoring_fb.records().facts();
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
    let assumptions = Default::default();
    let cancel = Default::default();
    let search = ImplementationSearchContext {
        assumptions: &assumptions,
        limits: ImplementationSearchLimits {
            max_checks: 4096,
            max_depth: 64,
        },
        cancel: &cancel,
    };
    for (facts, own_trait, own_method, point, foreign_trait, foreign_method) in
        [(fa, &ta, &ma, &pa, &tb, &mb), (fb, &tb, &mb, &pb, &ta, &ma)]
    {
        let definitions = facts.declarations.definitions();
        let table = &facts.typed.type_table();
        assert_eq!(
            facts.aggregates.implementation_count_bounded(
                definitions,
                &interface(own_trait),
                point,
                search,
            ),
            Ok(1)
        );
        assert_eq!(
            facts.aggregates.implementation_count_bounded(
                definitions,
                &interface(foreign_trait),
                point,
                search,
            ),
            Ok(0)
        );
        assert!(
            table
                .implementation_method(
                    &facts.aggregates,
                    definitions,
                    own_method,
                    &interface(own_trait),
                    point,
                    search,
                )
                .unwrap()
                .is_some()
        );
        assert!(
            table
                .implementation_method(
                    &facts.aggregates,
                    definitions,
                    foreign_method,
                    &interface(foreign_trait),
                    point,
                    search,
                )
                .unwrap()
                .is_none()
        );
        let cancelled = CancellationToken::default();
        cancelled.cancel();
        assert_eq!(
            table.implementation_method(
                &facts.aggregates,
                definitions,
                foreign_method,
                &interface(foreign_trait),
                point,
                ImplementationSearchContext {
                    cancel: &cancelled,
                    ..search
                },
            ),
            Err(ImplementationSearchError::Cancelled)
        );
        assert_eq!(
            table.implementation_method(
                &facts.aggregates,
                definitions,
                own_method,
                &interface(own_trait),
                point,
                ImplementationSearchContext {
                    limits: ImplementationSearchLimits {
                        max_checks: 0,
                        max_depth: 64,
                    },
                    ..search
                },
            ),
            Err(ImplementationSearchError::LimitExceeded)
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
            Some(interface(own_trait))
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
        a.queries().definition_at(offset).unwrap().id,
        DeclarationId::Definition(ma)
    );
    assert_eq!(
        b.queries().definition_at(offset).unwrap().id,
        DeclarationId::Definition(mb)
    );
}

#[test]
fn declaration_reordering_preserves_trait_and_method_identities() {
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("reorder.kgr", SOURCE.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    let authoring_before = old
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let before = authoring_before.records().facts();
    let (trait_id, method_id, point) = targets(before);
    let assumptions = Default::default();
    let cancel = Default::default();
    let search = ImplementationSearchContext {
        assumptions: &assumptions,
        limits: ImplementationSearchLimits {
            max_checks: 4096,
            max_depth: 64,
        },
        cancel: &cancel,
    };
    let previous_impl = before
        .typed
        .type_table()
        .implementation_method(
            &before.aggregates,
            before.declarations.definitions(),
            &method_id,
            &interface(&trait_id),
            &point,
            search,
        )
        .unwrap()
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
    let authoring_facts = after.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.records().facts();
    let definitions = facts.declarations.definitions();
    let table = &facts.typed.type_table();
    assert_eq!(
        facts.aggregates.implementation_count_bounded(
            definitions,
            &interface(&trait_id),
            &point,
            search,
        ),
        Ok(1)
    );
    let next_impl = table
        .implementation_method(
            &facts.aggregates,
            definitions,
            &method_id,
            &interface(&trait_id),
            &point,
            search,
        )
        .unwrap()
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
        Some(interface(&trait_id))
    );
    assert_eq!(
        after
            .queries()
            .definition_at(edited.find("p.get").unwrap() + 2)
            .unwrap()
            .id,
        DeclarationId::Definition(method_id)
    );
    assert_eq!(
        old.file(file)
            .unwrap()
            .queries()
            .definition_at(SOURCE.find("p.get").unwrap() + 2)
            .unwrap()
            .id,
        DeclarationId::Definition(method_id)
    );
    assert_eq!(
        before.typed.type_table().implementation_method(
            &before.aggregates,
            before.declarations.definitions(),
            &targets(before).1,
            &interface(&trait_id),
            &point,
            search,
        ),
        Ok(Some(previous_impl))
    );
}
