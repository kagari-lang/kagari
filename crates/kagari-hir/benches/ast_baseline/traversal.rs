//! Diagnostic traversal comparisons over retained parses, separate from parsing and field reads.

use std::{hint::black_box, time::Instant};

use kagari_syntax::ast::{module::ModuleSyntax, visit::NodeRef};
use serde_json::{Value, json};

use crate::{allocation, parse_source, workloads::Workload};

#[derive(Clone, Copy, Debug)]
enum Mode {
    PhysicalIterator,
    PhysicalVisitor,
    OwnedIterator,
    OwnedVisitor,
}

fn visit(modules: &[ModuleSyntax], mode: Mode) -> (usize, u64) {
    let mut count = 0;
    let mut checksum = 0_u64;
    let mut record = |node: NodeRef<'_>| {
        let span = node.body_span();
        count += 1;
        checksum = checksum
            .wrapping_mul(31)
            .wrapping_add(node.kind() as u64)
            .wrapping_mul(31)
            .wrapping_add(span.start as u64)
            .wrapping_mul(31)
            .wrapping_add(span.end as u64);
        true
    };
    for module in modules {
        match mode {
            Mode::PhysicalIterator => {
                for node in NodeRef::ModuleBody(module.body()).descendants() {
                    record(node);
                }
            }
            Mode::PhysicalVisitor => NodeRef::ModuleBody(module.body()).visit(&mut record),
            Mode::OwnedIterator => {
                for node in module.descendants() {
                    record(node);
                }
            }
            Mode::OwnedVisitor => module.visit_owned(&mut record),
        }
    }
    (count, checksum)
}

fn batch(modules: &[ModuleSyntax], mode: Mode, iterations: usize) -> f64 {
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(visit(black_box(modules), mode));
    }
    start.elapsed().as_nanos() as f64 / iterations as f64
}

pub fn report(workloads: &[Workload]) -> Value {
    let mut rows = Vec::new();
    for workload in workloads {
        let modules = workload
            .sources
            .iter()
            .map(|source| parse_source(workload, source).module())
            .collect::<Vec<_>>();
        let physical = visit(&modules, Mode::PhysicalIterator);
        let owned = visit(&modules, Mode::OwnedIterator);
        assert_eq!(physical, visit(&modules, Mode::PhysicalVisitor));
        assert_eq!(owned, visit(&modules, Mode::OwnedVisitor));
        let mut modes = Vec::new();
        for mode in [
            Mode::PhysicalIterator,
            Mode::PhysicalVisitor,
            Mode::OwnedIterator,
            Mode::OwnedVisitor,
        ] {
            let expected = match mode {
                Mode::PhysicalIterator | Mode::PhysicalVisitor => physical,
                Mode::OwnedIterator | Mode::OwnedVisitor => owned,
            };
            let iterations = (20_000_000.0 / batch(&modules, mode, 1).max(1.0))
                .ceil()
                .clamp(1.0, 100_000.0) as usize;
            for _ in 0..3 {
                black_box(batch(&modules, mode, iterations));
            }
            let samples = (0..11)
                .map(|_| batch(&modules, mode, iterations))
                .collect::<Vec<_>>();
            allocation::begin();
            let actual = visit(&modules, mode);
            let counts = allocation::finish();
            assert_eq!(actual, expected);
            assert_eq!(counts.live_bytes, 0);
            modes.push(
                json!({"mode": format!("{mode:?}"), "iterations": iterations,
                "ns_per_workload": samples, "nodes": actual.0, "checksum": actual.1,
                "allocations": counts}),
            );
        }
        rows.push(json!({"name": workload.name, "bytes": workload.bytes(),
            "blake3": workload.hash(), "modes": modes}));
    }
    json!({"schema": 1, "diagnostic_only": true, "warmups": 3, "samples": 11, "workloads": rows})
}
