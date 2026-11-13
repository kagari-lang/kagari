use crate::tests::execution::common::{
    compile_logging_test_bytecode, compile_test_bytecode, load_bytecode_program, load_test_module,
    logging_module, round_trip_bytecode, standard_runtime,
};
use kagari_runtime::{
    RuntimeConfig, host::HostError, module::entry::ProgramEntry,
    native::conversion::context::ConversionContext, value::view::ValueView,
};
use std::sync::{Arc, Mutex};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;

#[test]
fn executes_source_lowered_declared_host_log() {
    let messages = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&messages);

    let mut runtime = standard_runtime(RuntimeConfig::default());
    logging_module(move |cx, args| {
        let Some(message) = args.first() else {
            return Err(HostError::new("host.log expects one string argument"));
        };
        cx.read_string(message, |text| {
            sink.lock()
                .expect("message sink should lock")
                .push(text.to_owned());
        })
        .map_err(|error| HostError::new(error.to_string()))?;
        Ok(ValueView::unit())
    })
    .install(&mut runtime)
    .expect("host function should register");
    let bytecode = compile_logging_test_bytecode(include_str!(
        "../../../tests/fixtures/execution/host-log.kgr"
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
        vec!["hello".to_string()]
    );
}

#[test]
fn executes_bytecode_foundation_collection_bindings() {
    let program = compile_test_bytecode(include_str!(
        "../../../tests/fixtures/execution/foundation-collections.kgr"
    ));
    let decoded = round_trip_bytecode(program);
    let (runtime, loaded) = load_bytecode_program("standard_collections.kbc", decoded);
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert_eq!(
        ConversionContext::new(&mut vm, &loaded)
            .unwrap()
            .decode::<(u64, bool, u64, bool)>(&report.return_value)
            .unwrap(),
        (1, true, 1, true),
    );
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
    let (runtime, loaded) = load_bytecode_program(
        "test.kgr",
        compile_test_bytecode(include_str!(
            "../../../tests/fixtures/execution/struct-alias.kgr"
        )),
    );
    let program = loaded;
    let loaded = runtime.select_program(&program).unwrap();
    let mut vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert_eq!(report.return_value.as_i32(), Some(9));
}
