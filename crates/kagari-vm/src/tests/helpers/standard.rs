use super::*;
use crate::tests::common::load_bytecode_program;
use crate::tests::common::round_trip_bytecode;
use crate::tests::common::{compile_logging_test_bytecode, logging_module};
use kagari_runtime::module::entry::ProgramEntry;

use kagari_runtime::value_semantics::script_equal;

#[test]
fn executes_source_lowered_declared_host_log() {
    let messages = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&messages);

    let mut runtime = host_runtime();
    logging_module(move |cx, args| {
        let Some(Value::Str(message)) = args.first() else {
            return Err(HostError::new("host.log expects one string argument"));
        };
        sink.lock()
            .expect("message sink should lock")
            .push(cx.runtime().gc().string(*message).unwrap().to_owned());
        Ok(Value::Unit)
    })
    .install(&mut runtime)
    .expect("host function should register");
    let bytecode = compile_logging_test_bytecode(r#"use host::log; fn main() { log("hello"); }"#);
    let loaded = runtime
        .load_program("print.kgr", bytecode)
        .expect("print module should load");

    let vm = Vm::new(runtime);
    let report = vm
        .execute(&loaded, ProgramEntry::Main)
        .expect("vm should execute");

    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::Unit
    );
    assert_eq!(
        *messages.lock().expect("message sink should lock"),
        vec!["hello".to_string()]
    );
}

#[test]
fn executes_bytecode_foundation_collection_bindings() {
    let program = compile_test_bytecode(
        r#"use std::collections::{HashMap, HashSet};

fn main()->(u64,bool,u64,bool){
 val map:HashMap<String,i32> =HashMap::new();map.insert("k",7);
 val set:HashSet<String> =HashSet::new();set.insert("k");
 (map.len(),map.contains_key("k"),map.len(),set.contains("k"))
}
"#,
    );
    let decoded = round_trip_bytecode(program);
    let (runtime, loaded) = load_bytecode_program("standard_collections.kbc", decoded);
    let vm = Vm::new(runtime);
    let report = vm
        .execute(&loaded, ProgramEntry::Main)
        .expect("vm should execute");

    assert!(
        script_equal(
            vm.runtime().gc(),
            &(report
                .return_value
                .value(vm.runtime().gc())
                .expect("retained execution result")),
            &(vm.runtime()
                .gc()
                .alloc_tuple(vec![
                    Value::U64(1),
                    Value::Bool(true),
                    Value::U64(1),
                    Value::Bool(true),
                ])
                .unwrap())
        )
        .unwrap()
    );
}

#[test]
fn standard_vec_mutation_updates_shared_sequence_handles() {
    let (runtime, loaded) = load_test_module(
        r#"
fn main() -> u64 {
    val values = Vec::from([1, 2]);
    val alias = values;
    values.push(3);
    alias.len()
}
"#,
    );
    let vm = Vm::new(runtime);
    let report = vm
        .execute(&loaded, ProgramEntry::Main)
        .expect("vm should execute");

    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::U64(3)
    );
}

#[test]
fn struct_field_updates_mutate_shared_struct_handle_in_place() {
    let (runtime, loaded) = load_reflection_test_module(
        r#"
struct Point { var x: i32 }

fn main() -> i32 {
    val point = Point { x: 1 };
    val alias = point;
    set_field(point, "x", 9);
    alias.x
}
"#,
    );
    let vm = Vm::new(runtime);
    let report = vm
        .execute(&loaded, ProgramEntry::Main)
        .expect("vm should execute");

    assert_eq!(
        report
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(9)
    );
}
