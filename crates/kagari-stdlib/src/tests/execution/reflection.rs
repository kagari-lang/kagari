use crate::tests::execution::common::{compile_test_bytecode, load_bytecode_program};
use kagari_runtime::{module::entry::ProgramEntry, native::conversion::context::ConversionContext};
#[test]
fn executes_source_lowered_type_of_helper() {
    let (runtime, loaded) = load_bytecode_program(
        "test.kgr",
        compile_test_bytecode(include_str!(
            "../../../tests/fixtures/execution/reflection-scalar-category.kgr"
        )),
    );
    let program = loaded;
    let loaded = runtime.select_program(&program).unwrap();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert_eq!(
        ConversionContext::new(&mut vm, &loaded)
            .unwrap()
            .decode::<String>(&report.return_value)
            .unwrap(),
        "i32"
    );
    drop(report);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn standard_collection_reflection_metadata_reports_runtime_categories() {
    let (runtime, loaded) = load_bytecode_program(
        "test.kgr",
        compile_test_bytecode(include_str!(
            "../../../tests/fixtures/execution/reflection-collection-categories.kgr"
        )),
    );
    let program = loaded;
    let loaded = runtime.select_program(&program).unwrap();

    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert_eq!(
        ConversionContext::new(&mut vm, &loaded)
            .unwrap()
            .decode::<(String, String)>(&report.return_value)
            .unwrap(),
        ("map".to_owned(), "set".to_owned())
    );
    drop(report);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}
