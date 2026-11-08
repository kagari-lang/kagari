//! Documentation inventory and executable observations use their actual source blocks.
mod execution;
mod inventory;

use crate::tests::documentation::inventory::{Example, Inventory};
use kagari_runtime::{error::RuntimeErrorKind, native::installation::NativeInstallation};
use kagari_source::{source::SourceFile, source_database::SourceSnapshot};
use kagari_vm::error::VmError;
use rstest::rstest;
use std::{
    cmp::Reverse,
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn documentation_inventory_matches_registration_and_schema_examples() {
    let inventory = inventory::collect();
    assert!(inventory.items > 0);
    for example in &inventory.examples {
        if !example.code.contains("fn main(") {
            analyze(&inventory, example, 0);
        }
    }
    println!(
        "{} documented items, {} distinct source examples",
        inventory.items,
        inventory.examples.len()
    );
}

#[test]
fn displayed_string_witnesses_detect_wrong_offsets_and_typed_slice_traps() {
    let inventory = inventory::collect();
    let installation = NativeInstallation::prepare(&crate::modules().unwrap()).unwrap();
    let indices = inventory
        .examples
        .iter()
        .find(|example| example.code.contains("Some((3u64, '🙂'))"))
        .unwrap();
    let wrong = indices
        .code
        .replace("Some((3u64, '🙂'))", "Some((2u64, '🙂'))");
    assert_ne!(wrong, indices.code);
    let source = Arc::new(SourceFile::new("wrong-offset.kgr", wrong));
    let snapshot = inventory
        .baseline
        .clone()
        .snapshot(
            SourceSnapshot::single_file(source.clone()),
            &Default::default(),
        )
        .unwrap();
    assert!(
        snapshot
            .file(source.id())
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    assert!(
        !execution::run(&snapshot, source.id(), &installation, Default::default())
            .unwrap()
            .value
    );

    let slice = inventory
        .examples
        .iter()
        .find(|example| example.code.contains("pub fn invalid_boundary()"))
        .unwrap();
    let source = Arc::new(SourceFile::new("slice-boundary.kgr", &slice.code));
    let snapshot = inventory
        .baseline
        .clone()
        .snapshot(
            SourceSnapshot::single_file(source.clone()),
            &Default::default(),
        )
        .unwrap();
    assert!(
        snapshot
            .file(source.id())
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    let error = execution::run(
        &snapshot,
        source.id(),
        &installation,
        execution::Setup {
            export: Some("invalid_boundary"),
            scope: None,
        },
    )
    .unwrap_err();
    let execution::Failure::Execution(error) = error else {
        panic!("expected execution failure: {error:?}");
    };
    let VmError::RuntimeError(error) = error.cause() else {
        panic!("expected runtime error: {error:?}");
    };
    assert_eq!(error.kind(), RuntimeErrorKind::IndexOutOfBounds);
}

#[rstest]
#[case::strings("strings")]
#[case::vectors("vectors")]
#[case::arrays("arrays")]
#[case::characters("characters")]
#[case::comparison("comparison")]
#[case::conversions("conversions")]
#[case::iteration("iteration")]
#[case::numbers("numbers")]
#[case::operators("operators")]
#[case::carriers("carriers")]
#[case::futures("futures")]
#[case::tasks("tasks")]
#[case::collections("collections")]
fn documentation_examples_execute(#[case] group: &str) {
    let mut inventory = inventory::collect();
    let installation = NativeInstallation::prepare(&crate::modules().unwrap()).unwrap();
    let examples = std::mem::take(&mut inventory.examples);
    let mut failures = Vec::new();
    let started = Instant::now();
    let mut analysis_time = Duration::ZERO;
    let mut compilation_time = Duration::ZERO;
    let mut execution_time = Duration::ZERO;
    let mut slowest = Vec::new();
    let mut count = 0;
    for (index, example) in examples.iter().enumerate() {
        if group_for(example.group()) != group {
            continue;
        }
        if !example.code.contains("fn main(") {
            continue;
        }
        count += 1;
        let analysis_started = Instant::now();
        let source = Arc::new(SourceFile::new(
            format!("documentation-{index}.kgr"),
            &example.code,
        ));
        let snapshot = inventory
            .baseline
            .clone()
            .snapshot(
                SourceSnapshot::single_file(source.clone()),
                &Default::default(),
            )
            .unwrap();
        let analyzed = snapshot.file(source.id()).unwrap().result();
        analysis_time += analysis_started.elapsed();
        if !analyzed.diagnostics().is_empty() {
            failures.push(format!(
                "{}\n{}\n{:?}",
                example.label(),
                example.code,
                analyzed.diagnostics()
            ));
            continue;
        }
        let scope = if example.code.contains("main(closed_scope:") {
            Some(true)
        } else if example.code.contains("main(scope:") {
            Some(false)
        } else {
            None
        };
        match execution::run(
            &snapshot,
            source.id(),
            &installation,
            execution::Setup {
                scope,
                export: None,
            },
        ) {
            Ok(observed) if observed.value => {
                compilation_time += observed.compilation;
                execution_time += observed.execution;
                let origin = &example.origins[0];
                slowest.push((
                    observed.compilation + observed.execution,
                    format!("{}:{} {:?}", origin.file, origin.line, origin.owner),
                ));
            }
            observed => failures.push(format!(
                "{}\n{}\nObserved: {observed:?}",
                example.label(),
                example.code
            )),
        }
    }
    assert!(count > 0, "empty documentation group: {group}");
    println!(
        "{group}: {count} examples, analysis {analysis_time:?}, compilation/load {compilation_time:?}, execution/cleanup {execution_time:?}, total {:?}",
        started.elapsed()
    );
    slowest.sort_by_key(|entry| Reverse(entry.0));
    for (duration, owner) in slowest.iter().take(3) {
        println!("slow {duration:?}: {owner}");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

fn analyze(inventory: &Inventory, example: &Example, index: usize) {
    let source = Arc::new(SourceFile::new(
        format!("schema-{index}.kgr"),
        &example.code,
    ));
    let snapshot = inventory
        .baseline
        .clone()
        .snapshot(
            SourceSnapshot::single_file(source.clone()),
            &Default::default(),
        )
        .unwrap();
    let diagnostics = snapshot.file(source.id()).unwrap().result().diagnostics();
    assert!(
        diagnostics.is_empty(),
        "{}: {diagnostics:?}",
        example.label()
    );
}

fn group_for(file: &str) -> &'static str {
    match file {
        "alloc/string.kgr" => "strings",
        "alloc/vec.kgr" => "vectors",
        "core/array.kgr" => "arrays",
        "core/char.kgr" => "characters",
        "core/cmp.kgr" | "core/fmt.kgr" | "core/hash.kgr" => "comparison",
        "core/convert.kgr" | "core/default.kgr" | "core/str.kgr" => "conversions",
        "core/iter.kgr" => "iteration",
        "core/num.kgr" => "numbers",
        "core/ops.kgr" => "operators",
        "core/option.kgr" | "core/result.kgr" => "carriers",
        "core/future.kgr" => "futures",
        "core/task.kgr" => "tasks",
        "core/host.kgr" => "host schemas",
        file if file.starts_with("std/") => "collections",
        _ => panic!("unclassified documentation owner: {file}"),
    }
}
