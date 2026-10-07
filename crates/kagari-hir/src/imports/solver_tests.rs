//! Bounded reproducible solver workloads; source preparation is timed separately.
use crate::{
    analysis::{AnalysisDatabase, error::AnalysisError},
    host::HostDeclarations,
    imports::{
        ModuleGraph,
        solver::{ImportSolveError, SolverLimits},
        tests::insert,
    },
    lower::lower_module,
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::{sync::Arc, time::Instant};

#[test]
fn failed_solves_preserve_published_snapshots_and_allow_recovery() {
    let mut sources = SourceDatabase::default();
    let seed = insert(&mut sources, "seed", "pub fn value() {}");
    insert(&mut sources, "other", "pub fn value() {}");
    let a = insert(&mut sources, "a", "pub use pkg::seed::*;");
    insert(&mut sources, "b", "pub use pkg::a::value;");
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
    assert!(Arc::ptr_eq(
        old.file(seed).unwrap(),
        retained.file(seed).unwrap()
    ));
    assert!(Arc::ptr_eq(old.file(a).unwrap(), retained.file(a).unwrap()));
    assert_eq!(old.file(seed).unwrap().source().text(), "pub fn value() {}");
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
        let graph =
            ModuleGraph::build(&lowered, &HostDeclarations::empty(), &Default::default()).unwrap();
        if shape != "removal" {
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
            solve_start.elapsed().as_micros(),
            graph.work.module_visits,
            graph.work.changed_entries,
            graph.work.candidate_work
        );
    }
}
