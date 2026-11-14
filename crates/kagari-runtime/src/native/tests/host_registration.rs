//! Host registration uses shared declarations, explicit callbacks and atomic installation.
use crate::test_support as support;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;

use crate::{
    Runtime,
    error::RuntimeErrorKind,
    execution::roots::ExecutionRoots,
    host::{HostCallContext, HostError, HostObjectId, HostSchemaEpoch},
    metadata::{AbiFingerprint, TypeKind},
    native::{
        builder::ModuleBuilder,
        declarations::{FunctionDecl, MethodDecl},
        host,
        types::{Type, TypeRef},
    },
    value::{
        EphemeralValue,
        carriers::RootedValue,
        host_values::{EphemeralObject, HostRootObject},
        kind::ObjectKind,
        view::ValueView,
    },
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
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

fn root(runtime: &mut Runtime) -> RootedValue {
    let ty = runtime.types().get_by_name("game::Player").unwrap().id;
    let root = runtime
        .register_host_root(HostObjectId(1), ty, HostSchemaEpoch::new(0))
        .unwrap();
    runtime.host_root_value(root).unwrap()
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
        .bind(
            function,
            host::callback(|context: &mut HostCallContext<'_>, _: &[ValueView<'_>]| {
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
                let execution = context.runtime().resources.active_session().unwrap().id;
                {
                    let Runtime {
                        memory, resources, ..
                    } = context.runtime_mut();
                    memory.scope(resources, |cx, roots| {
                        ExecutionRoots::with(roots, cx, |slots| {
                            let value = EphemeralObject::alloc(
                                slots.scope(),
                                EphemeralValue::HostMut(token),
                            )?;
                            let location = slots.push_local(execution, value)?;
                            ValueView::protected(value, Some(location))
                        })
                    })
                }
                .map_err(|error| HostError::new(error.to_string()))
            }),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    drop(root(&mut runtime));
    let loaded_program = load_bytecode(&mut runtime, "borrow-result", program).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let baseline = runtime.gc_stats().root_registrations;
    assert_eq!(
        invoke(&mut runtime, &loaded, 0, &[]).unwrap_err().kind(),
        RuntimeErrorKind::HostBorrowEscape
    );
    let ty = runtime.types().get_by_name("game::Player").unwrap().id;
    let resources = runtime.host_scope(&[]).unwrap();
    resources
        .borrows()
        .borrow_unique(HostObjectId(1), ty)
        .unwrap();
    assert_eq!(resources.runtime().gc_stats().root_registrations, baseline);
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
            host::callback(
                |_: &mut HostCallContext<'_>, args: &[ValueView<'_>]| match args {
                    [root, hp] if root.kind() == Some(ObjectKind::HostRoot) => hp
                        .as_i32()
                        .map(ValueView::from)
                        .ok_or_else(|| HostError::new("expected i32")),
                    _ => Err(HostError::new("game::heal expects host root and i32")),
                },
            ),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    let root = root(&mut runtime);
    let loaded_program = load_bytecode(&mut runtime, "heal", program).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let registered = &loaded.bytecode.native_declarations[0];
    assert_eq!(registered.function.name, "heal");
    assert_eq!(registered.function.host_policy.as_ref(), Some(&policy));
    let identity = loaded
        .definitions()
        .resolve(registered.declaration)
        .unwrap();
    assert_eq!(identity.module().package.0, "game");
    assert_eq!(identity.last().unwrap().name, "heal");
    let baseline = runtime.collected_gc_stats().root_registrations;
    assert_eq!(
        invoke(&mut runtime, &loaded, 0, &[root, RootedValue::from(7)])
            .unwrap()
            .as_i32(),
        Some(7)
    );
    assert_eq!(
        runtime.collected_gc_stats().root_registrations,
        baseline - 1
    );
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
        .bind(
            function,
            host::callback(move |_: &mut HostCallContext<'_>, _: &[ValueView<'_>]| {
                *calls_for_host.lock().unwrap() += 1;
                Ok(ValueView::unit())
            }),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    let loaded_program = load_bytecode(&mut runtime, "tick", program).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let result = invoke(&mut runtime, &loaded, 0, &[]).unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            assert!(result.get(cx)?.is_unit());
            Ok(())
        })
        .unwrap();
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
        .bind(
            function,
            host::callback(
                |_: &mut HostCallContext<'_>, _: &[ValueView<'_>]| Ok(ValueView::unit()),
            ),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    assert_eq!(
        module.install(&mut runtime).unwrap_err().kind(),
        RuntimeErrorKind::MetadataConflict
    );
    let loaded_program = load_bytecode(
        &mut runtime,
        "original",
        free_function_program(&module.to_declaration()),
    )
    .unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let result = invoke(&mut runtime, &loaded, 0, &[]).unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            assert!(result.get(cx)?.is_unit());
            Ok(())
        })
        .unwrap();
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
                methods.bind(
                    heal,
                    host::callback(|_: &mut HostCallContext<'_>, args: &[ValueView<'_>]| {
                        args[1]
                            .as_i32()
                            .map(ValueView::from)
                            .ok_or_else(|| HostError::new("expected i32"))
                    }),
                )
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
    let root = root(&mut first);
    first
        .memory
        .scope(&first.resources, |cx, _| {
            let value = root.get(cx)?.object().unwrap();
            let object = cx.downcast::<HostRootObject>(value)?;
            assert_eq!(cx.read(object, HostRootObject::handle).type_id(), type_id);
            Ok(())
        })
        .unwrap();
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
    drop(root(&mut runtime));
}
