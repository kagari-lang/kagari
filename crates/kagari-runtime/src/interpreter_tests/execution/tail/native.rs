//! A completed Native effect remains visible when a reused chain is cancelled.
use super::{drive, runtime};
use crate::{
    error::RuntimeErrorKind,
    interpreter_tests::common::{compile_with_provider, round_trip_bytecode},
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::NativeCall,
        declarations::FunctionDecl,
    },
    test_support::bytecode::load_bytecode,
    value::carriers::RootedValue,
};
use kagari_common::cancellation::CancellationToken;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn cancellation_after_native_effect_preserves_the_reused_chain_and_commits() {
    let token = CancellationToken::default();
    let cancel = token.clone();
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    let mut module = ModuleBuilder::new("test::tail_control", &Default::default()).unwrap();
    let step = module.define_function(FunctionDecl::new("step")).unwrap();
    module
        .bind(step, move |cx: &mut NativeCall<'_>| -> NativeResult<()> {
            let count = observed.fetch_add(1, Ordering::Relaxed) + 1;
            cx.collect_garbage()?;
            if count == 10 {
                cancel.cancel();
            }
            Ok(())
        })
        .unwrap();
    let module = module.finish().unwrap();
    let code = round_trip_bytecode(compile_with_provider(
        include_str!("../../../../tests/fixtures/execution/self-tail-native.kgr"),
        Some(&module),
    ));
    let mut runtime = runtime();
    module.install(&mut runtime).unwrap();
    let program = load_bytecode(&mut runtime, "tail-effect", code).unwrap();
    let owner = runtime.select_program(&program).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let mut options = runtime.execution_options();
    options.cancellation = token;
    let execution = runtime
        .start(&owner, "counted", &[RootedValue::from(1000i64)], options)
        .unwrap();
    let error = drive(&mut runtime, &execution, 257).0.unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
    assert_eq!(effects.load(Ordering::Relaxed), 10);
    let trace = error.trace().unwrap();
    assert_eq!(
        trace
            .frames
            .iter()
            .map(|frame| frame.function_name.as_str())
            .collect::<Vec<_>>(),
        ["step", "counted"]
    );
    assert!(!trace.frames[0].tail_calls_elided);
    assert!(trace.frames[1].tail_calls_elided);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert!(!runtime.is_quarantined());
    let fresh = runtime
        .start(
            &owner,
            "counted",
            &[RootedValue::from(0i64)],
            runtime.execution_options(),
        )
        .unwrap();
    assert_eq!(
        drive(&mut runtime, &fresh, 257).0.unwrap().as_i64(),
        Some(0)
    );
    assert_eq!(effects.load(Ordering::Relaxed), 11);
}
