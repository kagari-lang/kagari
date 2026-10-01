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

fn cases() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for count in [0, 1, 3, 5] {
        for shape in ["index", "object", "shared", "tuple", "named"] {
            let (item, initializer, check) = match shape {
                "index" => ("usize", "i", "values[i] == i"),
                "object" => ("Cell", "Cell{value:i}", "values[i].value == i"),
                "shared" => ("Cell", "shared", "values[i].value == 42usize"),
                "tuple" => (
                    "(usize,ArrayList<usize>)",
                    "(i,[i])",
                    "values[i][0] == i && values[i][1][0usize] == i",
                ),
                _ => ("usize", "i", "values[i] == i"),
            };
            let create = if shape == "named" {
                format!("ArrayList::from_fn({count}usize, |i| initialize(i))")
            } else {
                format!(
                    "build({{std::debug::print(\"count\");{count}usize}}, {{std::debug::print(\"closure\");|i|{{std::debug::print(\"init\");{initializer}}}}})"
                )
            };
            out.push((format!("{shape}_{count}"), format!(r#"
struct Cell {{var value:usize}}
fn build<T>(count:usize, initializer:fn(usize)->T)->ArrayList<T>{{ArrayList::from_fn(count,initializer)}}
fn initialize(i:usize)->usize{{std::debug::print("init");i}}
fn main()->i32{{
 val shared=Cell{{value:42usize}};
 val values:ArrayList<{item}> ={create};
 std::debug::print("created");
 std::debug::assert(values.len()=={count}usize,"length");
 for i in 0usize..{count}usize{{std::debug::assert({check},"item");}}
 std::debug::print("done");42
}}
"#)));
        }
    }
    out
}

mod baseline;
mod boundaries;

#[test]
fn initialization_preserves_every_budget_cut() {
    for (name, source) in cases() {
        let baseline = baseline::BASELINE
            .iter()
            .find(|case| case.name == name)
            .unwrap();
        let program = compile_test_bytecode(&source);
        assert_eq!(
            program.modules[program.root.index()]
                .native_imports
                .iter()
                .filter(|import| import.binding
                    == EngineNativeBinding::Intrinsic(RuntimePrimitive::ArrayListFromFn))
                .count(),
            1,
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
            let loaded = rt.load_program("array-init", program).unwrap();
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
                assert_eq!(
                    session.counters().instruction_steps,
                    limit,
                    "{name} {encoded}"
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
