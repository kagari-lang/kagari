//! Actual source-free execution, not a private move-plan oracle.
use crate::{
    Runtime, RuntimeConfig,
    debug::DebugSession,
    error::{RuntimeError, RuntimeErrorKind, reason::ErrorReason},
    execution::owned::DriveResult,
    interpreter_tests::common::{compile_test_bytecode, round_trip_bytecode},
    resource::RuntimeLimits,
    session::owned::OwnedExecution,
    test_support::bytecode::load_bytecode,
    value::carriers::RootedValue,
};
use kagari_types::{arithmetic::ArithmeticError, scalar::error::ScalarError};
use std::num::NonZeroUsize;

#[cfg(feature = "execution-diagnostics")]
mod measurements;
mod native;
mod reload;
#[cfg(not(feature = "execution-diagnostics"))]
mod timings;

const SOURCE: &str = include_str!("../../../tests/fixtures/execution/self-tail.kgr");
const STACK_BYTES: usize = 16 * 1024;

fn runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        limits: RuntimeLimits {
            max_stack_bytes: NonZeroUsize::new(STACK_BYTES).unwrap(),
            ..Default::default()
        },
        ..Default::default()
    })
}

#[derive(Default)]
struct Observation {
    frames: usize,
    reserved: usize,
    elided: bool,
    stable: Option<usize>,
    grew_after_elision: bool,
}

fn drive(
    runtime: &mut Runtime,
    execution: &OwnedExecution,
    slice: usize,
) -> (Result<RootedValue, RuntimeError>, Observation) {
    let mut observed = Observation::default();
    for _ in 0..100_000 {
        match runtime
            .drive(execution, NonZeroUsize::new(slice).unwrap())
            .unwrap()
        {
            DriveResult::Complete(result) => return (result, observed),
            DriveResult::Waiting => panic!("scalar execution cannot wait"),
            DriveResult::Runnable => {
                let frames = runtime.resources.sessions.frames(execution.id).unwrap();
                observed.frames = observed.frames.max(frames.len());
                let elided = frames.iter().any(|frame| frame.tail_calls_elided());
                observed.elided |= elided;
                let space = runtime.memory.execution_space(execution.id).unwrap();
                let reserved = space.reserved();
                observed.reserved = observed.reserved.max(reserved);
                assert!(reserved <= STACK_BYTES);
                if elided {
                    if let Some(previous) = observed.stable {
                        observed.grew_after_elision |= reserved != previous;
                    } else {
                        observed.stable = Some(reserved);
                    }
                }
                drop(frames);
                // A real collection with parked scalar frames must retain their
                // exact body/source without materializing argument/root packs.
                if observed.elided && observed.stable == Some(reserved) {
                    runtime.collect_garbage().unwrap();
                }
            }
        }
    }
    panic!("bounded scalar fixture did not finish")
}

#[test]
fn scalar_tail_replacement_bounds_real_frames_and_preserves_parallel_arguments() {
    let code = round_trip_bytecode(compile_test_bytecode(SOURCE));
    for (name, arguments, expected, maximum, elided) in [
        ("sum", vec![100_000, 0], 5_000_050_000, 1, true),
        ("scalar_loop", vec![100_000, 0], 5_000_050_000, 1, false),
        ("permute", vec![20_001, 1, 2, 3, 4], 2143, 1, true),
        ("fanout", vec![20_000, 1, 2], 22, 1, true),
        ("nested", vec![], 5_000_050_017, 2, true),
        ("ordinary", vec![10], 55, 11, false),
    ] {
        let mut runtime = runtime();
        let program = load_bytecode(&mut runtime, "self-tail", code.clone()).unwrap();
        let owner = runtime.select_program(&program).unwrap();
        runtime.collect_garbage().unwrap();
        let baseline = runtime.gc_stats();
        let arguments: Vec<_> = arguments
            .into_iter()
            .map(|value| RootedValue::from(value as i64))
            .collect();
        let execution = runtime
            .start(&owner, name, &arguments, runtime.execution_options())
            .unwrap();
        let (result, observed) = drive(
            &mut runtime,
            &execution,
            if name == "ordinary" { 1 } else { 257 },
        );
        assert_eq!(result.unwrap().as_i64(), Some(expected), "{name}");
        assert_eq!(observed.frames, maximum, "{name}");
        assert_eq!(observed.elided, elided, "{name}");
        assert!(
            !observed.grew_after_elision,
            "{name} reuses committed capacity"
        );
        assert_eq!(
            runtime.resources().counters().peak_call_depth as usize,
            maximum
        );
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert!(!runtime.is_quarantined());
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().objects, baseline.objects);
        assert_eq!(
            runtime.gc_stats().root_registrations,
            baseline.root_registrations
        );
        println!(
            "TAIL_STORAGE,{name},frames={},reserved={}",
            observed.frames, observed.reserved
        );
    }
}

#[test]
fn tail_failure_keeps_the_real_origin_elision_and_complete_cleanup() {
    let code = round_trip_bytecode(compile_test_bytecode(SOURCE));
    for (name, arguments, reason, elided) in [
        (
            "sum",
            vec![i64::MIN, 0],
            ErrorReason::Scalar(ScalarError::from(ArithmeticError::Overflow)),
            false,
        ),
        (
            "sum",
            vec![2, i64::MAX - 2],
            ErrorReason::Scalar(ScalarError::from(ArithmeticError::Overflow)),
            true,
        ),
        (
            "trapped",
            vec![20, 0],
            ErrorReason::Scalar(ScalarError::from(ArithmeticError::DivisionByZero)),
            true,
        ),
        ("ordinary", vec![100_000], ErrorReason::StackOverflow, false),
    ] {
        let mut runtime = runtime();
        let program = load_bytecode(&mut runtime, "tail-failure", code.clone()).unwrap();
        let owner = runtime.select_program(&program).unwrap();
        runtime.collect_garbage().unwrap();
        let baseline = runtime.gc_stats();
        let arguments: Vec<_> = arguments.into_iter().map(RootedValue::from).collect();
        let execution = runtime
            .start(&owner, name, &arguments, runtime.execution_options())
            .unwrap();
        let (result, observed) = drive(&mut runtime, &execution, 1);
        let error = result.unwrap_err();
        assert_eq!(error.reason(), &reason);
        let trace = error.trace().unwrap();
        assert_eq!(trace.frames[0].function_name, name);
        assert_eq!(trace.frames[0].tail_calls_elided, elided);
        assert_eq!(
            trace.frames.len(),
            if name == "ordinary" {
                observed.frames
            } else {
                1
            }
        );
        assert_eq!(trace.omitted_frames, 0);
        assert!(!trace.incomplete);
        assert_eq!(
            trace.to_string().contains("self-tail callers elided"),
            elided
        );
        let function = owner
            .bytecode
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        let origin = function
            .metadata
            .instruction_origin(trace.frames[0].instruction_offset)
            .unwrap()
            .1;
        assert_eq!(trace.frames[0].source_span, Some(origin.span));
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().objects, baseline.objects);
        assert_eq!(
            runtime.gc_stats().root_registrations,
            baseline.root_registrations
        );
        let fresh = runtime
            .start(
                &owner,
                "sum",
                &[RootedValue::from(0i64), RootedValue::from(9i64)],
                runtime.execution_options(),
            )
            .unwrap();
        assert_eq!(
            drive(&mut runtime, &fresh, 257).0.unwrap().as_i64(),
            Some(9)
        );
        assert!(!runtime.is_quarantined());
    }
}

#[test]
fn observation_uses_ordinary_tail_calls_and_reports_already_elided_history() {
    let code = round_trip_bytecode(compile_test_bytecode(SOURCE));
    for late in [false, true] {
        let mut runtime = runtime();
        let program = load_bytecode(&mut runtime, "tail-debug", code.clone()).unwrap();
        let owner = runtime.select_program(&program).unwrap();
        let execution = runtime
            .start(
                &owner,
                "sum",
                &[RootedValue::from(30i64), RootedValue::from(0i64)],
                runtime.execution_options(),
            )
            .unwrap();
        if late {
            for _ in 0..100 {
                assert!(matches!(
                    runtime
                        .drive(&execution, NonZeroUsize::new(1).unwrap())
                        .unwrap(),
                    DriveResult::Runnable
                ));
                if runtime
                    .resources
                    .sessions
                    .frames(execution.id)
                    .unwrap()
                    .iter()
                    .any(|frame| frame.tail_calls_elided())
                {
                    break;
                }
            }
            assert!(
                runtime.resources.sessions.frames(execution.id).unwrap()[0].tail_calls_elided()
            );
        }
        let mut debug = DebugSession::new(&runtime).unwrap();
        debug.pause().unwrap();
        runtime.attach_debug_session(debug).unwrap();
        let (result, observed) = drive(&mut runtime, &execution, 1);
        assert_eq!(result.unwrap().as_i64(), Some(465));
        assert_eq!(observed.frames, if late { 30 } else { 31 });
        assert_eq!(observed.elided, late);
        let debug = runtime.debug_session().unwrap();
        let first = debug.pauses().first().unwrap();
        assert_eq!(first.frames.len(), 1);
        assert_eq!(first.frames[0].tail_calls_elided, late);
        assert_eq!(
            first.frames[0]
                .bindings
                .iter()
                .find(|binding| binding.name == "n")
                .unwrap()
                .value
                .as_i64(),
            Some(if late { 29 } else { 30 })
        );
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert!(!runtime.is_quarantined());
    }
}

#[test]
fn cancelling_a_parked_tail_chain_retires_its_single_frame_and_source() {
    let code = round_trip_bytecode(compile_test_bytecode(SOURCE));
    let mut runtime = runtime();
    let program = load_bytecode(&mut runtime, "tail-cancel", code).unwrap();
    let owner = runtime.select_program(&program).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let execution = runtime
        .start(
            &owner,
            "sum",
            &[RootedValue::from(100_000i64), RootedValue::from(0i64)],
            runtime.execution_options(),
        )
        .unwrap();
    assert!(matches!(
        runtime
            .drive(&execution, NonZeroUsize::new(257).unwrap())
            .unwrap(),
        DriveResult::Runnable
    ));
    let frames = runtime.resources.sessions.frames(execution.id).unwrap();
    assert_eq!(frames.len(), 1);
    assert!(frames[0].tail_calls_elided());
    drop(frames);
    execution.cancel();
    let error = drive(&mut runtime, &execution, 257).0.unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
    assert_eq!(error.trace().unwrap().frames.len(), 1);
    assert!(error.trace().unwrap().frames[0].tail_calls_elided);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    let fresh = runtime
        .start(
            &owner,
            "sum",
            &[RootedValue::from(0i64), RootedValue::from(9i64)],
            runtime.execution_options(),
        )
        .unwrap();
    assert_eq!(
        drive(&mut runtime, &fresh, 257).0.unwrap().as_i64(),
        Some(9)
    );
    assert!(!runtime.is_quarantined());
}
