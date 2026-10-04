use crate::{
    tests::common::{
        compile_test_bytecode, load_bytecode_module, load_bytecode_module_with_runtime,
        load_bytecode_program_with_runtime, load_test_module, standard_runtime,
        test_function_module,
    },
    vm::Vm,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{
        BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, PathId, Register,
        RuntimeHelper, StructId,
    },
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, PathRecord},
};
use kagari_contract::{ids::FunctionRef, native_import::NativeImport};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    host::{
        HostError, HostFunction, HostObjectId, HostPathAdapter, HostPathDescriptorId,
        HostPathDescriptorRegistration, HostPathSegmentRegistration, HostSchemaEpoch,
        HostTypeRegistration, PreparedHostPathWrite,
    },
    metadata::{AbiFingerprint, TypeKind, TypeRegistration},
    module::LoadedModule,
    value::Value,
};
use kagari_types::host_interface::{
    type_declaration::{
        HostFieldDeclaration, HostReflectionPolicy, HostTypeDeclaration, HostTypeOwnership,
        PathAccess,
    },
    value_type::HostValueType,
};
use std::sync::{Arc, Mutex};

fn host_runtime() -> Runtime {
    standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

fn reflection_runtime() -> Runtime {
    standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

fn load_reflection_bytecode_module(
    name: &str,
    bytecode: BytecodeModule,
) -> (Runtime, LoadedModule) {
    load_bytecode_module_with_runtime(reflection_runtime(), name, bytecode)
}

fn load_reflection_test_module(source_text: &str) -> (Runtime, LoadedModule) {
    load_bytecode_program_with_runtime(
        reflection_runtime(),
        "test.kgr",
        compile_test_bytecode(source_text),
    )
}

fn register_vm_host_path_runtime(access: PathAccess) -> (Runtime, Arc<Mutex<i32>>) {
    let mut runtime = host_runtime();
    let i32_id = runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(1),
            ..TypeRegistration::new("i32", TypeKind::Primitive)
        })
        .unwrap();
    let host_type = HostTypeRegistration::new(player_type_declaration(), "game.Player");
    let player_id = runtime.register_host_type(host_type).unwrap();
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(
            kagari_types::host_interface::HostFunctionDeclaration::new(
                "host.player",
                vec![],
                HostValueType::opaque("game.Player"),
            ),
            move |_, _| Ok(Value::HostRoot(root)),
        ))
        .unwrap();
    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegmentRegistration::Field {
                declaration: runtime
                    .host()
                    .host_type(player_id)
                    .unwrap()
                    .declaration
                    .fields
                    .iter()
                    .find(|field| field.name == "hp")
                    .unwrap()
                    .id
                    .clone(),
            }],
            access,
            schema_epoch: HostSchemaEpoch::new(0),
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
                .with_read(move |_, _| Ok(Value::I32(*read_hp.lock().unwrap())))
                .with_prepare_write(move |_, _, record| {
                    let Value::I32(value) = record.new_value else {
                        return Err(HostError::new("hp expects i32"));
                    };
                    let write_hp = write_hp.clone();
                    Ok(PreparedHostPathWrite::new(move || {
                        *write_hp.lock().unwrap() = value;
                    }))
                }),
        )
        .unwrap();

    (runtime, hp)
}

fn path_module(
    runtime: &Runtime,
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
    let metadata = FunctionMetadata {
        return_type,
        registers: vec![
            ValueType::HostHandle,
            ValueType::I32,
            ValueType::I32,
            ValueType::I32,
            ValueType::I32,
            ValueType::HostHandle,
        ],
        ..Default::default()
    };
    let host_interface = kagari_types::host_interface::HostInterface {
        paths: vec![],
        types: vec![player_type_declaration()],
        functions: vec![kagari_types::host_interface::HostFunctionDeclaration::new(
            "host.player",
            vec![],
            HostValueType::opaque("game.Player"),
        )],
    };
    let native_imports = host_interface
        .functions
        .iter()
        .map(NativeImport::from_host)
        .collect();
    BytecodeModule {
        host_interface,
        native_imports,
        module_slots: vec![],
        constants: instructions_constants,
        types: vec![ValueType::Unit, ValueType::HostHandle, ValueType::I32],
        paths: vec![PathRecord {
            contract_fingerprint: runtime
                .host()
                .path_descriptor(HostPathDescriptorId::new(0))
                .unwrap()
                .abi_fingerprint
                .0,
            id: PathId::new(0),
            root_ty: ValueType::HostHandle,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "game.Player.hp".to_owned(),
        }],
        function_table: vec![FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: name.to_owned(),
            params: metadata.params.clone(),
            return_type: metadata.return_type,
            effects: metadata.effects,
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            identity: None,
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

fn player_type_declaration() -> HostTypeDeclaration {
    let mut declaration = HostTypeDeclaration::new("game.Player");
    declaration.ownership = HostTypeOwnership::HostRoot;
    declaration.path_access = PathAccess::ReadWrite;
    let mut hp = HostFieldDeclaration::new(&declaration.id, "hp", HostValueType::I32);
    hp.writable = true;
    hp.path_access = PathAccess::ReadWrite;
    declaration.fields.push(hp);
    declaration.reflection = HostReflectionPolicy::Hidden;
    declaration
}

mod host_paths;
mod reflection;
mod standard;
