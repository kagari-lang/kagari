//! Numeric source forms and source-free execution share the language contract.
use crate::interpreter_tests::common::round_trip_bytecode;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    executor::Executor,
    interpreter_tests::common::{compile_test_bytecode, test_runtime},
    native::conversion::context::ConversionContext,
};

use crate::error::RuntimeErrorKind;

#[test]
fn scalar_field_overflow_preserves_prior_effects_and_does_not_commit() {
    let program = compile_test_bytecode(include_str!(
        "../../tests/fixtures/execution/scalar-field-overflow.kgr"
    ));
    let decoded = round_trip_bytecode(program.clone());
    for program in [program, decoded] {
        let mut runtime = test_runtime(Default::default());
        let loaded = load_bytecode(&mut runtime, "field-overflow", program).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let function = |name| {
            loaded
                .bytecode
                .functions
                .iter()
                .find(|f| f.name == name)
                .unwrap()
                .id
        };
        let mut vm = runtime;
        let kept = vm
            .execute_module(&loaded, "main", vm.execution_options())
            .unwrap();
        let baseline_roots = vm.gc_stats().root_registrations;
        let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
        let value = cx.import(&kept.return_value).unwrap();
        let arguments = cx.argument_values(&[value]).unwrap();
        let result = Executor::new(cx.runtime_mut(), &loaded, function("update"), arguments)
            .unwrap()
            .run(|runtime, stack, result| stack.retain_result(runtime, result));
        assert!(matches!(result, Err(error) if error.kind() == RuntimeErrorKind::ScriptTrap));
        drop(cx);
        assert_eq!(vm.gc_stats().root_registrations, baseline_roots);
        vm.collect_garbage().unwrap();
        let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
        let value = cx.import(&kept.return_value).unwrap();
        let arguments = cx.argument_values(&[value]).unwrap();
        let checked = Executor::new(cx.runtime_mut(), &loaded, function("check"), arguments)
            .unwrap()
            .run(|runtime, stack, result| stack.retain_result(runtime, result))
            .unwrap();
        assert!(cx.decode::<bool>(&checked).unwrap());
        drop(cx);
        assert_eq!(vm.gc_stats().root_registrations, baseline_roots);
        assert_eq!(vm.resources().counters().current_call_depth, 0);
    }
}
