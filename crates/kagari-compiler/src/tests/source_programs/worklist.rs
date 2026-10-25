//! Bounded scheduling measurements and post-migration no-regeneration contracts.
//!
//! Run measurements with `--ignored --nocapture --test-threads=1`. Each sample
//! builds fresh deterministic sources and checked inputs; no compiler-output cache
//! is reused. Source checking and lowering have separate clocks. History creation,
//! portable encoding and Rust build time are excluded from the lowering clock.
//! Counts include test-only hook overhead. These are observations, not speed gates.

use super::{checked, insert, update_demands::history};
use crate::source::{
    lower::instances::MirLoweringOptions,
    program::{lower_program_to_mir, lower_program_to_mir_with_demands},
    work_metrics::{self, Snapshot},
};
use kagari_mir::{codec::encode_program, program::VerifiedMirProgram};
use kagari_source::{identity::FileId, source_database::SourceDatabase};
use std::{
    collections::HashSet,
    fmt::Write,
    time::{Duration, Instant},
};

const SIZES: [usize; 3] = [2, 4, 8];
const REPETITIONS: usize = 3;

#[derive(Clone, Copy, Debug)]
enum Shape {
    Tiny,
    Chain(usize),
    Fanout(usize),
}

struct Fixture {
    sources: SourceDatabase,
    root: FileId,
    replacement: String,
    expected_bodies: usize,
}

fn fixture(shape: Shape) -> Fixture {
    let mut sources = SourceDatabase::default();
    let mut imports = String::new();
    let (entry, expected_bodies) = match shape {
        Shape::Tiny => ("7".to_owned(), 1),
        Shape::Chain(size) => {
            assert!((1..=8).contains(&size));
            for index in 0..size {
                let text = if index + 1 == size {
                    "pub fn pass<T>(value: T) -> T { value }".to_owned()
                } else {
                    format!(
                        "use pkg::chain{}; pub fn pass<T>(value: T) -> T {{ chain{}::pass(value) }}",
                        index + 1,
                        index + 1,
                    )
                };
                insert(&mut sources, &format!("chain{index}"), &text);
            }
            imports.push_str("use pkg::chain0; ");
            ("chain0::pass(7)".to_owned(), size + 1)
        }
        Shape::Fanout(size) => {
            assert!((1..=8).contains(&size));
            insert(
                &mut sources,
                "leaf",
                "pub fn pass<T>(value: T) -> T { value }",
            );
            let mut calls = Vec::new();
            for index in 0..size {
                insert(
                    &mut sources,
                    &format!("branch{index}"),
                    "use pkg::leaf; pub fn pass<T>(value: T) -> T { leaf::pass(value) }",
                );
                write!(imports, "use pkg::branch{index}; ").unwrap();
                calls.push(format!("branch{index}::pass(7)"));
            }
            (calls.join(" + "), size + 2)
        }
    };
    let root = insert(
        &mut sources,
        "root",
        &format!("{imports}fn main() -> i32 {{ {entry} }}"),
    );
    Fixture {
        sources,
        root,
        // Imports preserve the checked dependency closure; only demands disappear.
        replacement: format!("{imports}fn main() -> i32 {{ 0 }}"),
        expected_bodies,
    }
}

struct Measurement {
    program: VerifiedMirProgram,
    counts: Snapshot,
    checking: Duration,
    lowering: Duration,
}

fn measure(mut fixture: Fixture, retained: bool) -> Measurement {
    let options = MirLoweringOptions::default();
    let prior = retained.then(|| {
        let original =
            lower_program_to_mir(&checked(&fixture.sources, fixture.root), &options).unwrap();
        let prior = history(&original);
        insert(&mut fixture.sources, "root", &fixture.replacement);
        prior
    });
    let start = Instant::now();
    let source = checked(&fixture.sources, fixture.root);
    let checking = start.elapsed();
    work_metrics::reset();
    let start = Instant::now();
    let program = match prior {
        Some((demands, headers)) => {
            lower_program_to_mir_with_demands(&source, &options, &demands, &headers)
        }
        None => lower_program_to_mir(&source, &options),
    }
    .unwrap();
    let lowering = start.elapsed();
    let counts = work_metrics::snapshot();
    assert_eq!(body_count(&program), fixture.expected_bodies);
    assert_eq!(unique_bodies(&program), fixture.expected_bodies);
    assert!(
        counts.initializations > 0,
        "module initialization hook missing"
    );
    assert!(counts.bodies > 0, "body generation hook missing");
    assert!(counts.candidates > 0, "candidate verification hook missing");
    assert!(counts.rounds > 0, "scheduling pass hook missing");
    Measurement {
        program,
        counts,
        checking,
        lowering,
    }
}

fn body_count(program: &VerifiedMirProgram) -> usize {
    program.modules().iter().map(|m| m.functions.len()).sum()
}

fn unique_bodies(program: &VerifiedMirProgram) -> usize {
    // Output placement matters: a foreign authored body can belong to a distinct
    // output module. Local vector positions are not semantic instance identities.
    program
        .modules()
        .iter()
        .flat_map(|module| {
            module
                .functions
                .iter()
                .map(|function| (&module.identity, &function.instance))
        })
        .collect::<HashSet<_>>()
        .len()
}

fn assert_no_regeneration(measurement: &Measurement) {
    assert_eq!(
        measurement.counts.initializations,
        measurement.program.modules().len(),
        "initialize each authoring module once",
    );
    assert_eq!(
        measurement.counts.bodies,
        unique_bodies(&measurement.program),
        "generate each emitted body once, including generated descendants",
    );
    // Coverage expansion may legitimately require another candidate verification.
    // Neither candidate nor scheduler-pass counts are an exactly-once contract.
}

#[test]
#[ignore = "bounded measurement; run explicitly with --ignored --nocapture --test-threads=1"]
fn bounded_worklist_measurements() {
    println!(
        "shape,retained,repetition,check_us,lower_us,initializations,body_generations,candidate_verifications,scheduling_passes,modules,unique_bodies,instructions,mir_bytes"
    );
    for shape in [Shape::Tiny]
        .into_iter()
        .chain(SIZES.map(Shape::Chain))
        .chain(SIZES.map(Shape::Fanout))
    {
        for retained in [false, true] {
            if retained && matches!(shape, Shape::Tiny) {
                continue;
            }
            for repetition in 0..REPETITIONS {
                let measurement = measure(fixture(shape), retained);
                let program = &measurement.program;
                let instructions: usize = program
                    .modules()
                    .iter()
                    .flat_map(|module| &module.functions)
                    .flat_map(|function| &function.blocks)
                    .map(|block| block.instructions.len() + usize::from(block.terminator.is_some()))
                    .sum();
                let bytes = encode_program(program, &Default::default()).unwrap();
                println!(
                    "{shape:?},{retained},{repetition},{},{},{},{},{},{},{},{},{instructions},{}",
                    measurement.checking.as_micros(),
                    measurement.lowering.as_micros(),
                    measurement.counts.initializations,
                    measurement.counts.bodies,
                    measurement.counts.candidates,
                    measurement.counts.rounds,
                    program.modules().len(),
                    unique_bodies(program),
                    bytes.len(),
                );
            }
        }
    }
}

#[test]
#[ignore = "R7-04: enable after replacing the whole-module regeneration driver"]
fn worklist_chain_and_converging_demands_do_not_regenerate_bodies() {
    for shape in [Shape::Tiny, Shape::Chain(4), Shape::Fanout(4)] {
        assert_no_regeneration(&measure(fixture(shape), false));
    }
}

#[test]
#[ignore = "R7-04: enable after replacing the whole-module regeneration driver"]
fn worklist_retained_demands_do_not_regenerate_bodies() {
    for shape in [Shape::Chain(4), Shape::Fanout(4)] {
        assert_no_regeneration(&measure(fixture(shape), true));
    }
}

#[test]
#[ignore = "R7-04: enable after replacing the whole-module regeneration driver"]
fn worklist_recursive_dependency_reuses_the_admitted_body() {
    let mut sources = SourceDatabase::default();
    insert(
        &mut sources,
        "recursive",
        "pub fn repeat<T>(value: T, n: i32) -> T { if n == 0 { value } else { repeat(value, n - 1) } }",
    );
    let root = insert(
        &mut sources,
        "root",
        "use pkg::recursive; fn main() -> i32 { recursive::repeat(7, 3) }",
    );
    assert_no_regeneration(&measure(
        Fixture {
            sources,
            root,
            replacement: String::new(),
            expected_bodies: 2,
        },
        false,
    ));
}
