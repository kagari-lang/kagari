//! Same global allocator covers mlua's Rust allocator and Kagari's runtime backing.
use crate::{CASES, KagariFactory, LuaEngine, automatic, lifecycle, record::PHASES};
use kagari_runtime::diagnostics::allocations;

pub fn run(factory: &KagariFactory, automatic_factory: &KagariFactory, check: bool, reverse: bool) {
    for case in CASES {
        // Prepared Kagari products outlive Runtime; warm their shared caches first.
        lifecycle(factory.create(case), case);
        lifecycle(LuaEngine::new(case, false), case);
        for repetition in 0..if check { 1 } else { 3 } {
            for offset in 0..2 {
                let kagari = (offset + repetition + usize::from(reverse)) % 2 == 0;
                let name = if kagari { "kagari" } else { "lua54" };
                let (record, total) = allocations::measure(|| {
                    if kagari {
                        lifecycle(factory.create(case), case)
                    } else {
                        lifecycle(LuaEngine::new(case, false), case)
                    }
                });
                assert_eq!(
                    total.net_bytes, 0,
                    "complete state/runtime teardown: {name}"
                );
                let setup = record.setup.net_bytes;
                let post_peak = record
                    .snapshots
                    .iter()
                    .map(|c| c.interval_peak_net_bytes)
                    .max()
                    .unwrap();
                println!("memory,{name},{},{repetition},setup,{setup}", case.name);
                for (phase, counts) in PHASES.into_iter().zip(record.snapshots) {
                    println!(
                        "memory,{name},{},{repetition},{phase},{}",
                        case.name,
                        counts.net_bytes - setup
                    );
                }
                println!(
                    "memory,{name},{},{repetition},post_setup_peak,{}",
                    case.name,
                    post_peak - setup
                );
                println!(
                    "memory,{name},{},{repetition},lifecycle_peak,{}",
                    case.name, total.peak_net_bytes
                );
                println!(
                    "memory,{name},{},{repetition},teardown,{}",
                    case.name, total.net_bytes
                );
            }
        }
    }
    for case in [CASES[0], CASES[2]] {
        automatic::run(automatic_factory.create(case));
        automatic::run(LuaEngine::new(case, true));
        for repetition in 0..if check { 1 } else { 3 } {
            for offset in 0..2 {
                let kagari = (offset + repetition + usize::from(reverse)) % 2 == 0;
                let name = if kagari { "kagari" } else { "lua54" };
                let (record, total) = allocations::measure(|| {
                    if kagari {
                        automatic::run(automatic_factory.create(case))
                    } else {
                        automatic::run(LuaEngine::new(case, true))
                    }
                });
                assert_eq!(total.net_bytes, 0, "automatic runtime teardown: {name}");
                let setup = record.snapshots[0].net_bytes;
                for (phase, counts) in ["setup", "after_batches", "live", "empty"]
                    .into_iter()
                    .zip(record.snapshots)
                {
                    let bytes = if phase == "setup" {
                        counts.net_bytes
                    } else {
                        counts.net_bytes - setup
                    };
                    println!(
                        "memory,{name},auto_{},{repetition},{phase},{bytes}",
                        case.entry
                    );
                }
                let post_peak = record.snapshots[1..]
                    .iter()
                    .map(|c| c.interval_peak_net_bytes)
                    .max()
                    .unwrap();
                println!(
                    "memory,{name},auto_{},{repetition},post_setup_peak,{}",
                    case.entry,
                    post_peak - setup
                );
                println!(
                    "memory,{name},auto_{},{repetition},lifecycle_peak,{}",
                    case.entry, total.peak_net_bytes
                );
                println!(
                    "memory,{name},auto_{},{repetition},teardown,{}",
                    case.entry, total.net_bytes
                );
            }
        }
    }
}
