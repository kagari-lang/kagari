#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use kagari_runtime::{
    RuntimeConfig,
    execution::JitExecutionStatus,
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::NativeCall,
        declarations::FunctionDecl,
    },
    value::Value,
    value_semantics::script_equal,
};

use crate::tests::execution::{
    common::{self, standard_runtime},
    native_fixtures,
};

use std::sync::{Arc, Mutex};

#[test]
fn source_artifact_and_jit_fallback_resolve_imports_to_registered_slots() {
    let bytecode =
        common::compile_logging_test_bytecode(r#"fn main() -> i32 { print("linked"); 7 }"#);
    for artifact in [false, true] {
        for jit in [false, true] {
            let module = if artifact {
                common::round_trip_bytecode(bytecode.clone())
            } else {
                bytecode.clone()
            };
            let mut runtime = standard_runtime(RuntimeConfig {
                ..Default::default()
            });
            let mut unrelated = ModuleBuilder::new("fixture", &Default::default()).unwrap();
            let entry = unrelated
                .define_function(FunctionDecl::new("unrelated"))
                .unwrap();
            unrelated
                .bind(entry, |_: &mut NativeCall<'_>| -> NativeResult<()> {
                    panic!("a log import must not invoke an unrelated native entry")
                })
                .unwrap();
            unrelated.finish().unwrap().install(&mut runtime).unwrap();
            let calls = Arc::new(Mutex::new(Vec::new()));
            let called = calls.clone();
            common::logging_module(move |cx, args| {
                let [Value::Str(id)] = args else {
                    panic!("log string")
                };
                called
                    .lock()
                    .unwrap()
                    .push(cx.runtime().gc().string(*id).unwrap().to_owned());
                Ok(Value::Unit)
            })
            .install(&mut runtime)
            .unwrap();
            let loaded = load_bytecode(&mut runtime, "linked", module).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            assert_eq!(loaded.bytecode.native_imports.len(), 1);
            let mut vm = runtime;
            let report = if jit {
                vm.execute_prepared_module(
                    &loaded,
                    ProgramEntry::Main,
                    &native_fixtures::unsupported(),
                    vm.execution_options(),
                )
                .unwrap()
            } else {
                vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
                    .unwrap()
            };
            assert_eq!(
                report
                    .return_value
                    .value(vm.gc())
                    .expect("retained execution result"),
                Value::I32(7)
            );
            assert_eq!(*calls.lock().unwrap(), vec!["linked".to_owned()]);
        }
    }
}

#[test]
fn jit_fallback_executes_foundation_bindings_deterministically() {
    let module = common::compile_test_bytecode(
        r#"
fn main() -> (u64, u64, i32) {
    val values = Vec::from([1, 2]);
    values.push(3);
    (values.len(), values.len() - 1u64, match values.pop() { Some(value) => value + 4, None => 0 })
}

"#,
    );
    let (runtime, loaded) = common::load_bytecode_program_with_runtime(
        standard_runtime(Default::default()),
        "jit_stdlib_fallback",
        module,
    );
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let prepared = native_fixtures::unsupported();

    let report = vm
        .execute_prepared_module(
            &loaded,
            ProgramEntry::Main,
            &prepared,
            vm.execution_options(),
        )
        .expect("unsupported JIT compilation should fall back");

    assert!(
        script_equal(
            vm.gc(),
            &(report
                .return_value
                .value(vm.gc())
                .expect("retained execution result")),
            &(vm.gc()
                .alloc_tuple(vec![Value::U64(3), Value::U64(2), Value::I32(7)])
                .unwrap())
        )
        .unwrap()
    );
    let jit = report.jit.expect("JIT attempt should be reported");
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics.len(), 1);
}
