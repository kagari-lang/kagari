use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext, metadata::DefinitionMetadata, scoped::DefinitionReferences,
    },
};

use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    host::{HostError, HostFunction, HostObjectId, HostSchemaEpoch, HostTypeRegistration},
    metadata::{AbiFingerprint, TypeId, TypeKind, TypeRegistration},
    value::Value,
};
use kagari_types::{
    declaration::host::{
        HostFunctionEffects, HostPassingStyle, HostReflectionPolicy, HostTypeOwnership, PathAccess,
    },
    host_interface::{
        HostFunctionDeclaration, HostParameter, host_type_identity,
        type_declaration::HostTypeDeclaration, value_type::HostValueType,
    },
};
use std::sync::{Arc, Mutex};

fn scoped<T: DefinitionReferences + Clone>(
    context: &DefinitionContext,
    value: &T,
) -> DefinitionMetadata<T> {
    DefinitionMetadata::checked(
        context.snapshot(),
        value.clone(),
        &CancellationToken::default(),
    )
    .unwrap()
}

fn host_root_value(runtime: &mut Runtime, object_id: u64) -> Value {
    let context = DefinitionContext::new().unwrap();
    let mut declaration = HostTypeDeclaration::new(&context, "game.Player").unwrap();
    declaration.ownership = HostTypeOwnership::HostRoot;
    declaration.path_access = PathAccess::ReadWrite;
    let registration = HostTypeRegistration::new(scoped(&context, &declaration), "Player");
    let ty = runtime.register_host_type(registration).unwrap();
    let root = runtime
        .register_host_root(HostObjectId(object_id), ty, HostSchemaEpoch::new(0))
        .unwrap();
    runtime.gc().alloc_host_root(root).unwrap()
}

fn exposed_host_runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

fn host_call_enabled_runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

#[test]
fn callback_context_releases_borrows_and_rejects_borrowed_results() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = exposed_host_runtime();
    host_root_value(&mut runtime, 1);
    runtime
        .register_host_function(HostFunction::new(
            scoped(
                &context,
                &HostFunctionDeclaration::new(
                    &context,
                    "game.heal",
                    vec![],
                    HostValueType::Opaque(
                        context.intern(&host_type_identity("game.Player")).unwrap(),
                    ),
                )
                .unwrap(),
            ),
            |context, _| {
                let token = context
                    .borrows()
                    .borrow_unique(HostObjectId(1), TypeId::new(0))
                    .unwrap();
                Ok(context.runtime().gc().alloc_host_mut(token).unwrap())
            },
        ))
        .unwrap();
    assert_eq!(
        runtime.invoke_host("game.heal", &[]).unwrap_err().kind(),
        RuntimeErrorKind::HostBorrowEscape
    );
    let resources = runtime.host_scope(&[]).unwrap();
    let frame = resources.borrows();
    frame
        .borrow_unique(HostObjectId(1), TypeId::new(0))
        .unwrap();
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn registers_host_function_metadata_and_invokes_handler() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = exposed_host_runtime();
    let i32_id = runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(1),
            ..TypeRegistration::new("i32", TypeKind::Primitive)
        })
        .unwrap();
    let metadata = HostFunctionDeclaration {
        params: vec![
            HostParameter {
                name: "player".into(),
                ty: HostValueType::Opaque(
                    context.intern(&host_type_identity("game.Player")).unwrap(),
                ),
                passing: HostPassingStyle::UniqueBorrow,
            },
            HostParameter {
                name: "hp".into(),
                ty: HostValueType::I32,
                passing: HostPassingStyle::Owned,
            },
        ],

        effects: HostFunctionEffects {
            may_mutate_host_state: true,
            may_trap: true,
            ..HostFunctionEffects::default()
        },
        ..HostFunctionDeclaration::new(&context, "game.heal", vec![], HostValueType::I32).unwrap()
    };
    let metadata = scoped(&context, &metadata);
    let fingerprint = metadata
        .records()
        .fingerprint(metadata.definitions())
        .unwrap();

    let function_id = runtime
        .register_host_function(HostFunction::new(metadata, move |_, args| match args {
            [Value::HostRoot(_), Value::I32(hp)] => Ok(Value::I32(hp + i32_id.index() as i32)),
            _ => Err(HostError::new("game.heal expects host root and i32")),
        }))
        .unwrap();

    let registered = runtime.host().function("game.heal").unwrap();
    assert_eq!(function_id.index(), 0);
    assert_eq!(registered.id(), Some(function_id));
    assert_eq!(registered.declaration().symbol, "game.heal");
    assert_eq!(
        registered.declaration().params[0].passing,
        HostPassingStyle::UniqueBorrow
    );

    assert!(registered.declaration().effects.may_mutate_host_state);
    assert_eq!(
        registered
            .declaration()
            .fingerprint(registered.definitions())
            .unwrap(),
        fingerprint
    );
    let root = host_root_value(&mut runtime, 1);
    assert_eq!(
        runtime
            .invoke_host("game.heal", &[root, Value::I32(7)])
            .unwrap(),
        Value::I32(7)
    );
}

#[test]
fn installed_host_functions_are_available() {
    let context = DefinitionContext::new().unwrap();
    let calls = Arc::new(Mutex::new(0usize));
    let calls_for_host = Arc::clone(&calls);
    let mut runtime = host_call_enabled_runtime();
    runtime
        .register_host_function(HostFunction::new(
            scoped(
                &context,
                &HostFunctionDeclaration::new(&context, "game.tick", vec![], HostValueType::Unit)
                    .unwrap(),
            ),
            move |_, _| {
                *calls_for_host.lock().expect("counter should lock") += 1;
                Ok(Value::Unit)
            },
        ))
        .unwrap();

    assert_eq!(runtime.invoke_host("game.tick", &[]).unwrap(), Value::Unit);
    assert_eq!(*calls.lock().expect("counter should lock"), 1);
}

#[test]
fn rejects_duplicate_host_function_symbols() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    runtime
        .register_host_function(HostFunction::new(
            scoped(
                &context,
                &HostFunctionDeclaration::new(&context, "game.tick", vec![], HostValueType::Unit)
                    .unwrap(),
            ),
            |_, _| Ok(Value::Unit),
        ))
        .unwrap();

    let installed = runtime.host().function("game.tick").unwrap();
    let duplicate = DefinitionMetadata::checked(
        installed.definitions().clone(),
        installed.declaration().clone(),
        &CancellationToken::default(),
    )
    .unwrap();
    let error = runtime
        .register_host_function(HostFunction::new(duplicate, |_, _| Ok(Value::Unit)))
        .unwrap_err();

    assert_eq!(error.kind(), RuntimeErrorKind::MetadataConflict);
}

#[test]
fn registers_host_type_metadata_with_stable_runtime_type_identity() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let i32_id = runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(10),
            ..TypeRegistration::new("i32", TypeKind::Primitive)
        })
        .unwrap();

    use kagari_types::host_interface::{
        type_declaration::{HostFieldDeclaration, HostMethodDeclaration, HostTypeDeclaration},
        value_type::HostValueType,
    };
    let mut declaration = HostTypeDeclaration::new(&context, "game.Player").unwrap();
    declaration.ownership = HostTypeOwnership::HostRoot;
    declaration.path_access = PathAccess::ReadWrite;
    declaration.reflection = HostReflectionPolicy::Metadata;
    let mut field =
        HostFieldDeclaration::new(&context, declaration.id, "hp", HostValueType::I32).unwrap();
    field.writable = true;
    field.path_access = PathAccess::ReadWrite;
    declaration.fields.push(field);
    declaration.methods.push(
        HostMethodDeclaration::new(
            &context,
            declaration.id,
            "heal",
            vec![HostParameter {
                name: "hp".into(),
                ty: HostValueType::I32,
                passing: HostPassingStyle::Owned,
            }],
            HostValueType::I32,
        )
        .unwrap(),
    );
    let declaration = scoped(&context, &declaration);
    let fingerprint = AbiFingerprint(
        declaration
            .records()
            .fingerprint(declaration.definitions())
            .unwrap(),
    );
    let type_id = runtime
        .register_host_type(HostTypeRegistration::new(
            declaration,
            "crate::game::Player",
        ))
        .unwrap();

    let type_info = runtime.types().get(type_id).unwrap();
    let host_info = runtime.host().host_type(type_id).unwrap();
    let named_host_info = runtime.host().host_type_by_name("game.Player").unwrap();

    assert_eq!(type_info.id, type_id);
    assert_eq!(type_info.kind, TypeKind::HostObject);
    assert_eq!(type_info.fields[0].path_access, PathAccess::ReadWrite);
    assert_eq!(host_info.type_id, type_id);
    assert_eq!(named_host_info.rust_type_name, "crate::game::Player");
    assert_eq!(host_info.declaration.ownership, HostTypeOwnership::HostRoot);
    assert_eq!(
        host_info.declaration.reflection,
        HostReflectionPolicy::Metadata
    );
    assert_eq!(host_info.abi_fingerprint, fingerprint);
    assert_eq!(type_info.fields[0].ty, i32_id);
    assert_eq!(type_info.methods[0].params[0].ty, i32_id);
}

#[test]
fn rejects_duplicate_host_type_names() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    runtime
        .register_host_type(HostTypeRegistration::new(
            scoped(
                &context,
                &HostTypeDeclaration::new(&context, "game.Player").unwrap(),
            ),
            "crate::game::Player",
        ))
        .unwrap();

    let error = runtime
        .register_host_type(HostTypeRegistration::new(
            scoped(
                &context,
                &HostTypeDeclaration::new(&context, "game.Player").unwrap(),
            ),
            "crate::game::OtherPlayer",
        ))
        .unwrap_err();

    assert_eq!(error.kind(), RuntimeErrorKind::MetadataConflict);
}
