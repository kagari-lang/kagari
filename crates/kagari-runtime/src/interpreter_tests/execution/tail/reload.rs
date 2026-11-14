//! A resumed old chain must select current recursive calls after publication.
use super::{SOURCE, drive, runtime};
use crate::{
    execution::owned::DriveResult,
    interpreter_tests::common::{compile_test_bytecode, round_trip_bytecode},
    test_support::bytecode::{load_bytecode, reload_bytecode},
    value::carriers::RootedValue,
};
use std::num::NonZeroUsize;

#[test]
fn parked_self_tail_chain_falls_back_to_the_current_body_after_publication() {
    let old_code = round_trip_bytecode(compile_test_bytecode(SOURCE));
    let next_code = round_trip_bytecode(compile_test_bytecode(include_str!(
        "../../../../tests/fixtures/execution/self-tail-next.kgr"
    )));
    let mut runtime = runtime();
    let program = load_bytecode(&mut runtime, "tail-reload", old_code).unwrap();
    let old = runtime.select_program(&program).unwrap();
    let execution = runtime
        .start(
            &old,
            "sum",
            &[RootedValue::from(100_000i64), RootedValue::from(0i64)],
            runtime.execution_options(),
        )
        .unwrap();
    let single = NonZeroUsize::new(1).unwrap();
    for _ in 0..100 {
        assert!(matches!(
            runtime.drive(&execution, single).unwrap(),
            DriveResult::Runnable
        ));
        let frames = runtime.resources.sessions.frames(execution.id).unwrap();
        if frames[0].tail_calls_elided() {
            break;
        }
    }
    assert!(runtime.resources.sessions.frames(execution.id).unwrap()[0].tail_calls_elided());
    reload_bytecode(&mut runtime, &program, None, next_code).unwrap();
    let current = runtime.select_program(&program).unwrap();
    assert_ne!(old.key(), current.key());
    runtime.collect_garbage().unwrap();
    // The old caller remains intact while its next call admits the new body.
    // Subsequent current-body steps may reuse that new frame in this activation.
    let mut entered = false;
    for _ in 0..100 {
        assert!(matches!(
            runtime.drive(&execution, single).unwrap(),
            DriveResult::Runnable
        ));
        let frames = runtime.resources.sessions.frames(execution.id).unwrap();
        if frames.len() == 2 {
            assert_eq!(frames[0].loaded().key(), old.key());
            assert_eq!(frames[1].loaded().key(), current.key());
            entered = true;
            break;
        }
    }
    assert!(
        entered,
        "a stale self-tail eligibility must not jump back to v1"
    );
    let (result, observed) = drive(&mut runtime, &execution, 257);
    assert_eq!(result.unwrap().as_i64(), Some(5_000_051_000));
    assert_eq!(observed.frames, 2);
    assert!(observed.elided);
    assert!(!observed.grew_after_elision);
    assert_eq!(runtime.resources().counters().peak_call_depth, 2);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    runtime.collect_garbage().unwrap();
    assert!(runtime.validate_executable_module(&old).is_err());
    assert!(!runtime.is_quarantined());
}
