//! Every sample owns a fresh runtime; Lua/Kagari order reverses by sample/process.
use crate::automatic::{self, BATCH_SIZE, BATCHES};
use crate::{
    CASES, Case, KagariFactory, LuaEngine, lifecycle,
    record::{PHASES, report},
};

const WARMUPS: usize = 3;
const SAMPLES: usize = 11;
fn explicit(factory: &KagariFactory, case: Case, check: bool, reverse: bool) {
    let mut times = [[[0; SAMPLES]; 5]; 2];
    let warmups = if check { 0 } else { WARMUPS };
    let count = if check { 1 } else { SAMPLES };
    for repetition in 0..warmups + count {
        for offset in 0..2 {
            let engine = (offset + repetition + usize::from(reverse)) % 2;
            let record = if engine == 0 {
                lifecycle(factory.create(case), case)
            } else {
                lifecycle(LuaEngine::new(case, false), case)
            };
            if repetition >= warmups {
                for (phase, time) in record.times.into_iter().enumerate() {
                    times[engine][phase][repetition - warmups] = time;
                }
            }
        }
    }
    for (engine, phases) in ["kagari", "lua54"].into_iter().zip(times) {
        for (phase, samples) in PHASES.into_iter().zip(phases) {
            report(engine, case.name, phase, case.count, &samples[..count]);
        }
    }
}

pub fn run(factory: &KagariFactory, automatic_factory: &KagariFactory, check: bool, reverse: bool) {
    for case in CASES {
        explicit(factory, case, check, reverse);
    }
    // Both representations retain eight equally sized graphs and replace one each call.
    for case in [CASES[0], CASES[2]] {
        let mut times = [[[0; BATCHES]; SAMPLES]; 2];
        let warmups = if check { 0 } else { WARMUPS };
        let count = if check { 1 } else { SAMPLES };
        for repetition in 0..warmups + count {
            for offset in 0..2 {
                let engine = (offset + repetition + usize::from(reverse)) % 2;
                let samples = if engine == 0 {
                    automatic::run(automatic_factory.create(case)).times
                } else {
                    automatic::run(LuaEngine::new(case, true)).times
                };
                if repetition >= warmups {
                    times[engine][repetition - warmups] = samples;
                }
            }
        }
        for (engine, samples) in ["kagari", "lua54"].into_iter().zip(times) {
            let batches: Vec<_> = samples[..count].iter().flatten().copied().collect();
            let totals: Vec<_> = samples[..count]
                .iter()
                .map(|batch| batch.iter().sum())
                .collect();
            report(engine, case.entry, "auto_batch", BATCH_SIZE, &batches);
            report(
                engine,
                case.entry,
                "auto_total",
                BATCH_SIZE * BATCHES as i32,
                &totals,
            );
        }
    }
}
