use crate::test_support::native::program::ProgramFixture;
use crate::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    host::{
        DynamicPathArgument, DynamicPathArguments, HostBorrowTable, HostError, HostObjectId,
        HostPathAdapter, HostPathDescriptorId, HostPathDescriptorRegistration, HostPathOperation,
        HostSchemaEpoch, PreparedHostPathWrite,
    },
    metadata::{AbiFingerprint, TypeId, TypeKind, TypeRegistration},
    module::ExecutableModule,
    native::{builder::ModuleBuilder, module::NativeModule},
    value::Value,
};
use kagari_bytecode::{instruction::BinaryOp, module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
    metadata::DefinitionMetadata, table::DefinitionId,
};

use kagari_types::{
    declaration::{
        FieldDef,
        host::{
            HostFieldPolicy, HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        path::{
            HostIndexSegmentDeclaration, HostPathSegmentDeclaration, HostVirtualSegmentDeclaration,
        },
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::sync::{Arc, Mutex};

fn allocation_owner(runtime: &mut Runtime) -> ExecutableModule {
    let context = DefinitionContext::new().unwrap();
    let program = runtime
        .load_program(
            "allocation-owner",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .unwrap();
    runtime.select_program(&program).unwrap()
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

fn root_module(name: &str, access: PathAccess, fields: Vec<(&str, Ty)>) -> NativeModule {
    let (package, name) = name.split_once("::").unwrap();
    let mut module = ModuleBuilder::new(package, &Default::default()).unwrap();
    let mut owner = module.define_type(name).unwrap();
    owner
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: access,
        })
        .unwrap();
    for (name, ty) in fields {
        owner
            .field(
                DefinitionMetadata::checked(
                    DefinitionContext::new().unwrap().snapshot(),
                    FieldDef {
                        name: name.into(),
                        ty,
                        visibility: Visibility::Public,
                        mutable: access == PathAccess::ReadWrite,
                        host_policy: Some(HostFieldPolicy {
                            readable: true,
                            path_access: access,
                        }),
                    },
                    &Default::default(),
                )
                .unwrap(),
            )
            .unwrap();
    }
    owner.finish().unwrap();
    module.finish().unwrap()
}

fn register_host_root_type(runtime: &mut Runtime, name: &str, access: PathAccess) -> TypeId {
    root_module(
        name,
        access,
        ["hp", "secure_hp", "count"]
            .into_iter()
            .map(|name| (name, Ty::Builtin(BuiltinType::I32)))
            .collect(),
    )
    .install(runtime)
    .unwrap();
    runtime.types().get_by_name(name).unwrap().id
}

fn nominal(context: &DefinitionContext, name: &str) -> DefinitionId {
    let (package, name) = name.split_once("::").unwrap();
    let root = context
        .intern_root(&ModuleIdentity {
            package: PackageId(package.into()),
            path: vec![],
        })
        .unwrap();
    context
        .intern_child(root, DefinitionKind::Struct, name, 0)
        .unwrap()
}

fn host_declaration(runtime: &Runtime, ty: TypeId) -> DefinitionId {
    let name = runtime.types().get(ty).unwrap().name;
    let (package, name) = name.split_once("::").unwrap();
    let definitions = runtime.host().definitions();
    let root = definitions
        .lookup_root(&ModuleIdentity {
            package: PackageId(package.into()),
            path: vec![],
        })
        .unwrap();
    definitions
        .lookup_child(root, DefinitionKind::Struct, name, 0)
        .unwrap()
}

fn host_field(runtime: &Runtime, ty: TypeId, name: &str) -> DefinitionId {
    runtime
        .host()
        .definitions()
        .lookup_child(
            host_declaration(runtime, ty),
            DefinitionKind::Field,
            name,
            0,
        )
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
            segments: vec![HostPathSegmentDeclaration::Field(host_field(
                runtime, player_id, "hp",
            ))],
            access,
            schema_epoch: HostSchemaEpoch::new(0),
        })
        .unwrap()
}

mod contracts;
mod mutation;
