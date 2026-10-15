//! Host registration uses shared declarations, explicit callbacks and atomic installation.
mod support;

use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    host::{HostCallContext, HostError, HostObjectId, HostSchemaEpoch},
    metadata::{AbiFingerprint, TypeKind},
    native::{
        builder::ModuleBuilder,
        declarations::{FunctionDecl, MethodDecl},
        types::{Type, TypeRef},
    },
    value::Value,
};
use kagari_types::{
    declaration::{
        FieldDef, fingerprint,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostPassingStyle,
            HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::sync::{Arc, Mutex};
use support::native::{invoke::invoke, program::free_function_program};

fn player(module: &mut ModuleBuilder, reflection: HostReflectionPolicy, field: bool) -> TypeRef {
    let mut player = module.define_type("Player").unwrap();
    player
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection,
            path_access: PathAccess::ReadWrite,
        })
        .unwrap();
    if field {
        player
            .field(
                DefinitionMetadata::checked(
                    DefinitionContext::new().unwrap().snapshot(),
                    FieldDef {
                        name: "hp".into(),
                        ty: Ty::Builtin(BuiltinType::I32),
                        visibility: Visibility::Public,
                        mutable: true,
                        host_policy: Some(HostFieldPolicy {
                            readable: true,
                            path_access: PathAccess::ReadWrite,
                        }),
                    },
                    &Default::default(),
                )
                .unwrap(),
            )
            .unwrap();
    }
    player.finish().unwrap()
}

fn root(runtime: &mut Runtime) -> Value {
    let ty = runtime.types().get_by_name("game::Player").unwrap().id;
    let root = runtime
        .register_host_root(HostObjectId(1), ty, HostSchemaEpoch::new(0))
        .unwrap();
    runtime.gc().alloc_host_root(root).unwrap()
}

#[test]
fn callback_context_releases_borrows_and_rejects_borrowed_results() {
    let mut module = ModuleBuilder::new("game", &Default::default()).unwrap();
    let player = player(&mut module, HostReflectionPolicy::Hidden, false);
    let function = module
        .define_function(
            FunctionDecl::new("heal")
                .returns(player.apply([]).unwrap())
                .host_policy(HostCallPolicy {
                    passing: vec![],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    module
        .bind(function, |context: &HostCallContext<'_>, _: &[Value]| {
            let ty = context
                .runtime()
                .types()
                .get_by_name("game::Player")
                .unwrap()
                .id;
            let token = context
                .borrows()
                .borrow_unique(HostObjectId(1), ty)
                .unwrap();
            Ok(context.runtime().gc().alloc_host_mut(token).unwrap())
        })
        .unwrap();
    let module = module.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    root(&mut runtime);
    let loaded = runtime.load_program("borrow-result", program).unwrap();
    assert_eq!(
        invoke(&runtime, &loaded, 0, &[]).unwrap_err().kind(),
        RuntimeErrorKind::HostBorrowEscape
    );
    let ty = runtime.types().get_by_name("game::Player").unwrap().id;
    let resources = runtime.host_scope(&[]).unwrap();
    resources
        .borrows()
        .borrow_unique(HostObjectId(1), ty)
        .unwrap();
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn registers_host_function_metadata_and_invokes_handler() {
    let mut module = ModuleBuilder::new("game", &Default::default()).unwrap();
    let player = player(&mut module, HostReflectionPolicy::Hidden, false);
    let policy = HostCallPolicy {
        passing: vec![HostPassingStyle::UniqueBorrow, HostPassingStyle::Owned],
        effects: HostFunctionEffects {
            may_mutate_host_state: true,
            may_trap: true,
            ..Default::default()
        },
    };
    let function = module
        .define_function(
            FunctionDecl::new("heal")
                .parameter("player", player.apply([]).unwrap())
                .parameter("hp", Type::i32())
                .returns(Type::i32())
                .host_policy(policy.clone()),
        )
        .unwrap();
    module
        .bind(
            function,
            |_: &HostCallContext<'_>, args: &[Value]| match args {
                [Value::HostRoot(_), Value::I32(hp)] => Ok(Value::I32(*hp)),
                _ => Err(HostError::new("game::heal expects host root and i32")),
            },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    let root = root(&mut runtime);
    let loaded = runtime.load_program("heal", program).unwrap();
    let registered = &loaded.bytecode.native_declarations[0];
    assert_eq!(registered.function.name, "heal");
    assert_eq!(registered.function.host_policy.as_ref(), Some(&policy));
    let identity = loaded
        .definitions()
        .resolve(registered.declaration)
        .unwrap();
    assert_eq!(identity.module().package.0, "game");
    assert_eq!(identity.last().unwrap().name, "heal");
    assert_eq!(
        invoke(&runtime, &loaded, 0, &[root, Value::I32(7)]).unwrap(),
        Value::I32(7)
    );
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn installed_host_functions_are_available() {
    let calls = Arc::new(Mutex::new(0usize));
    let calls_for_host = calls.clone();
    let mut module = ModuleBuilder::new("game", &Default::default()).unwrap();
    let function = module
        .define_function(FunctionDecl::new("tick").host_policy(HostCallPolicy {
            passing: vec![],
            effects: Default::default(),
        }))
        .unwrap();
    module
        .bind(function, move |_: &HostCallContext<'_>, _: &[Value]| {
            *calls_for_host.lock().unwrap() += 1;
            Ok(Value::Unit)
        })
        .unwrap();
    let module = module.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    let loaded = runtime.load_program("tick", program).unwrap();
    assert_eq!(invoke(&runtime, &loaded, 0, &[]).unwrap(), Value::Unit);
    assert_eq!(*calls.lock().unwrap(), 1);
}

#[test]
fn rejects_duplicate_host_function_symbols() {
    let mut module = ModuleBuilder::new("game", &Default::default()).unwrap();
    let declaration = FunctionDecl::new("tick").host_policy(HostCallPolicy {
        passing: vec![],
        effects: Default::default(),
    });
    let function = module.define_function(declaration.clone()).unwrap();
    assert!(module.define_function(declaration).is_err());
    module
        .bind(function, |_: &HostCallContext<'_>, _: &[Value]| {
            Ok(Value::Unit)
        })
        .unwrap();
    let module = module.finish().unwrap();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    assert_eq!(
        module.install(&mut runtime).unwrap_err().kind(),
        RuntimeErrorKind::MetadataConflict
    );
    let loaded = runtime
        .load_program("original", free_function_program(&module.to_declaration()))
        .unwrap();
    assert_eq!(invoke(&runtime, &loaded, 0, &[]).unwrap(), Value::Unit);
}

#[test]
fn registers_host_type_metadata_with_stable_runtime_type_identity() {
    let mut module = ModuleBuilder::new("game", &Default::default()).unwrap();
    let player = player(&mut module, HostReflectionPolicy::Metadata, true);
    module
        .implement(player.clone(), |group| {
            group.inherent_impl(|methods| {
                let heal = methods.define_method(
                    MethodDecl::instance("heal")
                        .parameter("hp", Type::i32())
                        .returns(Type::i32())
                        .host_policy(HostCallPolicy {
                            passing: vec![HostPassingStyle::UniqueBorrow, HostPassingStyle::Owned],
                            effects: Default::default(),
                        }),
                )?;
                methods.bind(heal, |_: &HostCallContext<'_>, args: &[Value]| Ok(args[1]))
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let fingerprint = AbiFingerprint(
        fingerprint::schema(
            player.id(),
            &module.declaration().types[0],
            module.definitions(),
            &Default::default(),
        )
        .unwrap(),
    );
    let mut first = Runtime::default();
    let mut second = Runtime::default();
    module.install(&mut first).unwrap();
    module.install(&mut second).unwrap();
    let type_info = first.types().get_by_name("game::Player").unwrap();
    let type_id = type_info.id;
    assert_eq!(first.types().get(type_id).unwrap().id, type_id);
    assert_eq!(type_info.kind, TypeKind::HostObject);
    assert_eq!(type_info.abi_fingerprint, fingerprint);
    assert_eq!(type_info.fields[0].name, "hp");
    assert_eq!(type_info.fields[0].path_access, PathAccess::ReadWrite);
    assert!(type_info.fields[0].readable && type_info.fields[0].writable);
    let i32_id = first.types().get_by_name("i32").unwrap().id;
    assert_eq!(type_info.fields[0].ty, i32_id);
    assert_eq!(type_info.methods[0].name, "heal");
    assert_eq!(type_info.methods[0].params[0].name, "self");
    assert_eq!(type_info.methods[0].params[0].ty, type_id);
    assert_eq!(type_info.methods[0].params[1].ty, i32_id);
    assert_eq!(type_info.methods[0].return_type, i32_id);
    let other = second.types().get_by_name("game::Player").unwrap();
    assert_eq!(other.abi_fingerprint, type_info.abi_fingerprint);
    assert_eq!(
        other.fields[0].abi_fingerprint,
        type_info.fields[0].abi_fingerprint
    );
    assert_eq!(
        other.methods[0].abi_fingerprint,
        type_info.methods[0].abi_fingerprint
    );
    let Value::HostRoot(root) = root(&mut first) else {
        unreachable!()
    };
    assert_eq!(first.gc().host_root(root).unwrap().type_id(), type_id);
}

#[test]
fn rejects_duplicate_host_type_names() {
    let mut module = ModuleBuilder::new("game", &Default::default()).unwrap();
    player(&mut module, HostReflectionPolicy::Hidden, false);
    let mut duplicate = module.define_type("Player").unwrap();
    duplicate
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadWrite,
        })
        .unwrap();
    assert!(duplicate.finish().is_err());
    let module = module.finish().unwrap();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    let before = runtime.types().get_by_name("game::Player").unwrap();
    assert_eq!(
        module.install(&mut runtime).unwrap_err().kind(),
        RuntimeErrorKind::MetadataConflict
    );
    assert_eq!(
        runtime.types().get_by_name("game::Player").unwrap().id,
        before.id
    );
    root(&mut runtime);
}
