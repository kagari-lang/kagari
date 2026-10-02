use kagari_abi::{scalar::BuiltinType, types::AbiType};
use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::{
    collection::CollectionAccess,
    host_interface::{
        path::{HostIndexSegmentDeclaration, HostVirtualSegmentDeclaration},
        type_declaration::{HostFieldDeclaration, HostTypeDeclaration},
        value_type::HostValueType,
    },
};
use kagari_runtime::module::LoadedModule;

use std::sync::{Arc, Mutex};
use {
    kagari_common::host_interface::type_declaration::{
        HostReflectionPolicy, HostTypeOwnership, PathAccess,
    },
    kagari_runtime::{
        Runtime, RuntimeConfig,
        error::RuntimeErrorKind,
        host::{
            DynamicPathArgument, DynamicPathArguments, HostBorrowTable, HostError, HostObjectId,
            HostPathAdapter, HostPathDescriptorId, HostPathDescriptorRegistration,
            HostPathOperation, HostPathSegmentRegistration, HostSchemaEpoch, HostTypeRegistration,
            PreparedHostPathWrite,
        },
        metadata::{AbiFingerprint, TypeId, TypeKind, TypeRegistration},
        value::Value,
    },
};

use kagari_bytecode::instruction::BinaryOp;

fn allocation_owner(runtime: &mut Runtime) -> LoadedModule {
    runtime
        .load_program(
            "allocation-owner",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap()
}

fn path_mutation_runtime() -> Runtime {
    Runtime::new(path_mutation_config())
}

fn path_mutation_config() -> RuntimeConfig {
    RuntimeConfig {
        ..RuntimeConfig::default()
    }
}

fn register_i32(runtime: &Runtime) -> TypeId {
    runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(10),
            ..TypeRegistration::new("i32", TypeKind::Primitive)
        })
        .unwrap()
}

fn register_host_root_type(runtime: &mut Runtime, name: &str, access: PathAccess) -> TypeId {
    let mut registration = HostTypeRegistration::new(HostTypeDeclaration::new(name), name);
    registration.declaration.ownership = HostTypeOwnership::HostRoot;
    registration.declaration.path_access = access;
    for name in ["hp", "secure_hp", "count"] {
        let mut field =
            HostFieldDeclaration::new(&registration.declaration.id, name, HostValueType::I32);
        field.writable = access == PathAccess::ReadWrite;
        field.path_access = access;
        registration.declaration.fields.push(field);
    }

    registration.declaration.reflection = HostReflectionPolicy::Metadata;

    runtime.register_host_type(registration).unwrap()
}

fn register_hp_descriptor(
    runtime: &mut Runtime,
    player_id: TypeId,
    i32_id: TypeId,
    access: PathAccess,
) -> HostPathDescriptorId {
    runtime
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
        .unwrap()
}

mod contracts;
mod mutation;
