//! Matched standalone workloads; excludes VM execution and region admission.

// Reuse the reviewed adapter across example targets instead of duplicating rooting.
#[path = "../root_access/frames.rs"]
mod frames;
mod lifecycle;
mod scalar;

use kagari_gc::error::GcError;
use scalar::Operation;
use std::{env, time::Instant};

const ITERATIONS: usize = 50_000;
const WARMUPS: usize = 3;
const SAMPLES: usize = 11;

#[derive(Clone, Copy)]
enum Engine {
    Kagari,
    Arena,
}

impl Engine {
    fn name(self) -> &'static str {
        match self {
            Self::Kagari => "kagari",
            Self::Arena => "gc_arena",
        }
    }
}

fn emit(process: usize, round: usize, engine: Engine, case: &str, count: usize, ns: u128) {
    let phase = if round < WARMUPS { "warmup" } else { "sample" };
    println!(
        "{process},{phase},{round},{},{case},{count},{ns}",
        engine.name()
    );
}

fn main() -> Result<(), GcError> {
    let prepared = env::args().any(|arg| arg == "--prepared");
    let process: usize = env::args()
        .nth(1)
        .unwrap_or_else(|| "0".into())
        .parse()
        .expect("process index");
    println!("process,phase,sample,engine,case,count,elapsed_ns");
    for round in 0..WARMUPS + SAMPLES {
        // Rotate cases and engine order; every batch also checks its result/reclamation.
        for offset in 0..8 {
            let case = (offset + round + process) % 8;
            let engines = if (round + process + offset).is_multiple_of(2) {
                [Engine::Kagari, Engine::Arena]
            } else {
                [Engine::Arena, Engine::Kagari]
            };
            for engine in engines {
                if case < 5 {
                    let op = Operation::ALL[case];
                    let ns = scalar::measure(engine, op)?;
                    emit(process, round, engine, op.name(), ITERATIONS, ns);
                } else if case == 5 {
                    let ns = lifecycle::copies(engine)?;
                    emit(process, round, engine, "root_copy_idle_32", ITERATIONS, ns);
                } else {
                    let count = if case == 6 { 512 } else { 8192 };
                    for (name, ns) in lifecycle::population(engine, count, prepared)? {
                        emit(process, round, engine, name, count, ns);
                    }
                }
            }
        }
    }
    Ok(())
}

fn elapsed(start: Instant) -> u128 {
    start.elapsed().as_nanos()
}
