//! Numeric source forms and source-free execution share the language contract.
use crate::interpreter_tests::common::round_trip_bytecode;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    executor::Executor,
    interpreter_tests::common::{compile_test_bytecode, test_runtime},
};

use crate::{error::RuntimeErrorKind, value::Value};

#[test]
fn scalar_field_overflow_preserves_prior_effects_and_does_not_commit() {
    let program = compile_test_bytecode(
        "struct State { var before: i32, var value: i8, var after: i32 }
         pub fn main() -> State { State { before: 0, value: 127i8, after: 0 } }
         fn update(s: State) { s.before += 1; s.value += 1i8; s.after += 1; }
         fn check(s: State) -> bool { s.before == 1 && s.value == 127i8 && s.after == 0 }",
    );
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
        let vm = runtime;
        let kept = vm
            .execute_module(&loaded, "main", vm.execution_options())
            .unwrap();
        let value = kept.return_value.value(vm.gc()).unwrap();
        let result = Executor::new(&vm, &loaded, function("update"), &[value])
            .unwrap()
            .run();
        assert!(matches!(result, Err(error) if error.kind() == RuntimeErrorKind::ScriptTrap));
        vm.collect_garbage().unwrap();
        assert_eq!(
            Executor::new(&vm, &loaded, function("check"), &[value])
                .unwrap()
                .run()
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(vm.resources().counters().current_call_depth, 0);
    }
}
