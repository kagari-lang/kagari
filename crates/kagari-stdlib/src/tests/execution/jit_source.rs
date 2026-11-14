#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use kagari_runtime::{
    RuntimeConfig,
    execution::JitExecutionStatus,
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::NativeCall,
        conversion::context::ConversionContext, declarations::FunctionDecl,
    },
    value::view::ValueView,
};

use crate::tests::execution::{
    common::{self, standard_runtime},
    native_fixtures,
};

use std::sync::{Arc, Mutex};

#[test]
fn source_artifact_and_jit_fallback_resolve_imports_to_registered_slots() {
    let bytecode = common::compile_logging_test_bytecode(include_str!(
        "../../../tests/fixtures/execution/linked-log.kgr"
    ));
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
                let [message] = args else {
                    panic!("log string")
                };
                cx.read_string(message, |text| called.lock().unwrap().push(text.to_owned()))
                    .unwrap();
                Ok(ValueView::unit())
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
            assert_eq!(report.return_value.as_i32(), Some(7));
            assert_eq!(*calls.lock().unwrap(), vec!["linked".to_owned()]);
        }
    }
}

#[test]
fn jit_fallback_executes_foundation_bindings_deterministically() {
    let module = common::compile_test_bytecode(include_str!(
        "../../../tests/fixtures/execution/vec-fallback.kgr"
    ));
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

    assert_eq!(
        ConversionContext::new(&mut vm, &loaded)
            .unwrap()
            .decode::<(u64, u64, i32)>(&report.return_value)
            .unwrap(),
        (3, 2, 7),
    );
    let jit = report.jit.expect("JIT attempt should be reported");
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics.len(), 1);
}
