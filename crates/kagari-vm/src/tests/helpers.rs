use kagari_ir::builtin::iterable;
use kagari_ir::bytecode::{
    BinaryOp, BuiltinMethod, BytecodeFunction, BytecodeInstruction, BytecodeModule, CallTarget,
    ConstantOperand, FunctionRef, PathId, PathRecord, Register, RuntimeHelper, StructFieldInit,
};
use kagari_ir::module::ValueType;
use std::sync::{Arc, Mutex};

use kagari_runtime::{
    AbiFingerprint, CapabilitySet, FieldMetadataId, HostObjectId, HostPathAdapter,
    HostPathDescriptorRegistration, HostPathSegment, HostReflectionPolicy, HostSchemaEpoch,
    HostTypeOwnership, HostTypeRegistration, LanguageProfile, PathAccess, Runtime, RuntimeConfig,
    RuntimeErrorKind, SecurityContext, TypeKind, TypeRegistration,
    host::{HostError, HostFunction},
    value::Value,
};

use crate::Vm;
use crate::tests::common::{
    compile_test_bytecode, load_bytecode_module, load_bytecode_module_with_runtime,
    load_test_module, test_function_module,
};

fn reflection_runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_reflection: true,
                allow_reflection_write: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                reflection_read: true,
                reflection_write: true,
                ..CapabilitySet::default()
            },
        },
        ..RuntimeConfig::default()
    })
}

fn load_reflection_bytecode_module(
    name: &str,
    bytecode: BytecodeModule,
) -> (Runtime, kagari_runtime::LoadedModule) {
    load_bytecode_module_with_runtime(reflection_runtime(), name, bytecode)
}

fn load_reflection_test_module(source_text: &str) -> (Runtime, kagari_runtime::LoadedModule) {
    load_reflection_bytecode_module("test.kgr", compile_test_bytecode(source_text))
}

fn register_vm_host_path_runtime(access: PathAccess) -> (Runtime, Arc<Mutex<i32>>) {
    register_vm_host_path_runtime_with_capabilities(access, CapabilitySet::default())
}

fn register_vm_host_path_runtime_with_capabilities(
    access: PathAccess,
    capability_requirements: CapabilitySet,
) -> (Runtime, Arc<Mutex<i32>>) {
    let mut runtime = Runtime::default();
    let i32_id = runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(1),
            ..TypeRegistration::new("i32", TypeKind::Primitive)
        })
        .unwrap();
    let mut host_type = HostTypeRegistration::new("game.Player", "game.Player");
    host_type.ownership = HostTypeOwnership::HostRoot;
    host_type.path_access = PathAccess::ReadWrite;
    host_type.reflection = HostReflectionPolicy::Hidden;
    host_type.abi_fingerprint = AbiFingerprint(2);
    let player_id = runtime.register_host_type(host_type).unwrap();
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(
            "host.player",
            vec![],
            "Player",
            move |_| Ok(Value::HostRoot(root)),
        ))
        .unwrap();
    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegment::Field {
                name: "hp".to_owned(),
                field_id: FieldMetadataId::new(0),
                owner_type: player_id,
                result_type: i32_id,
                access,
                abi_fingerprint: AbiFingerprint(3),
            }],
            access,
            schema_epoch: HostSchemaEpoch::new(0),
            abi_fingerprint: AbiFingerprint(4),
            capability_requirements,
        })
        .unwrap();
    assert_eq!(descriptor_id.index(), 0);

    let hp = Arc::new(Mutex::new(10));
    let read_hp = Arc::clone(&hp);
    let write_hp = Arc::clone(&hp);
    runtime
        .register_host_path_adapter(
            descriptor_id,
            HostPathAdapter::new()
                .with_read(move |_| Ok(Value::I32(*read_hp.lock().unwrap())))
                .with_write(move |_, value| {
                    let Value::I32(value) = value else {
                        return Err(HostError::new("hp expects i32"));
                    };
                    *write_hp.lock().unwrap() = value;
                    Ok(())
                }),
        )
        .unwrap();

    (runtime, hp)
}

fn path_module(
    name: &str,
    instructions: Vec<BytecodeInstruction>,
    return_type: ValueType,
) -> BytecodeModule {
    let instructions_constants = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::LoadConst { constant, .. } => Some(constant.clone()),
            _ => None,
        })
        .collect();
    let metadata = kagari_ir::bytecode::FunctionMetadata {
        return_type,
        registers: vec![
            ValueType::HeapObject,
            ValueType::I32,
            ValueType::I32,
            ValueType::I32,
            ValueType::I32,
            ValueType::HeapObject,
        ],
        ..Default::default()
    };
    BytecodeModule {
        module_init: None,
        module_slots: vec![],
        constants: instructions_constants,
        types: vec![ValueType::Unit, ValueType::HeapObject, ValueType::I32],
        paths: vec![PathRecord {
            id: PathId::new(0),
            root_ty: ValueType::HeapObject,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "game.Player.hp".to_owned(),
        }],
        function_table: vec![kagari_ir::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            name: name.to_owned(),
            params: metadata.params.clone(),
            return_type: metadata.return_type,
            effects: metadata.effects,
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            name: name.to_owned(),
            parameter_count: 0,
            register_count: metadata.registers.len() as u16,
            local_count: 0,
            metadata,
            instructions,
        }],
        ..Default::default()
    }
}

#[test]
fn executes_runtime_host_helper_call() {
    let mut runtime = Runtime::default();
    runtime
        .register_host_function(HostFunction::new(
            "host.add_i32",
            vec![],
            "i32",
            |args| match args {
                [Value::I32(lhs), Value::I32(rhs)] => Ok(Value::I32(lhs + rhs)),
                _ => Err(HostError::new("host.add_i32 expects two i32 arguments")),
            },
        ))
        .expect("host function should register");

    let loaded = runtime
        .load_module(
            "helper.kbc",
            test_function_module(
                "main",
                vec![
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(0),
                        constant: ConstantOperand::I32(40),
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(1),
                        constant: ConstantOperand::I32(2),
                    },
                    BytecodeInstruction::Call {
                        dst: Some(Register::new(2)),
                        callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                            "host.add_i32".to_owned(),
                        )),
                        args: vec![Register::new(0), Register::new(1)],
                    },
                    BytecodeInstruction::Return(Some(Register::new(2))),
                ],
                ValueType::I32,
                vec![ValueType::I32, ValueType::I32, ValueType::I32],
            ),
        )
        .expect("helper module should load");

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(42));
}

#[test]
fn executes_typed_path_read_set_modify_and_view_instructions() {
    let (mut runtime, hp) = register_vm_host_path_runtime(PathAccess::ReadWrite);
    let loaded = runtime
        .load_module(
            "paths.kbc",
            path_module(
                "main",
                vec![
                    BytecodeInstruction::Call {
                        dst: Some(Register::new(0)),
                        callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                            "host.player".to_owned(),
                        )),
                        args: vec![],
                    },
                    BytecodeInstruction::ReadPath {
                        dst: Register::new(1),
                        root_or_view: Register::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(2),
                        constant: ConstantOperand::I32(5),
                    },
                    BytecodeInstruction::SetPath {
                        root_or_view: Register::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                        value: Register::new(2),
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(3),
                        constant: ConstantOperand::I32(2),
                    },
                    BytecodeInstruction::ModifyPath {
                        dst: Some(Register::new(4)),
                        root_or_view: Register::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                        op: BinaryOp::Add,
                        value: Register::new(3),
                    },
                    BytecodeInstruction::MakePathView {
                        dst: Register::new(5),
                        root_or_view: Register::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                    },
                    BytecodeInstruction::Return(Some(Register::new(4))),
                ],
                ValueType::I32,
            ),
        )
        .unwrap();

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").unwrap();

    assert_eq!(report.return_value, Value::I32(7));
    assert_eq!(*hp.lock().unwrap(), 7);
    assert_eq!(vm.runtime().host_dirty_paths().len(), 2);
}

#[test]
fn typed_path_instruction_failures_are_runtime_typed_path_errors() {
    let (mut runtime, _) = register_vm_host_path_runtime(PathAccess::ReadOnly);
    let loaded = runtime
        .load_module(
            "readonly_path.kbc",
            path_module(
                "main",
                vec![
                    BytecodeInstruction::Call {
                        dst: Some(Register::new(0)),
                        callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                            "host.player".to_owned(),
                        )),
                        args: vec![],
                    },
                    BytecodeInstruction::LoadConst {
                        dst: Register::new(1),
                        constant: ConstantOperand::I32(2),
                    },
                    BytecodeInstruction::SetPath {
                        root_or_view: Register::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                        value: Register::new(1),
                    },
                    BytecodeInstruction::Return(None),
                ],
                ValueType::Unit,
            ),
        )
        .unwrap();

    let mut vm = Vm::new(runtime);
    let error = vm.execute(&loaded, "main").unwrap_err();

    assert!(matches!(
        error,
        crate::VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::TypedPathValidation
    ));
}

#[test]
fn typed_path_helpers_enforce_runtime_capability_boundary() {
    let (mut runtime, _) = register_vm_host_path_runtime_with_capabilities(
        PathAccess::ReadWrite,
        CapabilitySet {
            fs_read: true,
            ..CapabilitySet::default()
        },
    );
    let loaded = runtime
        .load_module(
            "path_capability.kbc",
            path_module(
                "main",
                vec![
                    BytecodeInstruction::Call {
                        dst: Some(Register::new(0)),
                        callee: CallTarget::RuntimeHelper(RuntimeHelper::HostFunction(
                            "host.player".to_owned(),
                        )),
                        args: vec![],
                    },
                    BytecodeInstruction::ReadPath {
                        dst: Register::new(1),
                        root_or_view: Register::new(0),
                        path: PathId::new(0),
                        dynamic_args: vec![],
                    },
                    BytecodeInstruction::Return(Some(Register::new(1))),
                ],
                ValueType::I32,
            ),
        )
        .unwrap();

    let mut vm = Vm::new(runtime);
    let error = vm.execute(&loaded, "main").unwrap_err();

    assert!(matches!(
        error,
        crate::VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("fs_read")
    ));
}

#[test]
fn executes_runtime_reflect_type_of_helper() {
    let (runtime, loaded) = load_reflection_bytecode_module(
        "reflect_type.kbc",
        test_function_module(
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::I32(7),
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(1)),
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectTypeOf),
                    args: vec![Register::new(0)],
                },
                BytecodeInstruction::Return(Some(Register::new(1))),
            ],
            ValueType::Str,
            vec![ValueType::I32, ValueType::Str],
        ),
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::Str("i32".to_owned()));
}

#[test]
fn runtime_reflection_helpers_require_runtime_capability() {
    let (runtime, loaded) = load_bytecode_module(
        "reflect_denied.kbc",
        test_function_module(
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::I32(7),
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(1)),
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectTypeOf),
                    args: vec![Register::new(0)],
                },
                BytecodeInstruction::Return(Some(Register::new(1))),
            ],
            ValueType::Str,
            vec![ValueType::I32, ValueType::Str],
        ),
    );

    let mut vm = Vm::new(runtime);
    let error = vm.execute(&loaded, "main").unwrap_err();

    assert!(matches!(
        error,
        crate::VmError::RuntimeError(ref error)
            if error.kind() == RuntimeErrorKind::CapabilityDenied
                && error.message().contains("reflection_read")
    ));
}

#[test]
fn executes_runtime_reflect_get_and_set_field_helpers() {
    let (runtime, loaded) = load_reflection_bytecode_module(
        "reflect_field.kbc",
        test_function_module(
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::I32(1),
                },
                BytecodeInstruction::MakeStruct {
                    dst: Register::new(1),
                    name: "Point".to_owned(),
                    fields: vec![StructFieldInit {
                        name: "x".to_owned(),
                        value: Register::new(0),
                    }],
                },
                BytecodeInstruction::LoadConst {
                    dst: Register::new(2),
                    constant: ConstantOperand::I32(9),
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(3)),
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetField(
                        "x".to_owned(),
                    )),
                    args: vec![Register::new(1), Register::new(2)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(4)),
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectGetField(
                        "x".to_owned(),
                    )),
                    args: vec![Register::new(3)],
                },
                BytecodeInstruction::Return(Some(Register::new(4))),
            ],
            ValueType::I32,
            vec![
                ValueType::I32,
                ValueType::HeapObject,
                ValueType::I32,
                ValueType::HeapObject,
                ValueType::I32,
            ],
        ),
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(9));
}

#[test]
fn executes_runtime_reflect_set_index_helper() {
    let (runtime, loaded) = load_reflection_bytecode_module(
        "reflect_index.kbc",
        test_function_module(
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::I32(1),
                },
                BytecodeInstruction::LoadConst {
                    dst: Register::new(1),
                    constant: ConstantOperand::I32(2),
                },
                BytecodeInstruction::MakeArray {
                    dst: Register::new(2),
                    elements: vec![Register::new(0), Register::new(1)],
                },
                BytecodeInstruction::LoadConst {
                    dst: Register::new(3),
                    constant: ConstantOperand::I32(0),
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(4)),
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetIndex),
                    args: vec![Register::new(2), Register::new(3), Register::new(1)],
                },
                BytecodeInstruction::Return(Some(Register::new(4))),
            ],
            ValueType::HeapObject,
            vec![
                ValueType::I32,
                ValueType::I32,
                ValueType::HeapObject,
                ValueType::I32,
                ValueType::HeapObject,
            ],
        ),
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    let Value::Array(handle) = report.return_value else {
        panic!("expected array return value");
    };
    assert_eq!(
        vm.runtime().gc().array_snapshot(handle),
        Some(vec![Value::I32(2), Value::I32(2)])
    );
}

#[test]
fn executes_source_lowered_type_of_helper() {
    let (runtime, loaded) = load_reflection_test_module("fn main() -> String { type_of(7) }");
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::Str("i32".to_owned()));
}

#[test]
fn executes_source_lowered_print_builtin() {
    let messages = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&messages);

    let mut runtime = Runtime::default();
    runtime
        .register_host_function(HostFunction::new("host.log", vec![], "()", move |args| {
            let Some(Value::Str(message)) = args.first() else {
                return Err(HostError::new("host.log expects one string argument"));
            };
            sink.lock()
                .expect("message sink should lock")
                .push(message.clone());
            Ok(Value::Unit)
        }))
        .expect("host function should register");
    let bytecode = compile_test_bytecode(r#"fn main() { print("hello"); }"#);
    let loaded = runtime
        .load_module("print.kgr", bytecode)
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
fn executes_source_lowered_reflection_field_helpers() {
    let (runtime, loaded) = load_reflection_test_module(
        r#"
struct Point { var x: i32 }

fn main() -> i32 {
    val point = Point { x: 1 };
    val next = set_field(point, "x", 9);
    get_field(next, "x")
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(9));
}

#[test]
fn executes_source_lowered_set_index_helper() {
    let (runtime, loaded) = load_reflection_test_module(
        r#"
fn main() -> [i32] {
    val values = [1, 2];
    set_index(values, 0, 9)
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    let Value::Array(handle) = report.return_value else {
        panic!("expected array return value");
    };
    assert_eq!(
        vm.runtime().gc().array_snapshot(handle),
        Some(vec![Value::I32(9), Value::I32(2)])
    );
}

#[test]
fn executes_source_lowered_place_assignments() {
    let (runtime, loaded) = load_test_module(
        r#"
struct Point { var x: i32 }
struct Holder { var inner: Point }

fn main() -> i32 {
    var holder = Holder { inner: Point { x: 1 } };
    holder.inner.x = 7;
    var values = [1, 2];
    values[0] = 5;
    holder.inner.x + values[0]
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I32(12));
}

#[test]
fn executes_source_lowered_array_methods() {
    let (runtime, loaded) = load_test_module(
        r#"
fn main() -> usize {
    val values = [1, 2];
    values.push(3).pop().len()
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I64(2));
}

#[test]
fn executes_iterable_protocol_builtin_over_arrays() {
    let (runtime, loaded) = load_bytecode_module(
        "iterable_array.kbc",
        test_function_module(
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::I32(4),
                },
                BytecodeInstruction::LoadConst {
                    dst: Register::new(1),
                    constant: ConstantOperand::I32(7),
                },
                BytecodeInstruction::MakeArray {
                    dst: Register::new(2),
                    elements: vec![Register::new(0), Register::new(1)],
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(3)),
                    callee: CallTarget::BuiltinMethod(BuiltinMethod::Iterable(
                        iterable::Method::Len,
                    )),
                    args: vec![Register::new(2)],
                },
                BytecodeInstruction::LoadConst {
                    dst: Register::new(4),
                    constant: ConstantOperand::I32(1),
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(5)),
                    callee: CallTarget::BuiltinMethod(BuiltinMethod::Iterable(
                        iterable::Method::Get,
                    )),
                    args: vec![Register::new(2), Register::new(4)],
                },
                BytecodeInstruction::MakeTuple {
                    dst: Register::new(6),
                    elements: vec![Register::new(3), Register::new(5)],
                },
                BytecodeInstruction::Return(Some(Register::new(6))),
            ],
            ValueType::HeapObject,
            vec![
                ValueType::I32,
                ValueType::I32,
                ValueType::HeapObject,
                ValueType::I64,
                ValueType::I32,
                ValueType::I32,
                ValueType::HeapObject,
            ],
        ),
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![Value::I64(2), Value::I32(7)])
    );
}

#[test]
fn executes_source_lowered_string_len_method() {
    let (runtime, loaded) = load_test_module(
        r#"
fn main() -> usize {
    "kagari".len()
}
"#,
    );
    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(report.return_value, Value::I64(6));
}

#[test]
fn executes_iterable_protocol_builtin_over_strings() {
    let (runtime, loaded) = load_bytecode_module(
        "iterable_string.kbc",
        test_function_module(
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::Str("ab".to_owned()),
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(1)),
                    callee: CallTarget::BuiltinMethod(BuiltinMethod::Iterable(
                        iterable::Method::Len,
                    )),
                    args: vec![Register::new(0)],
                },
                BytecodeInstruction::LoadConst {
                    dst: Register::new(2),
                    constant: ConstantOperand::I32(0),
                },
                BytecodeInstruction::Call {
                    dst: Some(Register::new(3)),
                    callee: CallTarget::BuiltinMethod(BuiltinMethod::Iterable(
                        iterable::Method::Get,
                    )),
                    args: vec![Register::new(0), Register::new(2)],
                },
                BytecodeInstruction::MakeTuple {
                    dst: Register::new(4),
                    elements: vec![Register::new(1), Register::new(3)],
                },
                BytecodeInstruction::Return(Some(Register::new(4))),
            ],
            ValueType::HeapObject,
            vec![
                ValueType::Str,
                ValueType::I64,
                ValueType::I32,
                ValueType::Str,
                ValueType::HeapObject,
            ],
        ),
    );

    let mut vm = Vm::new(runtime);
    let report = vm.execute(&loaded, "main").expect("vm should execute");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![Value::I64(2), Value::Str("a".to_owned())])
    );
}

#[test]
fn array_methods_mutate_shared_array_handle_in_place() {
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

    assert_eq!(report.return_value, Value::I64(3));
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
