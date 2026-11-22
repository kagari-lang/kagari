use crate::tests::execution::common::{
    compile_print_test_bytecode, compile_test_bytecode, load_bytecode_program, load_test_module,
    print_module, round_trip_bytecode, standard_runtime,
};
use kagari_runtime::{
    RuntimeConfig, host::HostError, module::entry::ProgramEntry,
    native::conversion::context::ConversionContext,
};
use std::sync::{Arc, Mutex};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;

#[test]
fn executes_explicitly_registered_print_and_println() {
    let messages = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&messages);

    let mut runtime = standard_runtime(RuntimeConfig::default());
    print_module(move |cx, message, suffix| {
        cx.read_string(message, |text| {
            sink.lock()
                .expect("message sink should lock")
                .push(format!("{text}{suffix}"));
        })
        .map_err(|error| HostError::new(error.to_string()))
    })
    .install(&mut runtime)
    .expect("host function should register");
    let bytecode = compile_print_test_bytecode(include_str!(
        "../../../tests/fixtures/execution/std-io-output.kgr"
    ));
    let loaded =
        load_bytecode(&mut runtime, "print.kgr", bytecode).expect("print module should load");
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();

    let mut vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    ConversionContext::new(&mut vm, &loaded)
        .unwrap()
        .decode::<()>(&report.return_value)
        .unwrap();
    assert_eq!(
        *messages.lock().expect("message sink should lock"),
        ["hello", "world\n"]
    );
}

#[test]
fn executes_bytecode_foundation_collection_bindings() {
    let program = compile_test_bytecode(include_str!(
        "../../../tests/fixtures/execution/foundation-collections.kgr"
    ));
    let decoded = round_trip_bytecode(program);
    let (mut runtime, program) = load_bytecode_program("standard_collections.kbc", decoded);
    let loaded = runtime.select_program(&program).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let report = runtime
        .execute_module(&loaded, ProgramEntry::Main, runtime.execution_options())
        .expect("vm should execute");

    assert_eq!(
        ConversionContext::new(&mut runtime, &loaded)
            .unwrap()
            .decode::<(u64, bool, u64, bool)>(&report.return_value)
            .unwrap(),
        (1, true, 1, true),
    );
    drop(report);
    runtime.collect_garbage().unwrap();
    let after = runtime.gc_stats();
    assert_eq!(after.objects, baseline.objects);
    assert_eq!(after.root_registrations, baseline.root_registrations);
}

#[test]
fn concrete_conversions_forward_to_intrinsic_and_native_methods() {
    let program = compile_test_bytecode(include_str!(
        "../../../tests/fixtures/execution/conversion-forwarding.kgr"
    ));
    let (mut runtime, loaded) =
        load_bytecode_program("conversions.kbc", round_trip_bytecode(program));
    let result: bool = runtime
        .execute_typed(&loaded, ProgramEntry::Main, (), &Default::default())
        .unwrap();
    assert!(result);
}

#[test]
fn standard_vec_mutation_updates_shared_sequence_handles() {
    let (runtime, loaded) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/vec-alias.kgr"
    ));
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert_eq!(report.return_value.as_u64(), Some(3));
}

#[test]
fn struct_field_updates_mutate_shared_struct_handle_in_place() {
    let program = compile_test_bytecode(include_str!(
        "../../../tests/fixtures/execution/struct-alias.kgr"
    ));
    let (mut runtime, program) =
        load_bytecode_program("struct-alias.kbc", round_trip_bytecode(program));
    let loaded = runtime.select_program(&program).unwrap();
    let report = runtime
        .execute_module(&loaded, ProgramEntry::Main, runtime.execution_options())
        .expect("vm should execute");

    assert_eq!(report.return_value.as_i32(), Some(9));
}
