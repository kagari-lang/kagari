//! Fixed observation storage keeps measurement bookkeeping out of allocator deltas.
#[cfg(feature = "diagnostics")]
use kagari_runtime::diagnostics::allocations::{self, Counts};
#[cfg(not(feature = "diagnostics"))]
use std::time::Instant;

pub const PHASES: [&str; 5] = [
    "construct",
    "live_gc",
    "sparsify",
    "sparse_gc",
    "release_gc",
];

pub struct Record {
    #[cfg(not(feature = "diagnostics"))]
    pub times: [u128; 5],
    #[cfg(feature = "diagnostics")]
    pub setup: Counts,
    #[cfg(feature = "diagnostics")]
    pub snapshots: [Counts; 5],
}

impl Record {
    pub fn new() -> Self {
        Self {
            #[cfg(not(feature = "diagnostics"))]
            times: [0; 5],
            #[cfg(feature = "diagnostics")]
            setup: allocations::checkpoint().unwrap_or_default(),
            #[cfg(feature = "diagnostics")]
            snapshots: [Counts::default(); 5],
        }
    }

    pub fn phase<T>(&mut self, index: usize, action: impl FnOnce() -> T) -> T {
        #[cfg(not(feature = "diagnostics"))]
        let start = Instant::now();
        let result = action();
        #[cfg(not(feature = "diagnostics"))]
        {
            self.times[index] = start.elapsed().as_nanos();
        }
        #[cfg(feature = "diagnostics")]
        {
            self.snapshots[index] = allocations::checkpoint().unwrap_or_default();
        }
        result
    }
}

#[cfg(not(feature = "diagnostics"))]
pub fn report(engine: &str, name: &str, phase: &str, count: i32, samples: &[u128]) {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    println!(
        "timing,{engine},{name},{phase},{count},{},{},{},{},{}",
        samples.len(),
        sorted[sorted.len() / 2],
        sorted[0],
        sorted[sorted.len() - 1],
        samples
            .iter()
            .map(u128::to_string)
            .collect::<Vec<_>>()
            .join(";")
    );
}
