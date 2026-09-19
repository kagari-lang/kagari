use std::sync::{Arc, Mutex};

use kagari_runtime::{
    AbiFingerprint, CapabilitySet, FieldInfo, FieldMetadataId, HostExposurePolicy,
    HostFunctionDeclaration, HostFunctionEffects, HostReflectionPolicy, HostTypeOwnership,
    HostTypeRegistration, LanguageProfile, MethodInfo, MethodMetadataId, MethodOrigin,
    ParameterInfo, PathAccess, Runtime, RuntimeConfig, RuntimeErrorKind, SecurityContext, TypeId,
    TypeKind, TypeRegistration, Visibility,
    host::{
        HostError, HostFunction, HostObjectId, HostParameter, HostPassingStyle, HostSchemaEpoch,
    },
    value::Value,
};

fn host_root_value(runtime: &mut Runtime, object_id: u64) -> Value {
    let mut registration = HostTypeRegistration::new("game.Player", "Player");
    registration.ownership = HostTypeOwnership::HostRoot;
    registration.path_access = PathAccess::ReadWrite;
    let ty = runtime.register_host_type(registration).unwrap();
    Value::HostRoot(
        runtime
            .register_host_root(HostObjectId(object_id), ty, HostSchemaEpoch::new(0))
            .unwrap(),
    )
}

fn exposed_host_runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                allow_reflection: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                reflection_read: true,
                ..CapabilitySet::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_functions: vec!["game.heal".to_owned()],
            ..HostExposurePolicy::default()
        },
        ..RuntimeConfig::default()
    })
}

fn host_call_enabled_runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..CapabilitySet::default()
            },
        },
        ..RuntimeConfig::default()
    })
}

#[test]
fn callback_context_releases_borrows_and_rejects_borrowed_results() {
    let mut runtime = exposed_host_runtime();
    host_root_value(&mut runtime, 1);
    runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new(
                "game.heal",
                vec![],
                kagari_common::host_interface::HostValueType::opaque("game.Player"),
            ),
            |context, _| {
                let token = context
                    .borrows()
                    .borrow_unique(HostObjectId(1), TypeId::new(0))
                    .unwrap();
                Ok(Value::Ephemeral(
                    kagari_runtime::value::EphemeralValue::HostMut(token),
                ))
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
                ty: kagari_common::host_interface::HostValueType::opaque("game.Player"),
                passing: HostPassingStyle::UniqueBorrow,
            },
            HostParameter {
                name: "hp".into(),
                ty: kagari_common::host_interface::HostValueType::I32,
                passing: HostPassingStyle::Owned,
            },
        ],
        capability_requirements: CapabilitySet {
            reflection_read: true,
            ..CapabilitySet::default()
        },
        resource_cost_hint: Some(5),
        effects: HostFunctionEffects {
            may_mutate_host_state: true,
            may_trap: true,
            ..HostFunctionEffects::default()
        },
        ..HostFunctionDeclaration::new(
            "game.heal",
            vec![],
            kagari_common::host_interface::HostValueType::I32,
        )
    };
    let fingerprint = metadata.fingerprint().unwrap();

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
    assert_eq!(registered.declaration().resource_cost_hint, Some(5));
    assert!(registered.declaration().effects.may_mutate_host_state);
    assert_eq!(registered.declaration().fingerprint().unwrap(), fingerprint);
    let root = host_root_value(&mut runtime, 1);
    assert_eq!(
        runtime
            .invoke_host("game.heal", &[root, Value::I32(7)])
            .unwrap(),
        Value::I32(7)
    );
}

#[test]
fn host_functions_are_unavailable_until_exposed() {
    let calls = Arc::new(Mutex::new(0usize));
    let calls_for_host = Arc::clone(&calls);
    let mut runtime = host_call_enabled_runtime();
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::HostFunctionDeclaration::new(
                "game.tick",
                vec![],
                kagari_common::host_interface::HostValueType::Unit,
            ),
            move |_, _| {
                *calls_for_host.lock().expect("counter should lock") += 1;
                Ok(Value::Unit)
            },
        ))
        .unwrap();

    let error = runtime.invoke_host("game.tick", &[]).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::CapabilityDenied);
    assert_eq!(*calls.lock().expect("counter should lock"), 0);

    runtime.set_host_exposure_policy(HostExposurePolicy {
        allowed_host_functions: vec!["game.tick".to_owned()],
        ..HostExposurePolicy::default()
    });

    assert_eq!(runtime.invoke_host("game.tick", &[]).unwrap(), Value::Unit);
    assert_eq!(*calls.lock().expect("counter should lock"), 1);
}

#[test]
fn rejects_duplicate_host_function_symbols() {
    let mut runtime = Runtime::default();
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::HostFunctionDeclaration::new(
                "game.tick",
                vec![],
                kagari_common::host_interface::HostValueType::Unit,
            ),
            |_, _| Ok(Value::Unit),
        ))
        .unwrap();

    let error = runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::HostFunctionDeclaration::new(
                "game.tick",
                vec![],
                kagari_common::host_interface::HostValueType::Unit,
            ),
            |_, _| Ok(Value::Unit),
        ))
        .unwrap_err();

    assert_eq!(error.kind(), RuntimeErrorKind::MetadataConflict);
}

#[test]
fn registers_host_type_metadata_with_stable_runtime_type_identity() {
    let mut runtime = Runtime::default();
    let i32_id = runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(10),
            ..TypeRegistration::new("i32", TypeKind::Primitive)
        })
        .unwrap();

    let type_id = runtime
        .register_host_type(HostTypeRegistration {
            ownership: HostTypeOwnership::HostRoot,
            fields: vec![FieldInfo {
                id: FieldMetadataId::new(0),
                name: "hp".to_owned(),
                ty: i32_id,
                readable: true,
                writable: true,
                visibility: Visibility::Public,
                path_access: PathAccess::ReadWrite,
                abi_fingerprint: AbiFingerprint(11),
            }],
            methods: vec![MethodInfo {
                id: MethodMetadataId::new(0),
                name: "heal".to_owned(),
                params: vec![ParameterInfo {
                    name: "hp".to_owned(),
                    ty: i32_id,
                }],
                return_type: i32_id,
                origin: MethodOrigin::Host,
                capability_requirements: CapabilitySet::default(),
                abi_fingerprint: AbiFingerprint(12),
            }],
            path_access: PathAccess::ReadWrite,
            reflection: HostReflectionPolicy::Metadata,
            abi_fingerprint: AbiFingerprint(13),
            ..HostTypeRegistration::new("game.Player", "crate::game::Player")
        })
        .unwrap();

    let type_info = runtime.types().get(type_id).unwrap();
    let host_info = runtime.host().host_type(type_id).unwrap();
    let named_host_info = runtime.host().host_type_by_name("game.Player").unwrap();

    assert_eq!(type_info.id, type_id);
    assert_eq!(type_info.kind, TypeKind::HostObject);
    assert_eq!(type_info.fields[0].path_access, PathAccess::ReadWrite);
    assert_eq!(host_info.type_id, type_id);
    assert_eq!(named_host_info.rust_type_name, "crate::game::Player");
    assert_eq!(host_info.ownership, HostTypeOwnership::HostRoot);
    assert_eq!(host_info.reflection, HostReflectionPolicy::Metadata);
    assert_eq!(host_info.abi_fingerprint, AbiFingerprint(13));
}

#[test]
fn rejects_duplicate_host_type_names() {
    let mut runtime = Runtime::default();
    runtime
        .register_host_type(HostTypeRegistration::new(
            "game.Player",
            "crate::game::Player",
        ))
        .unwrap();

    let error = runtime
        .register_host_type(HostTypeRegistration::new(
            "game.Player",
            "crate::game::OtherPlayer",
        ))
        .unwrap_err();

    assert_eq!(error.kind(), RuntimeErrorKind::MetadataConflict);
}
