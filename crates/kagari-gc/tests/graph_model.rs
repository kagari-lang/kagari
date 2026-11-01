mod graph;
mod support;

use std::env;

#[test]
fn graph_operations_match_independent_reachability() {
    if cfg!(miri) {
        graph::run(0x4743, 64);
        return;
    }
    if let Ok(path) = env::var("KAGARI_GC_REPLAY") {
        graph::replay(&path);
        return;
    }
    let operations = env::var("KAGARI_GC_OPERATIONS")
        .map(|value| value.parse().expect("KAGARI_GC_OPERATIONS must be usize"))
        .unwrap_or(1024);
    if let Ok(seed) = env::var("KAGARI_GC_SEED") {
        graph::run(
            seed.parse().expect("KAGARI_GC_SEED must be u64"),
            operations,
        );
    } else {
        for seed in [0, 1, 2, 3, 0x4743, 0xdeadbeef, 0x123456789abcdef0, u64::MAX] {
            graph::run(seed, operations);
        }
    }
}
