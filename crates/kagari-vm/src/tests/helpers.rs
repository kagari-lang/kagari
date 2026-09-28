use kagari_abi::{
    budget::LogicalBudgetCharge, ids::FunctionRef, representation::ValueType,
    standard::StandardIntrinsic,
};
use kagari_bytecode::{
    BinaryOp, BytecodeFunction, BytecodeInstruction, BytecodeModule, CallTarget, ConstantOperand,
    PathId, PathRecord, Register, RuntimeHelper, StructId,
};
use kagari_runtime::{
    AbiFingerprint, CapabilitySet, HostExposurePolicy, HostObjectId, HostPathAdapter,
    HostPathDescriptorRegistration, HostPathSegmentRegistration, HostReflectionPolicy,
    HostSchemaEpoch, HostTypeOwnership, HostTypeRegistration, LanguageProfile, PathAccess,
    ResourcePolicy, Runtime, RuntimeConfig, RuntimeErrorKind, SecurityContext, TypeKind,
    TypeRegistration,
    host::{HostError, HostFunction, PreparedHostPathWrite},
    value::Value,
};
use std::sync::{Arc, Mutex};

use crate::{
    Vm,
    tests::common::{
        compile_test_bytecode, load_bytecode_module, load_bytecode_module_with_runtime,
        load_test_module, test_function_module,
    },
};

fn host_runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                allow_path_mutation: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                path_mutation: true,
                ..CapabilitySet::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_functions: vec![
                "host.player".to_owned(),
                "host.add_i32".to_owned(),
                "host.log".to_owned(),
            ],
            allowed_host_types: vec!["game.Player".to_owned()],
            allow_host_path_reads: true,
            allow_host_path_mutation: true,
            ..HostExposurePolicy::default()
        },
        ..RuntimeConfig::default()
    })
}

fn reflection_runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_reflection: true,
                allow_reflection_write: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                reflection_metadata: true,
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
            kagari_common::host_interface::HostFunctionDeclaration::new(
                "host.player",
                vec![],
                kagari_common::host_interface::HostValueType::opaque("game.Player"),
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
    let metadata = kagari_bytecode::FunctionMetadata {
        instruction_budgets: vec![LogicalBudgetCharge::Step; instructions.len()],
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
    BytecodeModule {
        host_interface: kagari_common::host_interface::HostInterface {
            paths: vec![],
            types: vec![player_type_declaration()],
            functions: vec![kagari_common::host_interface::HostFunctionDeclaration::new(
                "host.player",
                vec![],
                kagari_common::host_interface::HostValueType::opaque("game.Player"),
            )],
        },
        module_slots: vec![],
        constants: instructions_constants,
        types: vec![ValueType::Unit, ValueType::HostHandle, ValueType::I32],
        paths: vec![PathRecord {
            contract_fingerprint: runtime
                .host()
                .path_descriptor(kagari_runtime::HostPathDescriptorId::new(0))
                .unwrap()
                .abi_fingerprint
                .0,
            id: PathId::new(0),
            root_ty: ValueType::HostHandle,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "game.Player.hp".to_owned(),
        }],
        function_table: vec![kagari_bytecode::FunctionRecord {
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

fn player_type_declaration() -> kagari_common::host_interface::HostTypeDeclaration {
    let mut declaration = kagari_common::host_interface::HostTypeDeclaration::new("game.Player");
    declaration.ownership = HostTypeOwnership::HostRoot;
    declaration.path_access = PathAccess::ReadWrite;
    let mut hp = kagari_common::host_interface::HostFieldDeclaration::new(
        &declaration.id,
        "hp",
        kagari_common::host_interface::HostValueType::I32,
    );
    hp.writable = true;
    hp.path_access = PathAccess::ReadWrite;
    declaration.fields.push(hp);
    declaration.reflection = HostReflectionPolicy::Hidden;
    declaration
}

mod host_paths;
mod reflection;
mod standard;
