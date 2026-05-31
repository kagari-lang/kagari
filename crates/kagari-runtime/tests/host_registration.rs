use std::sync::{Arc, Mutex};

use kagari_runtime::{
    AbiFingerprint, CapabilitySet, FieldInfo, FieldMetadataId, HostExposurePolicy,
    HostFunctionEffects, HostFunctionMetadata, HostReflectionPolicy, HostTypeOwnership,
    HostTypeRegistration, LanguageProfile, MethodInfo, MethodMetadataId, MethodOrigin,
    ParameterInfo, PathAccess, Runtime, RuntimeConfig, RuntimeErrorKind, SecurityContext, TypeId,
    TypeKind, TypeRegistration, Visibility,
    host::{
        HostError, HostFunction, HostObjectId, HostParameter, HostPassingStyle, HostRootHandle,
        HostSchemaEpoch,
    },
    value::Value,
};

fn host_root_value(object_id: u64) -> Value {
    Value::HostRoot(HostRootHandle::new(
        HostObjectId(object_id),
        TypeId::new(0),
        HostSchemaEpoch::new(0),
        AbiFingerprint(1),
    ))
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
fn registers_host_function_metadata_and_invokes_handler() {
    let mut runtime = exposed_host_runtime();
    let i32_id = runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(1),
            ..TypeRegistration::new("i32", TypeKind::Primitive)
        })
        .unwrap();
    let metadata = HostFunctionMetadata {
        symbol: "game.heal",
        params: vec![
            HostParameter {
                name: "player",
                type_name: "game.Player",
                passing: HostPassingStyle::UniqueBorrow,
            },
            HostParameter {
                name: "hp",
                type_name: "i32",
                passing: HostPassingStyle::Owned,
            },
        ],
        return_type: "i32",
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
        abi_fingerprint: AbiFingerprint(55),
    };

    let function_id = runtime
        .register_host_function(HostFunction::with_metadata(
            metadata,
            move |args| match args {
                [Value::HostRoot(_), Value::I32(hp)] => Ok(Value::I32(hp + i32_id.index() as i32)),
                _ => Err(HostError::new("game.heal expects host root and i32")),
            },
        ))
        .unwrap();

    let registered = runtime.host().function("game.heal").unwrap();
    assert_eq!(function_id.index(), 0);
    assert_eq!(registered.id(), Some(function_id));
    assert_eq!(registered.metadata().symbol, "game.heal");
    assert_eq!(
        registered.metadata().params[0].passing,
        HostPassingStyle::UniqueBorrow
    );
    assert_eq!(registered.metadata().resource_cost_hint, Some(5));
    assert!(registered.metadata().effects.may_mutate_host_state);
    assert_eq!(registered.metadata().abi_fingerprint, AbiFingerprint(55));
    assert_eq!(
        runtime
            .invoke_host("game.heal", &[host_root_value(1), Value::I32(7)])
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
        .register_host_function(HostFunction::new("game.tick", vec![], "()", move |_| {
            *calls_for_host.lock().expect("counter should lock") += 1;
            Ok(Value::Unit)
        }))
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
        .register_host_function(HostFunction::new("game.tick", vec![], "()", |_| {
            Ok(Value::Unit)
        }))
        .unwrap();

    let error = runtime
        .register_host_function(HostFunction::new("game.tick", vec![], "()", |_| {
            Ok(Value::Unit)
        }))
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
