//! Owned roots parked at an instruction boundary; setup and retirement stay separate.
use crate::Options;
use kagari::{
    context::ExecutionContext,
    engine::KagariEngine,
    error::{EmbeddingError, RuntimeFailureKind},
    program::PreparedProgram,
    runtime::{KagariRuntime, owned::DriveResult},
};
use kagari_runtime::{module::LoadedModule, session::owned::OwnedExecution, value::Value};
use kagari_source::source::SourceFile;
use std::{hint::black_box, num::NonZeroUsize, time::Instant};

#[cfg(feature = "diagnostics")]
use kagari_runtime::diagnostics::{self, allocations};

const SOURCE: &str = r#"
pub fn hold(n: i32) -> i32 {
    var sum = 0; var i = 0;
    while i < 8 { sum += n; i += 1; }
    sum
}
"#;

const CASES: [(usize, &str); 4] = [(1, "tail"), (32, "tail"), (256, "forward"), (256, "random")];

struct Fixture {
    runtime: KagariRuntime,
    module: LoadedModule,
    context: ExecutionContext,
}

impl Fixture {
    fn new(engine: &KagariEngine, program: &PreparedProgram) -> Self {
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let module = runtime.load_program(program, Default::default()).unwrap();
        Self {
            runtime,
            module,
            context,
        }
    }

    fn park(&self, roots: &mut Vec<Option<OwnedExecution>>, population: usize) {
        assert!(roots.is_empty());
        for index in 0..population {
            let execution = self
                .runtime
                .start(
                    &self.module,
                    "hold",
                    &[Value::I32(index as i32 + 1)],
                    &self.context,
                )
                .unwrap();
            assert!(matches!(
                self.runtime
                    .drive(&execution, NonZeroUsize::new(1).unwrap())
                    .unwrap(),
                DriveResult::Runnable
            ));
            roots.push(Some(execution));
        }
    }

    fn retire(
        &self,
        roots: &mut Vec<Option<OwnedExecution>>,
        order: &[usize],
        cancel: bool,
    ) -> i64 {
        let mut checksum = 0;
        for &index in order {
            let execution = roots[index].take().unwrap();
            if cancel {
                execution.cancel();
            }
            let DriveResult::Complete(result) = self
                .runtime
                .drive(&execution, NonZeroUsize::new(10_000).unwrap())
                .unwrap()
            else {
                panic!("bounded control must complete");
            };
            if cancel {
                assert!(matches!(
                    result,
                    Err(EmbeddingError::Runtime {
                        kind: RuntimeFailureKind::Cancelled,
                        ..
                    })
                ));
                checksum += 1;
            } else {
                let value = result.unwrap().value(self.runtime.runtime().gc()).unwrap();
                assert_eq!(value, Value::I32((index as i32 + 1) * 8));
                checksum += i64::from((index as i32 + 1) * 8);
            }
        }
        roots.clear();
        assert_eq!(self.runtime.drain_retired_executions().unwrap(), 0);
        checksum
    }
}

fn release_order(population: usize, order: &str) -> Vec<usize> {
    let mut indices: Vec<_> = (0..population).collect();
    match order {
        "tail" => indices.reverse(),
        "random" => {
            let mut state = 0x4b41_4741_5249_u64;
            for index in (1..population).rev() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                indices.swap(index, (state % (index + 1) as u64) as usize);
            }
        }
        "forward" => {}
        _ => panic!("unknown release order"),
    }
    indices
}

pub(super) fn run(options: &Options) {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("execution-controls.kgr", SOURCE),
            Default::default(),
        )
        .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let fixture = Fixture::new(&engine, &program);
    for (population, order) in CASES {
        for cancel in [false, true] {
            let indices = release_order(population, order);
            let mode = if cancel { "cancel" } else { "complete" };
            let name = format!("roots_{population}_{order}_{mode}");
            #[cfg(feature = "diagnostics")]
            if options.diagnostics {
                count(&engine, &program, population, &indices, cancel, &name);
                continue;
            }
            let mut roots = Vec::with_capacity(population);
            for sample in 0..options.warmups + options.samples {
                let start = Instant::now();
                fixture.park(&mut roots, population);
                let admission = start.elapsed().as_nanos();
                let start = Instant::now();
                let checksum = fixture.retire(&mut roots, &indices, cancel);
                let retirement = start.elapsed().as_nanos();
                let start = Instant::now();
                fixture.runtime.runtime().collect_garbage().unwrap();
                let collection = start.elapsed().as_nanos();
                if sample >= options.warmups {
                    let sample = sample - options.warmups;
                    println!("admit,{name},kagari_vm,{population},1,{sample},{admission},0");
                    println!(
                        "retire,{name},kagari_vm,{population},1,{sample},{retirement},{checksum}"
                    );
                    println!("collect,{name},kagari_vm,{population},1,{sample},{collection},0");
                }
                black_box(&fixture);
            }
        }
    }
}

#[cfg(feature = "diagnostics")]
fn count(
    engine: &KagariEngine,
    program: &PreparedProgram,
    population: usize,
    order: &[usize],
    cancel: bool,
    name: &str,
) {
    // Warm global lazy initialization outside accounting, then measure a complete
    // independent runtime lifetime. All retained capacities remain included.
    let fixture = Fixture::new(engine, program);
    let mut roots = Vec::with_capacity(population);
    fixture.park(&mut roots, population);
    fixture.retire(&mut roots, order, cancel);
    drop(fixture);
    let ((phases, counts), total) = allocations::measure(|| {
        diagnostics::measure(|| {
            let fixture = Fixture::new(engine, program);
            let setup = allocations::current().unwrap();
            let mut phases = [(setup, setup); 3];
            for phase in &mut phases {
                fixture.park(&mut roots, population);
                phase.0 = allocations::current().unwrap();
                fixture.retire(&mut roots, order, cancel);
                fixture.runtime.runtime().collect_garbage().unwrap();
                phase.1 = allocations::current().unwrap();
            }
            (setup, phases)
        })
    });
    assert_eq!(total.net_bytes, 0, "runtime teardown: {name}");
    assert_eq!(
        phases.1[1].1.net_bytes, phases.1[2].1.net_bytes,
        "reuse plateau: {name}"
    );
    println!("ROOT_COUNTS,{name},phases={phases:?},counts={counts:?},total={total:?}");
}
