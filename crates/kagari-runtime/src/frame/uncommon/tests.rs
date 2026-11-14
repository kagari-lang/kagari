//! A parked method keeps cleanup facts after its SessionState is detached.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::storage::{execution_backing, execution_usage};
use crate::{
    Runtime, error::RuntimeErrorKind, frame::ExecutionStack,
    native::conversion::arguments::ArgumentValues, test_support::update_programs,
    value::carriers::RootedValue,
};

#[test]
fn parked_method_control_reuses_after_session_detachment() {
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program(
            "parked-method-control",
            update_programs::source_code(include_str!(
                "../../../tests/fixtures/metadata/interface-parents.kgr"
            )),
        )
        .unwrap();
    let module = runtime.select_program(&program).unwrap();
    let main = module
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap()
        .id;
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats().root_registrations;
    let mut capacity = None;
    for _ in 0..3 {
        let value = runtime
            .make_interface(&module, 0, &RootedValue::from(7_i32))
            .unwrap();
        let description = runtime.interface_description(&value).unwrap();
        let method = runtime
            .resolve_interface_method_slot(&value, description.interface_type(), 0, &[])
            .unwrap();
        let admitted = method.admit(&mut runtime).unwrap();
        drop((value, method));
        let execution = runtime
            .start_owned_execution(&module, main, &[], Default::default())
            .unwrap();
        let mut activation = Some(runtime.resume_owned_execution(&execution).unwrap());
        let computation = activation
            .as_mut()
            .unwrap()
            .with(|runtime, stack| {
                stack.push_interface_method(
                    runtime,
                    &admitted,
                    ArgumentValues::scalars(&[]),
                    None,
                )?;
                drop(admitted);
                runtime.collect_garbage()?;
                assert_eq!(runtime.gc_stats().root_registrations, baseline);
                assert_eq!(runtime.resources.uncommon_frames.live(), 1);
                Ok(stack
                    .current(runtime)?
                    .invocation(&runtime.resources)
                    .unwrap()
                    .computation()
                    .unwrap()
                    .clone())
            })
            .unwrap();
        assert!(ExecutionStack::try_park_ready(&mut activation).unwrap());
        assert!(activation.is_none());
        drop(activation);
        let reserved = runtime.resources.uncommon_frames.capacity();
        assert_eq!(*capacity.get_or_insert(reserved), reserved);
        assert_eq!(runtime.resources.uncommon_frames.live(), 1);
        #[cfg(feature = "execution-diagnostics")]
        {
            let usage = execution_usage(&runtime);
            println!(
                "UNCOMMON_PARKED_BACKING,{:?},pool={:?}",
                execution_backing(&runtime),
                usage
                    .buffers
                    .iter()
                    .find(|buffer| buffer.name == "uncommon frame control")
                    .unwrap()
            );
        }
        // The owned drain detaches SessionState before releasing frame sources
        // and then their uncommon Rust control. The pool must survive that handoff.
        drop(execution);
        assert_eq!(runtime.drain_retired_executions().unwrap(), 1);
        assert_eq!(runtime.resources.uncommon_frames.live(), 0);
        #[cfg(feature = "execution-diagnostics")]
        println!("UNCOMMON_RETIRED_BACKING,{:?}", execution_backing(&runtime));
        runtime.collect_garbage().unwrap();
        assert_eq!(
            runtime.application_record(&computation).unwrap_err().kind(),
            RuntimeErrorKind::ModuleValidation
        );
        assert_eq!(runtime.gc_stats().root_registrations, baseline);
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert!(!runtime.is_quarantined());
    }
}
