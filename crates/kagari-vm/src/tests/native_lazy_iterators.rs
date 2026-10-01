use crate::{error::VmError, tests::common::compile_test_bytecode, vm::Vm};
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
mod baseline;
mod boundaries;
mod cases;
mod contracts;
mod foreign;
use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
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
#[test]
fn lazy_iterators_preserve_every_budget_cut() {
    check_budget_cases(|name| {
        !name.starts_with("windows_") && !name.starts_with("chunks_") && !name.contains("_limits_")
    });
}
#[test]
fn native_lazy_windows_preserve_every_budget_cut() {
    check_budget_cases(|name| name.starts_with("windows_") || name.starts_with("chunks_"));
}
#[test]
fn native_lazy_limits_preserve_every_budget_cut() {
    check_budget_cases(|name| name.contains("_limits_"));
}
fn check_budget_cases(selected: impl Fn(&str) -> bool) {
    check_budget_cases_with_transform(selected, |_| {});
}
fn check_budget_cases_with_transform(
    selected: impl Fn(&str) -> bool,
    transform: impl Fn(&mut BytecodeProgram),
) {
    let cases = cases::cases();
    assert_eq!(cases.len(), baseline::all().count());
    for (name, source) in cases.into_iter().filter(|(name, _)| selected(name)) {
        let baseline = baseline::all()
            .find(|baseline| baseline.name == name)
            .unwrap();
        let mut program = compile_test_bytecode(&source);
        transform(&mut program);
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
            let loaded = rt.load_program("lazy-baseline", program).unwrap();
            let mut vm = Vm::new(rt);
            for limit in 0..=baseline.steps {
                effects.borrow_mut().clear();
                base.set(vm.runtime().resources().counters().instruction_steps);
                let mut options = vm.runtime().execution_options();
                options.resources.max_instruction_steps = Some(limit);
                let session = vm.runtime().begin_execution(&loaded, options).unwrap();
                let result = vm.execute(&loaded, "main");
                if limit == baseline.steps {
                    assert_eq!(result.unwrap().return_value, Value::I32(42), "{name}");
                    assert_eq!(
                        session.counters().allocation_units,
                        baseline.allocations,
                        "{name} allocations"
                    );
                    assert_eq!(
                        session.counters().peak_call_depth,
                        baseline.depth,
                        "{name} depth"
                    );
                } else {
                    assert!(
                        matches!(result.as_ref(),Err(error) if matches!(error.cause(),VmError::RuntimeError(error) if error.kind()==RuntimeErrorKind::ResourceLimitExceeded)),
                        "{name}: {limit}: {result:?}"
                    );
                }
                let spent = baseline
                    .holes
                    .iter()
                    .find_map(|(cut, spent)| (*cut == limit).then_some(*spent))
                    .unwrap_or(limit);
                assert_eq!(
                    session.counters().instruction_steps,
                    spent,
                    "{name} {limit} {encoded}"
                );
                let expected: Vec<_> = baseline
                    .effects
                    .iter()
                    .filter(|(_, step)| *step <= limit)
                    .map(|(label, step)| (label.to_string(), *step))
                    .collect();
                assert_eq!(*effects.borrow(), expected, "{name} {limit} {encoded}");
                drop(session);
                assert_eq!(vm.runtime().gc().active_roots(), 0);
                assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
                assert!(!vm.runtime().is_quarantined());
                vm.runtime().collect_garbage().unwrap();
                assert_eq!(vm.runtime().gc().allocated_objects(), 0);
            }
        }
    }
}

#[test]
fn native_lazy_iterators_execute_all_checked_shapes() {
    use kagari_abi::callable::EngineNativeBinding;
    let mut failures = Vec::new();
    for (name, source) in cases::cases() {
        let program = compile_test_bytecode(&source);
        assert!(program.modules.iter().flat_map(|module| &module.native_imports).any(|import| matches!(import.binding, EngineNativeBinding::TraitDefault(operation) if operation.lazy())), "{name} native constructor");
        assert!(
            program
                .modules
                .iter()
                .flat_map(|module| &module.functions)
                .all(|function| !function.name.starts_with("$iterator_")),
            "{name} generated iterator"
        );
        let mut rt = runtime();
        rt.register_host_function(HostFunction::new(standard_log(), |_, _| Ok(Value::Unit)))
            .unwrap();
        let loaded = rt.load_program("lazy-shapes", program).unwrap();
        let mut vm = Vm::new(rt);
        let session = vm
            .runtime()
            .begin_execution(&loaded, vm.runtime().execution_options())
            .unwrap();
        match vm.execute(&loaded, "main") {
            Ok(result) if result.return_value == Value::I32(42) => {}
            result => failures.push(format!("{name}: {result:?}")),
        }
        drop(session);
        assert_eq!(vm.runtime().gc().active_roots(), 0, "{name}");
        assert_eq!(
            vm.runtime().resources().counters().current_call_depth,
            0,
            "{name}"
        );
        assert!(!vm.runtime().is_quarantined(), "{name}");
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(vm.runtime().gc().allocated_objects(), 0, "{name}");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
