use crate::tests::execution::common::{compile_test_bytecode, load_bytecode_program};
use kagari_runtime::{module::entry::ProgramEntry, value_semantics::script_equal};
#[test]
fn executes_source_lowered_type_of_helper() {
    let (runtime, loaded) = load_bytecode_program(
        "test.kgr",
        compile_test_bytecode("fn main() -> String { type_of(7) }"),
    );
    let program = loaded;
    let loaded = runtime.select_program(&program).unwrap();
    let mut vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert!(
        script_equal(
            vm.gc(),
            &(report
                .return_value
                .value(vm.gc())
                .expect("retained execution result")),
            &(vm.gc().alloc_string("i32".to_owned()).unwrap())
        )
        .unwrap()
    );
}

#[test]
fn standard_collection_reflection_metadata_reports_runtime_categories() {
    let (runtime, loaded) = load_bytecode_program(
        "test.kgr",
        compile_test_bytecode(
            r#"use std::collections::{HashMap, HashSet};

        fn main() -> (String, String) {
            val map: HashMap<i32,i32> = HashMap::new();
            val set: HashSet<i32> = HashSet::new();
            (type_of(map), type_of(set))
        }
    "#,
        ),
    );
    let program = loaded;
    let loaded = runtime.select_program(&program).unwrap();

    let mut vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert!(
        script_equal(
            vm.gc(),
            &(report
                .return_value
                .value(vm.gc())
                .expect("retained execution result")),
            &(vm.gc()
                .alloc_tuple(vec![
                    vm.gc().alloc_string("map".to_owned()).unwrap(),
                    vm.gc().alloc_string("set".to_owned()).unwrap()
                ])
                .unwrap())
        )
        .unwrap()
    );
}
