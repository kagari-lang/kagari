use super::fixture;
use crate::error::VmError;
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::ExecutionFrame,
    module::{Module, entry::ProgramEntry},
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        conversion::{
            KagariType,
            context::{ConversionContext, ConversionLimits},
        },
        declarations::FunctionDecl,
        function_handle::Function,
        typed::NativeContext,
    },
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Debug)]
struct Observation {
    nested: RuntimeError,
    termination: Option<RuntimeError>,
    poll: NativeResult<()>,
    follow_up: NativeResult<i32>,
}

#[derive(Clone, Copy)]
enum Failure {
    ConfiguredConversion,
    DefaultConversion,
    ReturnedResource,
    OrdinaryTrap,
}

#[derive(Debug)]
struct TrapObserver {
    error: RuntimeErrorKind,
    terminations: Arc<Mutex<Vec<RuntimeError>>>,
}

impl ExecutionObserver for TrapObserver {
    fn observe(
        &mut self,
        runtime: &Runtime,
        event: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> NativeResult<()> {
        if event != ExecutionEvent::Trap {
            return Ok(());
        }
        let termination = runtime
            .resources()
            .termination()
            .expect("conversion terminated");
        assert_eq!(termination.kind(), RuntimeErrorKind::ResourceLimitExceeded);
        self.terminations.lock().unwrap().push(termination);
        Err(if self.error == RuntimeErrorKind::EngineFault {
            runtime.quarantine_execution_invariant("trap observer fault")
        } else {
            RuntimeError::new(self.error, "trap observer rejection")
        })
    }
}

fn conversion_reentry(failure: Failure, observer_error: Option<RuntimeErrorKind>) {
    let observed = Arc::new(Mutex::new(None));
    let effects = Arc::new(AtomicUsize::new(0));
    let mut module = ModuleBuilder::new(
        "example::functions",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let fail = module
        .define_function(
            FunctionDecl::new("fail").returns(String::kagari_type(module.declarations()).unwrap()),
        )
        .unwrap();
    module
        .bind(
            fail,
            move |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<String> {
                match failure {
                    Failure::DefaultConversion => {
                        Ok("x".repeat(ConversionLimits::default().max_string_bytes + 1))
                    }
                    Failure::ReturnedResource => {
                        Err(RuntimeError::resource_limit("native operation"))
                    }
                    Failure::OrdinaryTrap => Err(RuntimeError::new(
                        RuntimeErrorKind::ScriptTrap,
                        "native trap",
                    )),
                    Failure::ConfiguredConversion => {
                        let owner = cx.runtime().execution_root().unwrap();
                        let mut conversion = ConversionContext::with_limits(
                            cx.runtime(),
                            &owner,
                            ConversionLimits {
                                max_string_bytes: 0,
                                ..Default::default()
                            },
                        )?;
                        let error = conversion.encode("x".to_owned()).unwrap_err();
                        assert_eq!(cx.runtime().resources().termination(), Some(error));
                        // Even swallowing the conversion inside its callback cannot resume.
                        Ok(String::new())
                    }
                }
            },
        )
        .unwrap();
    let enter = module.define_function(FunctionDecl::new("enter")).unwrap();
    let root = Arc::new(OnceLock::<Module>::new());
    let callback_root = root.clone();
    let sink = observed.clone();
    module
        .bind(
            enter,
            move |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                let root = callback_root.get().expect("installed namespace");
                let nested: Function<(), i32> = cx.runtime().bind_function(root, "nested")?;
                let healthy: Function<(), i32> = cx.runtime().bind_function(root, "healthy")?;
                let nested = nested.call(cx, ()).unwrap_err();
                let termination = cx.runtime().resources().termination();
                if termination.is_some() && observer_error.is_none() {
                    assert_eq!(
                        cx.runtime()
                            .resources()
                            .admit_execution_error(RuntimeError::new(
                                RuntimeErrorKind::Cancelled,
                                "later cancellation"
                            )),
                        nested
                    );
                }
                let poll = cx.poll();
                let follow_up = healthy.call(cx, ());
                *sink.lock().unwrap() = Some(Observation {
                    nested,
                    termination,
                    poll,
                    follow_up,
                });
                // Deliberately swallow the failure: only the session can stop this root.
                Ok(())
            },
        )
        .unwrap();
    let effect = module.define_function(FunctionDecl::new("effect")).unwrap();
    let sink = effects.clone();
    module
        .bind(
            effect,
            move |_: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                sink.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
        )
        .unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::functions::{enter, fail, effect};
        fn main() -> i32 { val kept = [42]; effect(); enter(); effect(); kept[0] }
        pub fn nested() -> i32 { val kept = [1]; fail(); effect(); kept[0] }
        pub fn healthy() -> i32 { effect(); 7 }
        "#,
        Some(&module.finish().unwrap()),
    );
    let _owner_program = owner;
    root.set(_owner_program.root()).unwrap();
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let terminations = Arc::new(Mutex::new(Vec::new()));
    let session = observer_error.map(|error| {
        vm.runtime()
            .set_execution_observer(TrapObserver {
                error,
                terminations: terminations.clone(),
            })
            .unwrap();
        let session = vm
            .runtime()
            .begin_execution(&owner, vm.runtime().execution_options())
            .unwrap();
        assert!(vm.runtime().attach_execution_observer().unwrap());
        session
    });
    let result = vm.execute(&owner, ProgramEntry::Main);
    drop(session);
    let observed = observed.lock().unwrap().take().expect("host entered");
    let terminal = !matches!(failure, Failure::OrdinaryTrap);
    let quarantined = observer_error == Some(RuntimeErrorKind::EngineFault);
    if observer_error.is_some() {
        let terminations = terminations.lock().unwrap();
        assert!(!terminations.is_empty(), "Trap observer must run");
        assert!(
            terminations
                .iter()
                .all(|error| Some(error) == observed.termination.as_ref())
        );
    }
    assert_eq!(
        observed.nested.kind(),
        if quarantined {
            RuntimeErrorKind::EngineFault
        } else if terminal {
            RuntimeErrorKind::ResourceLimitExceeded
        } else {
            RuntimeErrorKind::ScriptTrap
        }
    );
    let names = observed
        .nested
        .trace()
        .unwrap()
        .frames
        .iter()
        .map(|frame| frame.function_name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["fail", "nested", "enter", "main"]);
    if terminal {
        assert!(
            result.is_err(),
            "root resumed: {observed:?}; effects={}",
            effects.load(Ordering::Relaxed)
        );
        let error = result.unwrap_err();
        if quarantined {
            assert_eq!(
                observed.termination.as_ref().unwrap().kind(),
                RuntimeErrorKind::ResourceLimitExceeded
            );
            assert_eq!(
                observed.poll.unwrap_err().kind(),
                RuntimeErrorKind::EngineFault
            );
            assert_eq!(
                observed.follow_up.unwrap_err().kind(),
                RuntimeErrorKind::EngineFault
            );
            assert!(
                matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::EngineFault)
            );
        } else {
            assert_eq!(observed.termination.as_ref(), Some(&observed.nested));
            assert_eq!(observed.poll.unwrap_err(), observed.nested);
            assert_eq!(observed.follow_up.unwrap_err(), observed.nested);
            assert!(
                matches!(error.cause(), VmError::RuntimeError(error) if error == &observed.nested)
            );
            assert_eq!(error.trace(), observed.nested.trace());
        }
    } else {
        assert!(observed.termination.is_none());
        observed.poll.unwrap();
        assert_eq!(observed.follow_up.unwrap(), 7);
        let report = result.unwrap();
        assert_eq!(
            report.return_value.value(vm.runtime().gc()),
            Some(Value::I32(42))
        );
    }
    let completed_effects = if terminal { 1 } else { 3 };
    assert_eq!(effects.load(Ordering::Relaxed), completed_effects);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert!(vm.runtime().execution_root().is_none());
    assert!(vm.runtime().resources().termination().is_none());
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(owner.key())
            .active_calls,
        0
    );
    assert_eq!(vm.runtime().is_quarantined(), quarantined);
    if quarantined {
        assert_eq!(
            vm.runtime()
                .resources()
                .ensure_execution_allowed()
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::EngineFault
        );
        return;
    }
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    let healthy = vm
        .runtime()
        .bind_function::<(), i32>(&_owner_program.root(), "healthy")
        .unwrap();
    assert_eq!(vm.call(&healthy, ()).unwrap(), 7);
    assert_eq!(effects.load(Ordering::Relaxed), completed_effects + 1);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn configured_conversion_failure_terminates_reentry_root() {
    conversion_reentry(Failure::ConfiguredConversion, None);
}

#[test]
fn default_conversion_failure_terminates_reentry_root() {
    conversion_reentry(Failure::DefaultConversion, None);
}

#[test]
fn returned_native_resource_failure_terminates_reentry_root() {
    conversion_reentry(Failure::ReturnedResource, None);
}

#[test]
fn ordinary_native_trap_allows_follow_up_reentry() {
    conversion_reentry(Failure::OrdinaryTrap, None);
}

#[test]
fn trap_observer_cannot_replace_conversion_termination() {
    for error in [
        RuntimeErrorKind::ModuleValidation,
        RuntimeErrorKind::ScriptTrap,
    ] {
        conversion_reentry(Failure::ConfiguredConversion, Some(error));
    }
}

#[test]
fn trap_observer_quarantine_takes_precedence_over_conversion_termination() {
    conversion_reentry(
        Failure::ConfiguredConversion,
        Some(RuntimeErrorKind::EngineFault),
    );
}
