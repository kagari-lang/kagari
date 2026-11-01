//! Bounded release characterization, not a frozen comparison or performance gate.

use kagari_gc::{error::GcError, heap::Heap};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

pub(super) fn run() -> Result<(), GcError> {
    println!("characterization only: 3 warmups, 11 samples; no baseline, p99 or pass claim");
    println!(
        "workload,population,construct_ns,live_collect_ns,release_handles_ns,empty_collect_ns,managed_peak_bytes"
    );
    for population in [512usize, 8192] {
        let mut measured = Vec::new();
        for lifecycle in 0..14 {
            let result = sample(population)?;
            if lifecycle >= 3 {
                measured.push(result);
            }
        }
        for column in 0..5 {
            let mut values = measured
                .iter()
                .map(|sample| sample[column])
                .collect::<Vec<_>>();
            values.sort_unstable();
            print!(
                "{}{}",
                if column == 0 {
                    format!("u64_roots,{population},")
                } else {
                    String::new()
                },
                values[values.len() / 2]
            );
            if column != 4 {
                print!(",");
            }
        }
        println!();
        println!("raw_samples_{population}={measured:?}");
    }
    Ok(())
}

fn sample(population: usize) -> Result<[u128; 5], GcError> {
    let mut heap = Heap::default();
    // Warm descriptor registration and reserve the benchmark's handle vector outside
    // construction timing. Each object still receives its actual owning Root lease.
    let descriptor = heap.access()?.register::<u64>()?;
    let mut roots = Vec::with_capacity(population);
    let start = Instant::now();
    {
        let mut access = heap.access()?;
        for value in 0..population {
            roots.push(access.alloc_leaf(black_box(value as u64))?);
        }
    }
    let construction = start.elapsed();
    let start = Instant::now();
    heap.collect()?;
    let live = start.elapsed();
    {
        let mut access = heap.access()?;
        for (expected, root) in roots.iter().enumerate() {
            assert_eq!(
                access.with_object(root, |view| *view.object())?,
                expected as u64
            );
        }
    }
    let peak = heap.stats().peak_managed_bytes;
    let start = Instant::now();
    drop(roots);
    let release = start.elapsed();
    let start = Instant::now();
    heap.collect()?;
    let empty = start.elapsed();
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().root_registrations, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    drop(descriptor);
    Ok([
        ns(construction),
        ns(live),
        ns(release),
        ns(empty),
        peak as u128,
    ])
}

fn ns(duration: Duration) -> u128 {
    duration.as_nanos()
}
