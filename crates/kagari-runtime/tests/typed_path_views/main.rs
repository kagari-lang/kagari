use kagari_common::collection::CollectionAccess;
use kagari_common::host_interface::{
    HostIndexSegmentDeclaration, HostValueType, HostVirtualSegmentDeclaration,
};
use kagari_runtime::host::PreparedHostPathWrite;
use std::sync::{Arc, Mutex};

use kagari_bytecode::BinaryOp;
use kagari_runtime::AbiFingerprint;
use kagari_runtime::CapabilitySet;
use kagari_runtime::DynamicPathArgument;
use kagari_runtime::DynamicPathArguments;
use kagari_runtime::HostBorrowTable;
use kagari_runtime::HostExposurePolicy;
use kagari_runtime::HostObjectId;
use kagari_runtime::HostPathAdapter;
use kagari_runtime::HostPathDescriptorId;
use kagari_runtime::HostPathDescriptorRegistration;
use kagari_runtime::HostPathOperation;
use kagari_runtime::HostPathSegmentRegistration;
use kagari_runtime::HostReflectionPolicy;
use kagari_runtime::HostSchemaEpoch;
use kagari_runtime::HostTypeOwnership;
use kagari_runtime::HostTypeRegistration;
use kagari_runtime::LanguageProfile;
use kagari_runtime::PathAccess;
use kagari_runtime::Runtime;
use kagari_runtime::RuntimeConfig;
use kagari_runtime::RuntimeErrorKind;
use kagari_runtime::SecurityContext;
use kagari_runtime::TypeId;
use kagari_runtime::TypeKind;
use kagari_runtime::TypeRegistration;
use kagari_runtime::host::HostError;
use kagari_runtime::value::Value;

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
