use kagari_common::{
    collection::CollectionAccess,
    host_interface::{HostIndexSegmentDeclaration, HostValueType, HostVirtualSegmentDeclaration},
};
use kagari_runtime::{
    AbiFingerprint, CapabilitySet, DynamicPathArgument, DynamicPathArguments, HostBorrowTable,
    HostExposurePolicy, HostObjectId, HostPathAdapter, HostPathDescriptorId,
    HostPathDescriptorRegistration, HostPathOperation, HostPathSegmentRegistration,
    HostReflectionPolicy, HostSchemaEpoch, HostTypeOwnership, HostTypeRegistration,
    LanguageProfile, PathAccess, Runtime, RuntimeConfig, RuntimeErrorKind, SecurityContext, TypeId,
    TypeKind, TypeRegistration,
    host::{HostError, PreparedHostPathWrite},
    value::Value,
};
use std::sync::{Arc, Mutex};

use kagari_bytecode::BinaryOp;

fn path_mutation_runtime() -> Runtime {
    Runtime::new(path_mutation_config())
}

fn path_mutation_config() -> RuntimeConfig {
    RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_path_mutation: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                path_mutation: true,
                ..CapabilitySet::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_types: vec![
                "game.Player".to_owned(),
                "game.Item".to_owned(),
                "game.ReadOnly".to_owned(),
                "game.Opaque".to_owned(),
            ],
            allow_host_path_reads: true,
            allow_host_path_mutation: true,
            ..HostExposurePolicy::default()
        },
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
    let mut registration = HostTypeRegistration::new(
        kagari_common::host_interface::HostTypeDeclaration::new(name),
        name,
    );
    registration.declaration.ownership = HostTypeOwnership::HostRoot;
    registration.declaration.path_access = access;
    for name in ["hp", "secure_hp", "count"] {
        let mut field = kagari_common::host_interface::HostFieldDeclaration::new(
            &registration.declaration.id,
            name,
            kagari_common::host_interface::HostValueType::I32,
        );
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
            capability_requirements: CapabilitySet::default(),
        })
        .unwrap()
}

mod contracts;
mod mutation;
