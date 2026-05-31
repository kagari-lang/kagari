use kagari_runtime::{
    AbiFingerprint, CapabilitySet, FieldInfo, FieldMetadataId, HostFunctionEffects,
    HostFunctionMetadata, HostReflectionPolicy, HostTypeOwnership, HostTypeRegistration,
    MethodInfo, MethodMetadataId, MethodOrigin, ParameterInfo, PathAccess, Runtime,
    RuntimeErrorKind, TypeKind, TypeRegistration, Visibility,
    host::{HostError, HostFunction, HostParameter, HostPassingStyle},
    value::Value,
};

#[test]
fn registers_host_function_metadata_and_invokes_handler() {
    let mut runtime = Runtime::default();
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
                [Value::HostOwned(_), Value::I32(hp)] => Ok(Value::I32(hp + i32_id.index() as i32)),
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
            .invoke_host(
                "game.heal",
                &[
                    Value::HostOwned(kagari_runtime::host::HostObjectId(1)),
                    Value::I32(7)
                ]
            )
            .unwrap(),
        Value::I32(7)
    );
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
