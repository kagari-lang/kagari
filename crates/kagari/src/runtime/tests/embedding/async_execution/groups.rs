//! Structured pair contracts on the same notification-driven scheduler as Tasks.
use super::{
    Fixture,
    scheduler::{HostLoop, decode, result, scheduled},
};
use crate::{engine::EngineConfig, runtime::owned::DriveResult};
use kagari_runtime::{
    error::RuntimeErrorKind, native::completion::CompletionStatus, scheduler::TurnBudget,
    value::Value,
};
use std::{num::NonZeroUsize, sync::atomic::Ordering};

const SOURCE: &str = r#"
use core::task::{join, try_join};
use test::async_sdk::request;
pub fn cold() -> Future<(i32,i32)> { join(request(10), request(20)) }
pub async fn pair() -> i32 { val (a,b)=cold().await; a*100+b }
async fn read() -> Result<i32,String> { Ok(request(10).await) }
async fn reject() -> Result<i32,String> { Err("original") }
pub async fn fail_right() -> Result<(i32,i32),String> { try_join(read(), reject()).await }
pub async fn ordinary_result() -> i32 {
    val (a,b)=join(reject(), request(20)).await;
    match a { Err(_) => b, Ok(_) => 0 }
}
pub async fn successful() -> i32 {
    val value:Result<(i32,i32),String> = try_join(async { Ok(3) }, async { Ok(4) }).await;
    match value { Ok((a,b)) => a*10+b, Err(_) => 0 }
}
pub async fn nested_output() -> i32 {
    val (a,b)=join(async { request(10) }, async { 2 }).await;
    a.await+b
}
pub fn input() -> Future<i32> { request(0) }
pub fn compose(a:Future<i32>,b:Future<i32>) -> Future<(i32,i32)> { join(a,b) }
pub async fn duplicate(a:Future<i32>) -> i32 { val (x,y)=join(a,a).await; x+y }
pub async fn supplied(a:Future<i32>,b:Future<i32>) -> i32 { val (x,y)=join(a,b).await; x+y }
pub async fn nested() -> i32 { val ((a,b),c)=join(join(request(10),request(20)),request(30)).await; a+b+c }
pub async fn busy() -> i32 { var n=0; while n<200 { n+=1; } n }
pub async fn fair() -> i32 { val (a,b)=join(busy(),request(10)).await; a+b }
fn admitted(result:Result<Task<i32>,SpawnError>) -> Task<i32> {
    match result { Ok(task) => task, Err(_) => { val zero=0; val bad=1/zero; admitted(result) } }
}
pub fn launch(scope:TaskScope) -> Task<i32> { admitted(scope.spawn(async || request(10).await)) }
pub async fn observe(task:Task<i32>) -> i32 { task.await }
pub async fn shared(task:Task<i32>) -> Result<(i32,i32),String> { try_join(async { Ok(task.await) },reject()).await }
pub fn tasks() -> Vec<Task<i32>> { Vec::from([]) }
pub fn add(tasks:Vec<Task<i32>>, task:Task<i32>) { tasks.push(task); }
pub fn cycle(scope:TaskScope, tasks:Vec<Task<i32>>, nested:bool) -> Task<i32> {
    admitted(scope.spawn(async || {
        if nested { val ((a,b),c)=join(join(async { tasks[0].await },async { 1 }),async { 2 }).await; a+b+c }
        else { val (a,b)=join(async { tasks[0].await },async { 1 }).await; a+b }
    }))
}
async fn recurse(n:i32) -> i32 { if n==0 { 1 } else { val (a,b)=join(recurse(n-1),async { 0 }).await; a+b } }
pub async fn deep() -> i32 { recurse(20).await }
"#;

#[test]
fn groups_waits_are_nonowning_and_cycles_cross_nested_groups() {
    let mut f = Fixture::source(SOURCE);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let scope = f
        .runtime
        .create_scheduled_task_scope(&f.module, &Default::default())
        .unwrap();
    let task = f.invoke("launch", &[f.value(scope.capability())]);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 1);
    let future = f.invoke("shared", &[f.value(&task)]);
    let mut joined = f
        .runtime
        .runtime()
        .schedule_execution(f.start_future(&future), |_, _| Ok(()))
        .unwrap();
    host.until(&mut f.runtime, || joined.is_finished());
    result(&mut joined).unwrap();
    assert_eq!(
        f.cancels.load(Ordering::SeqCst),
        0,
        "group exit detaches a wait, not its shared Task"
    );
    let mut observer = scheduled(&f, "observe", &[f.value(&task)]);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(42));
    host.until(&mut f.runtime, || observer.is_finished());
    assert_eq!(result(&mut observer).unwrap(), 42);
    for nested in [false, true] {
        let tasks = f.invoke("tasks", &[]);
        let task = f.invoke(
            "cycle",
            &[
                f.value(scope.capability()),
                f.value(&tasks),
                Value::Bool(nested),
            ],
        );
        drop(f.invoke("add", &[f.value(&tasks), f.value(&task)]));
        let mut observer = scheduled(&f, "observe", &[f.value(&task)]);
        host.until(&mut f.runtime, || observer.is_finished());
        let error = result(&mut observer).unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        assert!(error.to_string().contains("cycle"), "{error}");
        assert!(
            f.runtime.take_task_event().is_some(),
            "only the independent Task reports failure"
        );
        assert!(f.runtime.take_task_event().is_none());
    }
    host.settle(&mut f.runtime);
}

#[test]
fn groups_pre_admission_cancel_capacity_and_parent_depth() {
    let mut config = EngineConfig::default();
    config.default_runtime.async_limits.max_groups = NonZeroUsize::new(1).unwrap();
    let mut f = Fixture::configured_source(SOURCE, config);
    let left = f.invoke("input", &[]);
    let right = f.invoke("input", &[]);
    let composition = f.invoke("compose", &[f.value(&left), f.value(&right)]);
    let owner = f.start_future(&composition);
    assert!(matches!(
        f.runtime
            .drive(&owner, NonZeroUsize::new(256).unwrap())
            .unwrap(),
        DriveResult::Waiting
    ));
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        0,
        "parent parks before admission"
    );
    let blocked = f.start_future(&f.invoke("cold", &[]));
    assert!(matches!(
        f.runtime
            .drive(&blocked, NonZeroUsize::new(256).unwrap())
            .unwrap(),
        DriveResult::Complete(Err(_))
    ));
    drop(owner);
    f.runtime.runtime().drain_retired_executions().unwrap();
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let mut ticket = scheduled(&f, "supplied", &[f.value(&left), f.value(&right)]);
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(
        result(&mut ticket).unwrap(),
        14,
        "cancel before admission claims neither input and releases capacity"
    );
    let mut config = EngineConfig::default();
    config.default_runtime.limits.max_call_depth = Some(8);
    let mut f = Fixture::configured_source(SOURCE, config);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let mut ticket = scheduled(&f, "deep", &[]);
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(
        result(&mut ticket).unwrap_err().kind(),
        RuntimeErrorKind::ResourceLimitExceeded,
        "branches inherit logical parent depth"
    );
}

#[test]
fn groups_cold_dual_io_order_and_result_policy() {
    let mut f = Fixture::source(SOURCE);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let cold = f.invoke("cold", &[]);
    f.runtime.runtime().collect_garbage().unwrap();
    host.settle(&mut f.runtime);
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    drop(cold);
    let mut ticket = scheduled(&f, "pair", &[]);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 2);
    host.settle(&mut f.runtime);
    assert_eq!(*f.inputs.lock().unwrap(), vec![10, 20]);
    let right = f.sent.lock().unwrap().pop().unwrap();
    assert_eq!(right.complete(Ok(2)), CompletionStatus::Accepted);
    host.settle(&mut f.runtime);
    assert!(!ticket.is_finished());
    f.runtime.runtime().collect_garbage().unwrap();
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(1));
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(result(&mut ticket).unwrap(), 102);
    let mut ticket = scheduled(&f, "successful", &[]);
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(result(&mut ticket).unwrap(), 34);
    let mut ticket = scheduled(&f, "ordinary_result", &[]);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 3);
    host.settle(&mut f.runtime);
    assert!(!ticket.is_finished(), "join does not interpret Err");
    assert_eq!(f.cancels.load(Ordering::SeqCst), 0);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(9));
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(result(&mut ticket).unwrap(), 9);
    let mut ticket = scheduled(&f, "nested_output", &[]);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 4);
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(7));
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(result(&mut ticket).unwrap(), 9);
    assert!(f.runtime.take_task_event().is_none());
}

#[test]
fn groups_right_err_cleans_sibling_before_publication_and_keeps_origin() {
    let mut f = Fixture::source(SOURCE);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let future = f.invoke("fail_right", &[]);
    let cancels = f.cancels.clone();
    let mut ticket = f
        .runtime
        .runtime()
        .schedule_execution(f.start_future(&future), move |runtime, root| {
            assert_eq!(
                cancels.load(Ordering::SeqCst),
                1,
                "cleanup precedes parent result conversion"
            );
            let value = root.value(runtime.gc()).unwrap();
            let trace = runtime
                .gc()
                .result_error_trace(&value)
                .expect("original Err origin");
            assert!(
                trace
                    .frames
                    .iter()
                    .any(|frame| frame.function_name.contains("reject")),
                "{trace:?}"
            );
            Ok(root.clone())
        })
        .unwrap();
    host.until(&mut f.runtime, || ticket.is_finished());
    let root = result(&mut ticket).unwrap();
    f.runtime.runtime().collect_garbage().unwrap();
    assert!(root.value(f.runtime.runtime().gc()).is_some());
    assert_eq!(f.starts.load(Ordering::SeqCst), 1);
    assert_eq!(f.cancels.load(Ordering::SeqCst), 1);
    let late = f.sent.lock().unwrap().pop().unwrap();
    assert_eq!(late.complete(Ok(99)), CompletionStatus::Stale);
    host.settle(&mut f.runtime);
    assert!(f.runtime.take_task_event().is_none());
}

#[test]
fn groups_claim_pair_atomically_and_bound_admission() {
    let mut f = Fixture::source(SOURCE);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let input = f.invoke("input", &[]);
    let mut duplicate = scheduled(&f, "duplicate", &[f.value(&input)]);
    host.until(&mut f.runtime, || duplicate.is_finished());
    assert_eq!(
        result(&mut duplicate).unwrap_err().kind(),
        RuntimeErrorKind::ScriptTrap
    );
    assert_eq!(f.starts.load(Ordering::SeqCst), 0);
    let mut consumed = f
        .runtime
        .runtime()
        .schedule_execution(f.start_future(&input), decode)
        .unwrap();
    host.until(&mut f.runtime, || consumed.is_finished());
    assert_eq!(result(&mut consumed).unwrap(), 7);
    let other = f.invoke("input", &[]);
    let mut invalid = scheduled(&f, "supplied", &[f.value(&other), f.value(&input)]);
    host.until(&mut f.runtime, || invalid.is_finished());
    assert_eq!(
        result(&mut invalid).unwrap_err().kind(),
        RuntimeErrorKind::ScriptTrap
    );
    let mut untouched = f
        .runtime
        .runtime()
        .schedule_execution(f.start_future(&other), decode)
        .unwrap();
    host.until(&mut f.runtime, || untouched.is_finished());
    assert_eq!(result(&mut untouched).unwrap(), 7);
    assert_eq!(f.starts.load(Ordering::SeqCst), 2);

    let mut config = EngineConfig::default();
    config.default_runtime.async_limits.max_group_depth = NonZeroUsize::new(1).unwrap();
    let mut f = Fixture::configured_source(SOURCE, config);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let mut ticket = scheduled(&f, "nested", &[]);
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(
        result(&mut ticket).unwrap_err().kind(),
        RuntimeErrorKind::ResourceLimitExceeded
    );
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        0,
        "nested left branch terminates before right activation"
    );
}

#[test]
fn groups_parent_drop_drains_nested_waits_and_shares_turn_budget() {
    let mut f = Fixture::source(SOURCE);
    let host = HostLoop::new();
    assert!(f.runtime.poll(&mut host.context()).is_pending());
    let mut ticket = scheduled(&f, "nested", &[]);
    host.until(&mut f.runtime, || f.starts.load(Ordering::SeqCst) == 3);
    host.settle(&mut f.runtime);
    f.runtime
        .runtime()
        .cancel_scheduled_execution(&ticket)
        .unwrap();
    assert_eq!(f.cancels.load(Ordering::SeqCst), 3);
    assert_eq!(
        result(&mut ticket).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    for late in f.sent.lock().unwrap().drain(..) {
        assert_eq!(late.complete(Ok(99)), CompletionStatus::Stale);
    }
    host.settle(&mut f.runtime);
    let mut ticket = scheduled(&f, "fair", &[]);
    let budget = TurnBudget {
        instructions: NonZeroUsize::new(1).unwrap(),
        quantum: NonZeroUsize::new(1).unwrap(),
        activations: NonZeroUsize::new(1).unwrap(),
    };
    for _ in 0..100 {
        if f.starts.load(Ordering::SeqCst) == 4 {
            break;
        }
        host.turn(&mut f.runtime, budget, None);
    }
    assert_eq!(f.starts.load(Ordering::SeqCst), 4);
    assert!(!ticket.is_finished());
    f.sent.lock().unwrap().pop().unwrap().complete(Ok(5));
    host.until(&mut f.runtime, || ticket.is_finished());
    assert_eq!(result(&mut ticket).unwrap(), 205);
}
