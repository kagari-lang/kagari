//! Manual ordinary-profile timing, kept separate from allocation instrumentation.
use super::SOURCE;
use crate::{
    Runtime,
    executor::Executor,
    interpreter_tests::common::{compile_test_bytecode, round_trip_bytecode},
    module::{ExecutableModule, InstalledProgram},
    native::conversion::arguments::ArgumentValues,
    test_support::bytecode::load_bytecode,
    value::view::ValueView,
};
use kagari_contract::ids::FunctionRef;
use std::{array, hint::black_box, time::Instant};

struct Case {
    runtime: Runtime,
    _program: InstalledProgram,
    owner: ExecutableModule,
    function: FunctionRef,
    arguments: Vec<ValueView<'static>>,
}

impl Case {
    fn run(&mut self) -> i64 {
        Executor::new(
            &mut self.runtime,
            &self.owner,
            self.function,
            ArgumentValues::scalars(&self.arguments),
        )
        .unwrap()
        .run(|runtime, stack, value| stack.retain_result(runtime, value))
        .unwrap()
        .as_i64()
        .unwrap()
    }
}

#[test]
#[ignore = "manual uninstrumented tail/loop/ordinary timing; run without concurrent jobs"]
fn scalar_tail_loop_and_ordinary_recursion_timing() {
    let setup = Instant::now();
    let code = round_trip_bytecode(compile_test_bytecode(SOURCE));
    let compile_ns = setup.elapsed().as_nanos();
    for (n, calls) in [(16i64, 1000usize), (1024, 100)] {
        let setup = Instant::now();
        let names = ["sum", "scalar_loop", "ordinary"];
        let mut cases: Vec<_> = names
            .iter()
            .map(|&name| {
                let mut runtime = Runtime::default();
                let program = load_bytecode(&mut runtime, "tail-timing", code.clone()).unwrap();
                let owner = runtime.select_program(&program).unwrap();
                let function = owner
                    .bytecode
                    .functions
                    .iter()
                    .find(|function| function.name == name)
                    .unwrap()
                    .id;
                let arguments = if name == "ordinary" {
                    vec![ValueView::from(n)]
                } else {
                    vec![ValueView::from(n), ValueView::from(0i64)]
                };
                Case {
                    runtime,
                    _program: program,
                    owner,
                    function,
                    arguments,
                }
            })
            .collect();
        let prepare_ns = setup.elapsed().as_nanos();
        let expected = n * (n + 1) / 2;
        for case in &mut cases {
            for _ in 0..3 {
                assert_eq!(case.run(), expected);
            }
        }
        let mut samples: [Vec<f64>; 3] = array::from_fn(|_| Vec::with_capacity(11));
        for round in 0..11 {
            for shift in 0..3 {
                let index = (round + shift) % 3;
                let start = Instant::now();
                let mut last = 0;
                for _ in 0..calls {
                    last = black_box(cases[index].run());
                }
                let elapsed = start.elapsed().as_nanos();
                assert_eq!(last, expected);
                let ns = elapsed as f64 / calls as f64;
                samples[index].push(ns);
                println!(
                    "TAIL_TIMING_SAMPLE,n={n},route={},round={round},calls={calls},ns_per_call={ns:.3}",
                    names[index]
                );
            }
        }
        for sample in &mut samples {
            sample.sort_by(f64::total_cmp);
        }
        let medians = samples.map(|sample| sample[sample.len() / 2]);
        println!(
            "TAIL_TIMING,n={n},calls={calls},compile_ns={compile_ns},prepare_three_runtimes_ns={prepare_ns},tail_ns={:.3},loop_ns={:.3},ordinary_ns={:.3},tail_over_loop={:.3},tail_over_ordinary={:.3}",
            medians[0],
            medians[1],
            medians[2],
            medians[0] / medians[1],
            medians[0] / medians[2]
        );
        for case in &cases {
            assert_eq!(case.runtime.resources().counters().current_call_depth, 0);
            assert!(!case.runtime.is_quarantined());
        }
    }
}
