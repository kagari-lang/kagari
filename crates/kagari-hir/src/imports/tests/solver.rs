//! Bounded reproducible solver workloads; source preparation is timed separately.
use crate::{
    analysis::{AnalysisDatabase, error::AnalysisError},
    collections::HashSet,
    imports::{
        DirectiveResolution, LocalName, ModuleGraph, NamespaceId, SourceUnit,
        bindings::LookupOutcome,
        catalog::{LookupContext, LookupResult},
        resolve::{ResolutionInputs, resolve_imports},
        solver::{ImportSolveError, Observation, PendingGlobs, SolverLimits},
        tests::insert,
    },
    lower::lower_module,
    resolver::table::NameTable,
};
use kagari_common::{cancellation::CancellationToken, identity::map::DefinitionContext};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::declaration::names::NameNamespace;
use std::{cell::RefCell, sync::Arc, time::Instant};

#[test]
fn failed_solves_preserve_retained_imports_and_allow_recovery() {
    let mut sources = SourceDatabase::default();
    let seed = insert(&mut sources, "seed", "pub fn value() {}");
    insert(&mut sources, "other", "pub fn value() {}");
    let a = insert(&mut sources, "a", "pub use pkg::seed::*;");
    let b = insert(&mut sources, "b", "pub use pkg::a::value;");
    let old_input = sources.snapshot();
    let mut analysis = AnalysisDatabase::default();
    let old = analysis
        .declarations(old_input.clone(), &Default::default())
        .unwrap();
    sources
        .set(
            "mem://seed",
            "pub fn renamed() {}".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let changed = sources.snapshot();
    for limits in [
        SolverLimits {
            module_visits: 0,
            candidate_work: usize::MAX,
        },
        SolverLimits {
            module_visits: usize::MAX,
            candidate_work: 0,
        },
    ] {
        analysis.import_solver_limits = Some(limits);
        assert!(matches!(
            analysis.declarations(changed.clone(), &Default::default()),
            Err(AnalysisError::Imports(ImportSolveError::Exhausted { .. }))
        ));
    }
    analysis.import_solver_limits = None;
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        analysis.declarations(changed, &cancel),
        Err(AnalysisError::Cancelled)
    ));
    // Each named slot suppresses its local glob. Treating either temporary absence
    // as final can manufacture an order-dependent, self-supporting binding.
    sources
        .set(
            "mem://a",
            "pub use pkg::b::value; pub use pkg::seed::*;".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    sources
        .set(
            "mem://b",
            "pub use pkg::a::value; pub use pkg::other::*;".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    sources
        .set(
            "mem://seed",
            "pub fn value() {}".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    assert!(matches!(
        analysis.declarations(sources.snapshot(), &Default::default()),
        Err(AnalysisError::Imports(
            ImportSolveError::NonConvergent { .. }
        ))
    ));
    let retained = analysis
        .declarations(old_input, &Default::default())
        .unwrap();
    for snapshot in [&old, &retained] {
        let catalog = &snapshot.module_graph().catalog;
        let seed_file = snapshot.file(seed).unwrap();
        assert_eq!(seed_file.source().text(), "pub fn value() {}");
        assert!(snapshot.file(a).unwrap().diagnostics().is_empty());
        let importer = snapshot.file(b).unwrap();
        assert!(importer.diagnostics().is_empty());
        let dependencies = &importer.names().imports.dependencies;
        assert!(
            dependencies
                .iter()
                .any(|module| catalog.identity(*module) == seed_file.source().module_identity())
        );
    }
    sources
        .set(
            "mem://a",
            "pub use pkg::seed::*;".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let recovered = analysis
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(recovered.files().all(|file| file.diagnostics().is_empty()));
}

#[test]
fn pending_glob_only_defers_weak_lookups_for_importers_that_can_see_it() {
    let mut sources = SourceDatabase::default();
    insert(
        &mut sources,
        "left",
        "pub fn clash() {} pub fn fixed() {} pub fn solo() {}",
    );
    insert(&mut sources, "right", "pub fn clash() {}");
    let late = insert(&mut sources, "late", "pub use pkg::left::fixed;");
    let relay = insert(&mut sources, "relay", "pub use pkg::late::*;");
    let facade = insert(
        &mut sources,
        "facade",
        "pub use pkg::left::*; pub use pkg::right::*; pub use pkg::right::clash as fixed; use pkg::relay::*;",
    );
    let input = sources.snapshot();
    let lowered: Vec<_> = input.files().map(|file| lower_module(file)).collect();
    let module = |id| lowered.iter().find(|m| m.source.id() == id).unwrap();
    let cancel = CancellationToken::default();
    let graph = ModuleGraph::build(&lowered, &DefinitionContext::new().unwrap(), &cancel).unwrap();
    let unit = |id| SourceUnit::of(module(id), graph.catalog.definitions());
    let ns = NamespaceId::Module(unit(facade));
    let mut draft = (*graph.catalog).clone();
    // Replay the two glob imports while late's named export is still reserved. A fully
    // solved snapshot cannot distinguish a deferred lookup from one retried later.
    let late_scope = &mut draft
        .namespaces
        .get_mut(&NamespaceId::Module(unit(late)))
        .unwrap()
        .names;
    Arc::make_mut(late_scope)
        .entries
        .get_mut(&LocalName::new("fixed").unwrap())
        .unwrap()
        .values
        .strong[0]
        .resolution = LookupOutcome::Pending;
    let mut base = (*graph.catalog).clone();
    let mut pending_globs = PendingGlobs::default();
    // Both visited modules contain only imports. The relay has no known
    // member to copy yet, but its public glob can still supply new names.
    for id in [relay, facade] {
        let owner = unit(id);
        let ns = NamespaceId::Module(owner);
        base.namespaces.get_mut(&ns).unwrap().names = Arc::new(NameTable::for_unit(owner));
        let (facts, pending) = resolve_imports(
            module(id),
            &ResolutionInputs {
                base: &base,
                catalog: &draft,
                pending_globs: &pending_globs,
                sources: &lowered.iter().collect::<Vec<_>>(),
                cancel: &cancel,
                closed: &HashSet::default(),
                observations: &RefCell::default(),
            },
        )
        .unwrap();
        assert_eq!(
            facts.directives.last().unwrap().resolution,
            DirectiveResolution::Glob(LookupOutcome::Pending)
        );
        assert_eq!(pending.len(), 1);
        pending_globs.insert(ns, pending.into());
        draft.namespaces.get_mut(&ns).unwrap().names = facts.scope;
    }
    let observations = RefCell::default();
    let lookup = |importer, name| {
        draft
            .lookup_member(
                &LookupContext::draft(&unit(importer).module, &observations, &pending_globs),
                &ns,
                name,
                NameNamespace::Value,
                &cancel,
            )
            .unwrap()
    };
    assert_eq!(lookup(facade, "missing"), LookupResult::Unresolved);
    assert_eq!(lookup(late, "missing"), LookupResult::Missing);
    assert_eq!(lookup(facade, "solo"), LookupResult::Unresolved);
    assert!(matches!(lookup(late, "solo"), LookupResult::Found(_)));
    assert_eq!(lookup(facade, "clash"), LookupResult::Unresolved);
    assert!(matches!(lookup(late, "clash"), LookupResult::Ambiguous(_)));
    assert_eq!(
        lookup(facade, "fixed").hit().unwrap().target,
        *graph.imports_for(&unit(facade)).unwrap().directives[2]
            .resolution
            .target()
            .unwrap()
    );
}

#[test]
fn failed_relative_imports_observe_access_to_successful_prefixes() {
    let mut sources = SourceDatabase::default();
    let tree = insert(&mut sources, "tree", "pub mod middle;");
    insert(&mut sources, "tree::middle", "pub mod leaf;");
    let leaf = insert(
        &mut sources,
        "tree::middle::leaf",
        "use super::super::super::super::missing;",
    );
    let input = sources.snapshot();
    let lowered = input
        .files()
        .map(|source| lower_module(source))
        .collect::<Vec<_>>();
    let graph = ModuleGraph::build(
        lowered.iter(),
        &DefinitionContext::new().unwrap(),
        &Default::default(),
    )
    .unwrap();
    let module = lowered
        .iter()
        .find(|module| module.source.id() == leaf)
        .unwrap();
    let parent = lowered
        .iter()
        .find(|module| module.source.id() == tree)
        .unwrap();
    let observations = RefCell::new(HashSet::default());
    let cancel = CancellationToken::default();
    let (facts, pending) = resolve_imports(
        module,
        &ResolutionInputs {
            base: &graph.catalog,
            catalog: &graph.catalog,
            pending_globs: &PendingGlobs::default(),
            sources: &lowered.iter().collect::<Vec<_>>(),
            cancel: &cancel,
            closed: &HashSet::default(),
            observations: &observations,
        },
    )
    .unwrap();
    assert_eq!(
        facts
            .diagnostics
            .iter()
            .map(|diagnostic| &diagnostic.kind)
            .collect::<Vec<_>>(),
        [&DiagnosticKind::UnknownName {
            name: "super::super::super::super::missing".into()
        }],
    );
    assert!(pending.is_empty());
    assert!(facts.directives[0].resolution.targets().next().is_none());
    // The full underflowing lookup reads only InputUniverse. The shorter `super`
    // prefix must still watch the parent's access binding, even without editor hits.
    assert_eq!(
        observations.into_inner(),
        [
            Observation::InputUniverse,
            Observation::Namespace(NamespaceId::Module(SourceUnit::of(
                parent,
                graph.catalog.definitions()
            ))),
        ]
        .into_iter()
        .collect::<HashSet<_>>()
    );
}

#[test]
#[ignore = "explicit import-solver measurement checkpoint"]
fn solver_workload_measurements() {
    for shape in [
        "named-chain",
        "glob-chain",
        "diamond",
        "seeded-cycle",
        "unrelated",
        "removal",
    ] {
        let mut sources = SourceDatabase::default();
        let count = 48;
        for index in 0..count {
            let text = match shape {
                "diamond" if index > 0 => {
                    format!("pub use pkg::m{}::*; pub use pkg::m0::*;", (index - 1) / 2)
                }
                "seeded-cycle" => format!(
                    "{} pub use pkg::m{}::*;",
                    if index == 0 { "pub fn value() {}" } else { "" },
                    (index + 1) % count
                ),
                _ if index == 0 => "pub fn value() {}".into(),
                "glob-chain" => format!("pub use pkg::m{}::*;", index - 1),
                _ => format!("pub use pkg::m{}::value;", index - 1),
            };
            insert(&mut sources, &format!("m{index}"), &text);
        }
        if shape == "unrelated" {
            for index in 0..96 {
                insert(&mut sources, &format!("idle{index}"), "pub fn idle() {}");
            }
        }
        if shape == "removal" {
            sources
                .set("mem://m0", "".into(), SourceLayer::Overlay)
                .unwrap();
        }
        let input = sources.snapshot();
        let prepare_start = Instant::now();
        let lowered = input
            .files()
            .map(|file| lower_module(file))
            .collect::<Vec<_>>();
        let prepare = prepare_start.elapsed();
        let solve_start = Instant::now();
        let graph = ModuleGraph::build(
            &lowered,
            &kagari_common::identity::map::DefinitionContext::new().unwrap(),
            &Default::default(),
        )
        .unwrap();
        let solve = solve_start.elapsed();
        if shape == "removal" {
            assert_eq!(
                graph
                    .modules()
                    .filter(|(_, node)| !node.imports.diagnostics.is_empty())
                    .count(),
                count - 1
            );
        } else {
            assert!(
                graph
                    .modules()
                    .all(|(_, node)| node.imports.diagnostics.is_empty()),
                "{shape}"
            );
        }
        eprintln!(
            "{shape}: modules={} prepare_us={} solve_us={} visits={} changed={} candidates={}",
            lowered.len(),
            prepare.as_micros(),
            solve.as_micros(),
            graph.work.module_visits,
            graph.work.changed_entries,
            graph.work.candidate_work
        );
    }
}
