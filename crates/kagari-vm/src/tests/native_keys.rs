use crate::{Vm, VmError, tests::common::compile_test_bytecode};
use kagari_abi::{callable::EngineNativeBinding, standard::RuntimePrimitive};
use kagari_bytecode::KbcArtifact;
use kagari_common::host_interface::standard_log;
use kagari_runtime::{
    CapabilitySet, HostExposurePolicy, LanguageProfile, Runtime, RuntimeConfig, RuntimeErrorKind,
    SecurityContext, gc::GcHeapConfig, host::HostFunction, value::Value,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

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

mod baseline;
mod boundaries;
mod cases;
mod foreign;
#[test]
fn keys_preserve_every_budget_cut() {
    let cases = cases::cases();
    assert_eq!(cases.len(), baseline::all().count());
    for (name, source) in cases {
        let baseline = baseline::all().find(|case| case.name == name).unwrap();
        let program = compile_test_bytecode(&source);
        let operation = cases::OPERATIONS
            .iter()
            .find(|op| {
                name.starts_with(&format!("{op}_scalar_"))
                    || name.starts_with(&format!("{op}_nominal_"))
                    || ["tuple", "option", "choice", "identity"]
                        .iter()
                        .any(|shape| name.starts_with(&format!("{op}_{shape}_")))
            })
            .unwrap();
        let binding = EngineNativeBinding::Intrinsic(match *operation {
            "get" => RuntimePrimitive::MapGet,
            "contains_key" => RuntimePrimitive::MapContainsKey,
            "map_insert" => RuntimePrimitive::MapInsert,
            "map_remove" => RuntimePrimitive::MapRemove,
            "contains" => RuntimePrimitive::SetContains,
            "set_insert" => RuntimePrimitive::SetInsert,
            "set_remove" => RuntimePrimitive::SetRemove,
            "get_or_insert_with" => RuntimePrimitive::MapGetOrInsertWith,
            "update" => RuntimePrimitive::MapUpdate,
            _ => panic!(),
        });
        assert!(
            program
                .modules
                .iter()
                .flat_map(|m| &m.native_imports)
                .any(|i| i.binding == binding),
            "{name}"
        );
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
            rt.register_host_function(HostFunction::new(standard_log(), move |ctx, args| {
                let Value::Str(label) = &args[0] else {
                    panic!()
                };
                sink.borrow_mut().push((
                    label.clone(),
                    ctx.runtime().resources().counters().instruction_steps - root_base.get(),
                ));
                Ok(Value::Unit)
            }))
            .unwrap();
            let loaded = rt.load_program("native-keys", program).unwrap();
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
                } else {
                    assert!(
                        matches!(result,Err(VmError::RuntimeError(error)) if error.kind()==RuntimeErrorKind::ResourceLimitExceeded),
                        "{name} {limit} {encoded}"
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
                assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
                assert!(!vm.runtime().is_quarantined());
                drop(session);
                assert_eq!(vm.runtime().gc().active_roots(), 0);
                vm.runtime().collect_garbage().unwrap();
                assert_eq!(vm.runtime().gc().allocated_objects(), 0);
            }
        }
    }
}
