//! Shared automatic-GC workload; batch latency and allocation accounting are separate builds.
use crate::{assert_live, engines::Engine};
#[cfg(feature = "diagnostics")]
use kagari_runtime::diagnostics::allocations::{self, Counts};
use std::array;
#[cfg(not(feature = "diagnostics"))]
use std::time::Instant;

pub const BATCHES: usize = 128;
pub const BATCH_SIZE: i32 = 256;

pub struct AutoRecord {
    #[cfg(not(feature = "diagnostics"))]
    pub times: [u128; BATCHES],
    #[cfg(feature = "diagnostics")]
    pub snapshots: [Counts; 4],
}

pub fn run<E: Engine>(mut engine: E) -> AutoRecord {
    engine.collect();
    let baseline = engine.live_objects();
    let collections = engine.collections();
    let mut roots: [Option<E::Root>; 8] = array::from_fn(|_| None);
    #[cfg(not(feature = "diagnostics"))]
    let mut times = [0; BATCHES];
    #[cfg(feature = "diagnostics")]
    let mut snapshots = [Counts::default(); 4];
    #[cfg(feature = "diagnostics")]
    {
        snapshots[0] = allocations::checkpoint().unwrap_or_default();
    }
    for batch in 0..BATCHES {
        #[cfg(not(feature = "diagnostics"))]
        let start = Instant::now();
        let values = engine.construct(BATCH_SIZE);
        let previous = roots[batch % 8].replace(values);
        drop(previous);
        #[cfg(not(feature = "diagnostics"))]
        {
            times[batch] = start.elapsed().as_nanos();
        }
    }
    if let Some(before) = collections {
        assert!(
            engine.collections().unwrap() > before,
            "automatic workload must complete a cycle"
        );
    }
    #[cfg(feature = "diagnostics")]
    {
        snapshots[1] = allocations::checkpoint().unwrap_or_default();
    }
    engine.collect();
    #[cfg(feature = "diagnostics")]
    {
        snapshots[2] = allocations::checkpoint().unwrap_or_default();
    }
    for root in roots.iter().flatten() {
        engine.check(root, 1, BATCH_SIZE);
    }
    drop(roots);
    engine.collect();
    assert_live(&engine, baseline, 0);
    #[cfg(feature = "diagnostics")]
    {
        snapshots[3] = allocations::checkpoint().unwrap_or_default();
    }
    AutoRecord {
        #[cfg(not(feature = "diagnostics"))]
        times,
        #[cfg(feature = "diagnostics")]
        snapshots,
    }
}
