use kagari_bytecode::{
    instruction::BinaryOp,
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    host::{
        DynamicPathArgument, DynamicPathArguments, HostBorrowTable, HostError, HostObjectId,
        HostPathAdapter, HostPathDescriptorId, HostPathDescriptorRegistration, HostPathOperation,
        HostSchemaEpoch, HostTypeRegistration, PreparedHostPathWrite,
    },
    metadata::{AbiFingerprint, TypeId, TypeKind, TypeRegistration},
    module::LoadedModule,
    value::Value,
};
use kagari_types::{
    declaration::host::{HostReflectionPolicy, HostTypeOwnership, PathAccess},
    host_interface::{
        path::{
            HostIndexSegmentDeclaration, HostPathSegmentDeclaration, HostVirtualSegmentDeclaration,
        },
        type_declaration::{HostFieldDeclaration, HostTypeDeclaration},
        value_type::HostValueType,
    },
    scalar::BuiltinType,
    ty::Ty,
};
use std::sync::{Arc, Mutex};

fn allocation_owner(runtime: &mut Runtime) -> LoadedModule {
    let context = DefinitionContext::new().unwrap();
    runtime
        .load_program(
            "allocation-owner",
            DefinitionMetadata::checked(
                context.snapshot(),
                BytecodeProgram {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                &Default::default(),
            )
            .unwrap(),
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
    let context = DefinitionContext::new().unwrap();
    let mut declaration = HostTypeDeclaration::new(&context, name).unwrap();
    declaration.ownership = HostTypeOwnership::HostRoot;
    declaration.path_access = access;
    for name in ["hp", "secure_hp", "count"] {
        let mut field =
            HostFieldDeclaration::new(&context, declaration.id, name, HostValueType::I32).unwrap();
        field.writable = access == PathAccess::ReadWrite;
        field.path_access = access;
        declaration.fields.push(field);
    }

    declaration.reflection = HostReflectionPolicy::Metadata;

    runtime
        .register_host_type(HostTypeRegistration::new(
            DefinitionMetadata::checked(context.snapshot(), declaration, &Default::default())
                .unwrap(),
            name,
        ))
        .unwrap()
}

fn register_hp_descriptor(
    runtime: &mut Runtime,
    player_id: TypeId,
    i32_id: TypeId,
    access: PathAccess,
) -> HostPathDescriptorId {
    runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            definitions: runtime.host().definitions(),
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegmentDeclaration::Field(
                runtime
                    .host()
                    .host_type(player_id)
                    .unwrap()
                    .declaration
                    .fields
                    .iter()
                    .find(|field| field.name == "hp")
                    .unwrap()
                    .id,
            )],
            access,
            schema_epoch: HostSchemaEpoch::new(0),
        })
        .unwrap()
}

mod contracts;
mod mutation;
