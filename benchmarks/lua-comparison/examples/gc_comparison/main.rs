//! Matched graph algorithms with explicit collection and automatic-GC batch latency.
mod automatic;
mod engines;
#[cfg(feature = "diagnostics")]
mod memory;
mod record;
#[cfg(not(feature = "diagnostics"))]
mod timing;

use engines::{Engine, KagariFactory, LuaEngine};
use record::Record;
use std::env;

#[cfg(feature = "diagnostics")]
use kagari_runtime::diagnostics::allocations::Allocator;
#[cfg(feature = "diagnostics")]
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

#[derive(Clone, Copy)]
pub struct Case {
    name: &'static str,
    entry: &'static str,
    check: &'static str,
    keep: &'static str,
    count: i32,
    per_item: usize,
}

const CASES: [Case; 3] = [
    Case {
        name: "points_512",
        entry: "points",
        check: "check_points",
        keep: "keep_points",
        count: 512,
        per_item: 1,
    },
    Case {
        name: "points_8192",
        entry: "points",
        check: "check_points",
        keep: "keep_points",
        count: 8192,
        per_item: 1,
    },
    Case {
        name: "cycles_4096",
        entry: "nodes",
        check: "check_nodes",
        keep: "keep_nodes",
        count: 4096,
        per_item: 2,
    },
];

fn assert_live(engine: &impl Engine, baseline: Option<usize>, extra: usize) {
    if let Some(baseline) = baseline {
        assert_eq!(engine.live_objects(), Some(baseline + extra));
    }
}

fn lifecycle<E: Engine>(mut engine: E, case: Case) -> Record {
    engine.collect();
    let baseline = engine.live_objects();
    let mut record = Record::new();
    let values = record.phase(0, || engine.construct(case.count));
    record.phase(1, || engine.collect());
    assert_live(&engine, baseline, case.count as usize * case.per_item + 1);
    engine.check(&values, 1, case.count);
    let kept = record.phase(2, || {
        let kept = engine.keep(&values);
        drop(values);
        kept
    });
    record.phase(3, || engine.collect());
    let count = (case.count + 63) / 64;
    assert_live(&engine, baseline, count as usize * case.per_item + 1);
    engine.check(&kept, 64, count);
    drop(kept);
    record.phase(4, || engine.collect());
    assert_live(&engine, baseline, 0);
    record
}

fn main() {
    let arguments: Vec<_> = env::args().skip(1).collect();
    assert!(
        arguments
            .iter()
            .all(|arg| arg == "--check" || arg == "--reverse")
    );
    let check = arguments.iter().any(|arg| arg == "--check");
    let reverse = arguments.iter().any(|arg| arg == "--reverse");
    let factory = KagariFactory::new(false);
    let automatic = KagariFactory::new(true);
    if check {
        for case in [CASES[0], CASES[2]] {
            for count in [0, 1] {
                let case = Case { count, ..case };
                lifecycle(factory.create(case), case);
                lifecycle(LuaEngine::new(case, false), case);
            }
        }
    }
    #[cfg(not(feature = "diagnostics"))]
    {
        timing::run(&factory, &automatic, check, reverse);
    }
    #[cfg(feature = "diagnostics")]
    memory::run(&factory, &automatic, check, reverse);
}
