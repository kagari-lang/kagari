//! Bounded cold/warm construction timing. Compilation and destruction are excluded.
use kagari_runtime::Runtime;
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

fn construct() -> Duration {
    let started = Instant::now();
    let runtime = black_box(Runtime::default());
    let elapsed = started.elapsed();
    drop(runtime);
    elapsed
}

fn main() {
    let cold = construct();
    let mut samples: Vec<_> = (0..5).map(|_| construct()).collect();
    samples.sort_unstable();
    println!("cold construction: {:.3} ms", cold.as_secs_f64() * 1_000.0);
    println!(
        "warm construction (5 samples): median {:.3} ms, min {:.3} ms, max {:.3} ms",
        samples[2].as_secs_f64() * 1_000.0,
        samples[0].as_secs_f64() * 1_000.0,
        samples[4].as_secs_f64() * 1_000.0,
    );
}
