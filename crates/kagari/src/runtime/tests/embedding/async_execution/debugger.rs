//! Debug observation follows an owned root across slices and observer replacement.
use super::{Fixture, Value};
use kagari_runtime::debug::{DebugPauseReason, DebugSession, SourceBreakpoint};
use kagari_runtime::error::RuntimeErrorKind;
use kagari_runtime::execution::owned::DriveResult;
use kagari_runtime::{native::completion::CompletionStatus, session::owned::OwnedExecution};
use std::{num::NonZeroUsize, sync::atomic::Ordering};

const SOURCE: &str = r#"
use test::async_sdk::request;
pub async fn work(tag: i32) -> i32 {
    val reply = request(tag).await;
    reply + 1
}
pub async fn left() -> i32 { work(10).await }
pub async fn right() -> i32 { work(20).await }
"#;

fn drive(f: &mut Fixture, execution: &OwnedExecution) -> DriveResult {
    for _ in 0..1000 {
        let result = f
            .runtime
            .drive(execution, NonZeroUsize::new(1).unwrap())
            .unwrap();
        f.runtime.collect_garbage().unwrap();
        if !matches!(result, DriveResult::Runnable) {
            return result;
        }
    }
    panic!("bounded debug driver");
}

#[test]
fn async_debugger_drive_contract() {
    let mut f = Fixture::source(SOURCE);
    let left_future = f.invoke("left", &[]);
    let right_future = f.invoke("right", &[]);
    let mut debug = DebugSession::new(&f.runtime).unwrap();
    let breakpoint = debug
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            f.runtime.select_program(&f.module).unwrap().name.clone(),
            SOURCE.find("val reply").unwrap(),
        ))
        .unwrap();
    f.runtime.set_execution_observer(debug).unwrap();
    let left = f.start_future(&left_future);
    let right = f.start_future(&right_future);
    assert_ne!(left.execution_id(), right.execution_id());
    drop((left_future, right_future));
    assert!(matches!(drive(&mut f, &left), DriveResult::Waiting));
    let depth = {
        let debug = f.runtime.execution_observer::<DebugSession>().unwrap();
        let pause = debug.pauses().last().unwrap();
        assert_eq!(pause.execution, left.execution_id());
        assert_eq!(pause.reason, DebugPauseReason::Breakpoint(breakpoint));
        assert!(
            pause
                .frames
                .iter()
                .any(|frame| frame.function_name.contains("left"))
        );
        assert!(
            pause
                .frames
                .iter()
                .all(|frame| !frame.function_name.contains("right"))
        );
        pause.frames.len()
    };
    f.runtime
        .execution_observer_mut::<DebugSession>()
        .unwrap()
        .step_over(depth)
        .unwrap();
    assert!(matches!(drive(&mut f, &right), DriveResult::Waiting));
    {
        let debug = f.runtime.execution_observer::<DebugSession>().unwrap();
        assert!(
            debug
                .pauses()
                .iter()
                .any(|pause| pause.execution == right.execution_id()
                    && pause.reason == DebugPauseReason::Breakpoint(breakpoint))
        );
        assert!(
            debug
                .pauses()
                .iter()
                .all(|pause| pause.reason != DebugPauseReason::Step),
            "an unrelated root cannot consume left's step request"
        );
        for pause in debug
            .pauses()
            .iter()
            .filter(|pause| pause.execution == right.execution_id())
        {
            assert!(
                pause
                    .frames
                    .iter()
                    .all(|frame| !frame.function_name.contains("left"))
            );
        }
    }
    let left_reply = f.sent.lock().unwrap().remove(0);
    left_reply.complete(Ok(4));
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        2,
        "wake never drives debugged code"
    );
    let DriveResult::Complete(result) = drive(&mut f, &left) else {
        panic!("left completes")
    };
    assert_eq!(f.value(&result.unwrap()), Value::I32(5));
    {
        let debug = f.runtime.execution_observer::<DebugSession>().unwrap();
        let step = debug
            .pauses()
            .iter()
            .find(|pause| pause.reason == DebugPauseReason::Step)
            .unwrap();
        assert_eq!(step.execution, left.execution_id());
    }
    // Detaching between activations must not leave a sticky observer-attached flag.
    f.runtime.clear_execution_observer().unwrap();
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(7));
    let DriveResult::Complete(result) = drive(&mut f, &right) else {
        panic!("right completes after detach")
    };
    assert_eq!(f.value(&result.unwrap()), Value::I32(8));
    assert!(!f.runtime.is_quarantined());
    drop((left, right));
    let future = f.invoke("left", &[]);
    let resumed = f.start_future(&future);
    drop(future);
    assert!(matches!(drive(&mut f, &resumed), DriveResult::Waiting));
    // Attaching to an already parked root resolves its pinned code before resumption.
    let mut replacement = DebugSession::new(&f.runtime).unwrap();
    let after = replacement
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            f.runtime.select_program(&f.module).unwrap().name.clone(),
            SOURCE.find("reply + 1").unwrap(),
        ))
        .unwrap();
    f.runtime.set_execution_observer(replacement).unwrap();
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(8));
    let DriveResult::Complete(result) = drive(&mut f, &resumed) else {
        panic!("reattached root completes")
    };
    assert_eq!(f.value(&result.unwrap()), Value::I32(9));
    {
        let debug = f.runtime.execution_observer::<DebugSession>().unwrap();
        assert!(
            debug
                .pauses()
                .iter()
                .any(|pause| pause.execution == resumed.execution_id()
                    && pause.reason == DebugPauseReason::Breakpoint(after))
        );
    }
    f.runtime.clear_execution_observer().unwrap();
    drop(resumed);
    let future = f.invoke("right", &[]);
    let cancelled = f.start_future(&future);
    drop(future);
    assert!(matches!(drive(&mut f, &cancelled), DriveResult::Waiting));
    let late = f.sent.lock().unwrap().pop().unwrap();
    f.runtime
        .set_execution_observer(DebugSession::new(&f.runtime).unwrap())
        .unwrap();
    cancelled.cancel();
    assert!(matches!(
        drive(&mut f, &cancelled),
        DriveResult::Complete(Err(error)) if matches!(error.clone(), ref error if error.kind() == RuntimeErrorKind::Cancelled)
    ));
    assert_eq!(f.cancels.load(Ordering::SeqCst), 1);
    assert_eq!(late.complete(Ok(10)), CompletionStatus::Stale);
    assert!(!f.runtime.is_quarantined());
    f.runtime.clear_execution_observer().unwrap();
    drop(cancelled);
    f.runtime.collect_garbage().unwrap();
    assert_eq!(f.runtime.gc().active_roots(), 0);
}
