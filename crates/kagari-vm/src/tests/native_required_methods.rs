use crate::{error::VmError, tests::common::compile_test_bytecode, vm::Vm};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::host_interface::standard_log;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use {
    kagari_common::capability::CapabilitySet,
    kagari_runtime::{
        Runtime, RuntimeConfig,
        error::RuntimeErrorKind,
        gc::GcHeapConfig,
        host::HostFunction,
        security::{HostExposurePolicy, LanguageProfile, SecurityContext},
        value::Value,
    },
};
mod boundaries;
mod cases;
mod contracts;

fn runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
        },
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..Default::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_functions: vec!["host.log".into()],
            ..Default::default()
        },
        ..Default::default()
    })
}

struct Baseline {
    name: &'static str,
    steps: u64,
    allocations: usize,
    depth: u32,
    effects: &'static [(&'static str, u64)],
    holes: &'static [(u64, u64)],
}
mod baseline;
#[test]
fn required_methods_preserve_every_budget_cut() {
    let cases = cases::cases();
    assert_eq!(cases.len(), baseline::BASELINE.len());
    for (name, source) in cases {
        let expected = baseline::BASELINE
            .iter()
            .find(|case| case.name == name)
            .unwrap();
        let program = compile_test_bytecode(&source);
        for encoded in [false, true] {
            let program = if encoded {
                KbcArtifact::from_bytes(
                    &KbcArtifact::from_program(program.clone(), Default::default())
                        .unwrap()
                        .to_bytes()
                        .unwrap(),
                )
                .unwrap()
                .program
            } else {
                program.clone()
            };
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let base = Rc::new(Cell::new(0));
            let root_base = base.clone();
            let mut rt = runtime();
            rt.register_host_function(HostFunction::new(standard_log(), move |context, args| {
                let Value::Str(label) = &args[0] else {
                    panic!()
                };
                sink.borrow_mut().push((
                    label.clone(),
                    context.runtime().resources().counters().instruction_steps - root_base.get(),
                ));
                Ok(Value::Unit)
            }))
            .unwrap();
            let loaded = rt.load_program(name, program).unwrap();
            let mut vm = Vm::new(rt);
            for limit in 0..=expected.steps {
                effects.borrow_mut().clear();
                base.set(vm.runtime().resources().counters().instruction_steps);
                let mut options = vm.runtime().execution_options();
                options.resources.max_instruction_steps = Some(limit);
                let session = vm.runtime().begin_execution(&loaded, options).unwrap();
                let result = vm.execute(&loaded, "main");
                if limit == expected.steps {
                    assert_eq!(result.unwrap().return_value, Value::I32(42), "{name}");
                    assert_eq!(
                        session.counters().allocation_units,
                        expected.allocations,
                        "{name}"
                    );
                    assert_eq!(session.counters().peak_call_depth, expected.depth, "{name}");
                } else {
                    assert!(
                        matches!(result.as_ref(),Err(error) if matches!(error.cause(),VmError::RuntimeError(error) if error.kind()==RuntimeErrorKind::ResourceLimitExceeded)),
                        "{name} {limit}: {result:?}"
                    );
                }
                assert_eq!(
                    session.counters().instruction_steps,
                    expected
                        .holes
                        .iter()
                        .find_map(|(cut, spent)| (*cut == limit).then_some(*spent))
                        .unwrap_or(limit),
                    "{name} {limit}"
                );
                let prefix: Vec<_> = expected
                    .effects
                    .iter()
                    .filter(|(_, step)| *step <= limit)
                    .map(|(label, step)| (label.to_string(), *step))
                    .collect();
                assert_eq!(*effects.borrow(), prefix, "{name} {limit}");
                drop(session);
                assert_eq!(vm.runtime().gc().active_roots(), 0, "{name}");
                assert_eq!(
                    vm.runtime().resources().counters().current_call_depth,
                    0,
                    "{name}"
                );
                assert!(!vm.runtime().is_quarantined());
                vm.runtime().collect_garbage().unwrap();
                assert_eq!(vm.runtime().gc().allocated_objects(), 0, "{name}");
            }
        }
    }
}
