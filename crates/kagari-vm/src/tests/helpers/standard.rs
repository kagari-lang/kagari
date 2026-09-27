use super::*;

#[test]
fn executes_source_lowered_print_builtin() {
    let messages = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&messages);

    let mut runtime = host_runtime();
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::standard_log(),
            move |_, args| {
                let Some(Value::Str(message)) = args.first() else {
                    return Err(HostError::new("host.log expects one string argument"));
                };
                sink.lock()
                    .expect("message sink should lock")
                    .push(message.clone());
                Ok(Value::Unit)
            },
        ))
        .expect("host function should register");
    let bytecode = compile_test_bytecode(r#"fn main() { print("hello"); }"#);
    let loaded = runtime
        .load_program(
            "print.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
        .expect("print module should load");

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::Unit);
    assert_eq!(
        *messages.lock().expect("message sink should lock"),
        vec!["hello".to_string()]
    );
}

#[test]
fn executes_source_lowered_standard_intrinsics() {
    let (runtime, loaded) = load_test_module(
        r#"
fn main() -> (usize, usize, i32, bool) {
    val values = [1, 2];
    values.push(3);
    val popped = values.pop();
    (values.len(), "kagari".len_chars(), std::math::clamp(4, 1, 3), popped.is_some())
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![
            Value::U64(2),
            Value::U64(6),
            Value::I32(3),
            Value::Bool(true),
        ])
    );
}

#[test]
fn executes_source_standard_collection_string_math_and_debug_modules() {
    let (runtime, loaded) = load_test_module(
        r#"
fn main() -> (usize, bool, usize, usize, usize, usize, usize, usize, bool, bool, bool, i32) {
    val map: LinkedHashMap<String, i32> = LinkedHashMap::new();
    map.insert("a", 1);
    map.insert("b", 2);
    val keys = map.keys();
    val values = map.values();
    val entries = map.entries();

    val set: LinkedHashSet<String> = LinkedHashSet::new();
    set.insert("x");
    set.insert("y");
    val set_items = set.to_array();
    val union = set.union(set);

    std::debug::assert(map.contains_key("a"), "map key must exist");
    (
        map.len(),
        map.contains_key("a"),
        keys.len(),
        values.len(),
        entries.len(),
        set.len(),
        set_items.len(),
        union.len(),
        std::string::String::contains("kagari", "gar"),
        std::string::String::starts_with("kagari", "ka"),
        std::string::String::ends_with("kagari", "ri"),
        std::math::max(1, 2)
    )
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![
            Value::U64(2),
            Value::Bool(true),
            Value::U64(2),
            Value::U64(2),
            Value::U64(2),
            Value::U64(2),
            Value::U64(2),
            Value::U64(2),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::I32(2),
        ])
    );
}

#[test]
fn executes_bytecode_standard_collection_intrinsics() {
    let (runtime, loaded) = load_bytecode_module(
        "standard_collections.kbc",
        test_function_module(
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::Str("k".to_owned()),
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(1)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::LinkedHashMapNew),
                    args: vec![],
                },
                BytecodeInstruction::LoadConst {
                    dst: Register::new(2),
                    constant: ConstantOperand::I32(7),
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(3)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::MapInsert),
                    args: vec![Register::new(1), Register::new(0), Register::new(2)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(4)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::MapLen),
                    args: vec![Register::new(1)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(5)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::MapContainsKey),
                    args: vec![Register::new(1), Register::new(0)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(6)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::MapKeysStorage),
                    args: vec![Register::new(1)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(7)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayLen),
                    args: vec![Register::new(6)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(8)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::LinkedHashSetNew),
                    args: vec![],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(9)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::SetInsert),
                    args: vec![Register::new(8), Register::new(0)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(10)),
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::SetContains),
                    args: vec![Register::new(8), Register::new(0)],
                },
                BytecodeInstruction::MakeTuple {
                    dst: Register::new(11),
                    elements: vec![
                        Register::new(4),
                        Register::new(5),
                        Register::new(7),
                        Register::new(10),
                    ],
                },
                BytecodeInstruction::Return(Some(Register::new(11))),
            ],
            ValueType::HeapObject,
            vec![
                ValueType::Str,
                ValueType::HeapObject,
                ValueType::I32,
                ValueType::HeapObject,
                ValueType::U64,
                ValueType::Bool,
                ValueType::HeapObject,
                ValueType::U64,
                ValueType::HeapObject,
                ValueType::HeapObject,
                ValueType::Bool,
                ValueType::HeapObject,
            ],
        ),
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![
            Value::U64(1),
            Value::Bool(true),
            Value::U64(1),
            Value::Bool(true),
        ])
    );
}

#[test]
fn standard_intrinsics_reject_invalid_hash_keys_before_publication() {
    let bytecode = test_function_module(
        "main",
        vec![
            BytecodeInstruction::Call {
                dst: Some(Register::new(0)),
                callee: CallTarget::StandardIntrinsic(StandardIntrinsic::LinkedHashMapNew),
                args: vec![],
            },
            BytecodeInstruction::LoadConst {
                dst: Register::new(1),
                constant: ConstantOperand::I32(1),
            },
            BytecodeInstruction::LoadConst {
                dst: Register::new(2),
                constant: ConstantOperand::F32(1.0),
            },
            BytecodeInstruction::Call {
                dst: Some(Register::new(3)),
                callee: CallTarget::StandardIntrinsic(StandardIntrinsic::MapInsert),
                args: vec![Register::new(0), Register::new(2), Register::new(1)],
            },
            BytecodeInstruction::Return(Some(Register::new(3))),
        ],
        ValueType::HeapObject,
        vec![
            ValueType::HeapObject,
            ValueType::I32,
            ValueType::F32,
            ValueType::HeapObject,
        ],
    );
    let mut runtime = Runtime::default();

    let error = runtime
        .load_program(
            "standard_invalid_key.kbc",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
        .expect_err("float map key should reject before publication");

    assert!(matches!(error.kind(), RuntimeErrorKind::ModuleValidation));
    assert!(error.message().contains("hash-key"));
}

#[test]
fn standard_intrinsic_execution_observes_resource_limits() {
    let bytecode = compile_test_bytecode(
        r#"
fn main() -> usize {
    val values = [1, 2];
    values.push(3);
    values.len()
}
"#,
    );
    let mut runtime = Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_instruction_steps: Some(1),
            ..ResourcePolicy::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded = runtime
        .load_program(
            "standard_resource_limit.kgr",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
        .expect("module should load");
    let mut vm = Vm::new(runtime);
    let error = vm
        .execute(&loaded, "main")
        .expect_err("standard intrinsic program should hit resource limit");

    assert!(matches!(
        error,
        crate::VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::ResourceLimitExceeded
    ));
}

#[test]
fn executes_source_lowered_string_len_chars_method() {
    let (runtime, loaded) = load_test_module(
        r#"
fn main() -> usize {
    "kagari".len_chars()
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::U64(6));
}

#[test]
fn standard_array_mutation_updates_shared_array_handles() {
    let (runtime, loaded) = load_test_module(
        r#"
fn main() -> usize {
    val values = [1, 2];
    val alias = values;
    values.push(3);
    alias.len()
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::U64(3));
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
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(9));
}
