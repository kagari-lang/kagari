use super::{numeric_baseline::BASELINE, numeric_cases::cases, route, runtime};
use crate::{Vm, VmError, tests::common::compile_test_bytecode};
use kagari_abi::{callable::EngineNativeBinding, standard::bindings::NativeProtocolMethod};
use kagari_common::host_interface::standard_log;
use kagari_runtime::{RuntimeErrorKind, host::HostFunction, value::Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[test]
fn direct_numeric_providers_preserve_every_budget_cut() {
    let cases = cases();
    assert_eq!(cases.len(), BASELINE.len());
    for (name, source) in cases {
        let baseline = BASELINE.iter().find(|case| case.name == name).unwrap();
        let program = compile_test_bytecode(&source);
        assert_eq!(
            program.modules[program.root.index()]
                .engine_imports
                .iter()
                .filter(|import| {
                    matches!(
                        import.binding,
                        EngineNativeBinding::Protocol(
                            NativeProtocolMethod::NumericSum | NativeProtocolMethod::NumericProduct
                        )
                    )
                })
                .count(),
            1,
            "{name}"
        );
        for encoded in [false, true] {
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let base = Rc::new(Cell::new(0));
            let root_base = base.clone();
            let mut runtime = runtime();
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |context, args| {
                    let Value::Str(label) = &args[0] else {
                        panic!("log label")
                    };
                    sink.borrow_mut().push((
                        label.clone(),
                        context.runtime().resources().counters().instruction_steps
                            - root_base.get(),
                    ));
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime
                .load_program("numeric-native", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(runtime);
            for limit in 0..=baseline.steps {
                effects.borrow_mut().clear();
                base.set(vm.runtime().resources().counters().instruction_steps);
                let mut options = vm.runtime().execution_options();
                options.resources.max_instruction_steps = Some(limit);
                let session = vm.runtime().begin_execution(&loaded, options).unwrap();
                let result = vm.execute(&loaded, "main");
                if limit == baseline.steps {
                    assert_eq!(result.unwrap().return_value, Value::I32(42), "{name}");
                } else {
                    assert!(
                        matches!(result,Err(VmError::RuntimeError(error)) if error.kind()==RuntimeErrorKind::ResourceLimitExceeded),
                        "{name} {limit} {encoded}"
                    );
                }
                assert_eq!(session.counters().instruction_steps, limit, "{name}");
                let expected: Vec<_> = baseline
                    .effects
                    .iter()
                    .filter(|(_, step)| *step <= limit)
                    .map(|(label, step)| (label.to_string(), *step))
                    .collect();
                assert_eq!(*effects.borrow(), expected, "{name} {limit} {encoded}");
                assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
                assert!(!vm.runtime().is_quarantined());
                drop(session);
                assert_eq!(vm.runtime().gc().active_roots(), 0, "{name} {limit}");
                vm.runtime().collect_garbage().unwrap();
                assert_eq!(vm.runtime().gc().allocated_objects(), 0);
            }
        }
    }
}
