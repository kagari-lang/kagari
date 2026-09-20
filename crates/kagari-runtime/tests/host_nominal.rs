use kagari_common::{
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle, HostValueType,
        host_type_identity,
    },
    identity::DefinitionId,
};
use kagari_runtime::{
    CapabilitySet, HostExposurePolicy, HostObjectId, HostSchemaEpoch, HostTypeOwnership,
    HostTypeRegistration, LanguageProfile, PathAccess, Runtime, RuntimeConfig, RuntimeErrorKind,
    SecurityContext, TypeId, host::HostFunction, value::Value,
};
use std::{cell::Cell, rc::Rc};

fn runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..Default::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allow_host_functions: true,
            ..Default::default()
        },
        ..Default::default()
    })
}

fn register(runtime: &mut Runtime, symbol: &str, declaration: DefinitionId) -> (TypeId, Value) {
    let mut registration = HostTypeRegistration::new(
        kagari_common::host_interface::HostTypeDeclaration::new(symbol),
        "Object",
    );
    registration.declaration.id = declaration;
    registration.declaration.ownership = HostTypeOwnership::HostRoot;
    registration.declaration.path_access = PathAccess::ReadWrite;
    let ty = runtime.register_host_type(registration).unwrap();
    let value = Value::HostRoot(
        runtime
            .register_host_root(HostObjectId(ty.index() as u64), ty, HostSchemaEpoch::new(0))
            .unwrap(),
    );
    (ty, value)
}

fn function(ty: HostValueType, passing: HostPassingStyle) -> HostFunctionDeclaration {
    HostFunctionDeclaration::new(
        "host.take",
        vec![HostParameter {
            name: "value".into(),
            ty,
            passing,
        }],
        HostValueType::Unit,
    )
}

#[test]
fn declaration_conflicts_and_invalid_identities_do_not_partially_register_metadata() {
    let mut runtime = runtime();
    let declaration = host_type_identity("game.Player");
    let (ty, _) = register(&mut runtime, "renamed.Export", declaration.clone());
    assert_eq!(
        runtime
            .host()
            .host_type_by_declaration(&declaration)
            .unwrap()
            .type_id,
        ty
    );
    let before = runtime.types().len();
    let mut duplicate = HostTypeRegistration::new(
        kagari_common::host_interface::HostTypeDeclaration::new("other.Export"),
        "Other",
    );
    duplicate.declaration.id = declaration;
    assert_eq!(
        runtime.register_host_type(duplicate).unwrap_err().kind(),
        RuntimeErrorKind::MetadataConflict
    );
    assert_eq!(runtime.types().len(), before);
    assert!(runtime.types().get_by_name("other.Export").is_none());
    let mut invalid = HostTypeRegistration::new(
        kagari_common::host_interface::HostTypeDeclaration::new("bad.Export"),
        "Other",
    );
    invalid.declaration.id.path.clear();
    assert!(runtime.register_host_type(invalid).is_err());
    assert_eq!(runtime.types().len(), before);
    let (next, _) = register(
        &mut runtime,
        "other.Export",
        host_type_identity("other.Player"),
    );
    assert_eq!(next.index(), before);
}

#[test]
fn nested_signature_types_must_be_bound_before_program_publication() {
    use kagari_ir::bytecode::{BytecodeModule, BytecodeProgram, ModuleRef};
    let mut runtime = runtime();
    let declaration = function(
        HostValueType::Tuple(vec![HostValueType::Option(Box::new(
            HostValueType::opaque("game.Player"),
        ))]),
        HostPassingStyle::Owned,
    );
    runtime
        .register_host_function(HostFunction::new(declaration.clone(), |_, _| {
            panic!("linking cannot invoke callbacks")
        }))
        .unwrap();
    let mut expected_type = kagari_common::host_interface::HostTypeDeclaration::new("export.Alias");
    expected_type.id = host_type_identity("game.Player");
    expected_type.ownership = HostTypeOwnership::HostRoot;
    expected_type.path_access = PathAccess::ReadWrite;
    let program = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule {
            host_interface: HostInterface {
                types: vec![expected_type],
                functions: vec![declaration],
            },
            ..Default::default()
        }],
    };
    assert!(runtime.load_program("nominal", program.clone()).is_err());
    assert_eq!(runtime.modules().loaded_count(), 0);
    assert_eq!(runtime.resources().counters().loaded_modules, 0);
    register(
        &mut runtime,
        "export.Alias",
        host_type_identity("game.Player"),
    );
    runtime.load_program("nominal", program).unwrap();
}

#[test]
fn roots_and_borrows_match_nominal_declarations_instead_of_names_or_categories() {
    let mut runtime = runtime();
    let a = host_type_identity("first.Player");
    let b = host_type_identity("second.Player");
    assert_eq!(a.path, b.path);
    let (a_ty, a_value) = register(&mut runtime, "export.First", a.clone());
    let (b_ty, b_value) = register(&mut runtime, "export.Second", b);
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let id = runtime
        .register_host_function(HostFunction::new(
            function(
                HostValueType::Opaque(a.clone()),
                HostPassingStyle::SharedBorrow,
            ),
            move |_, _| {
                observed.set(observed.get() + 1);
                Ok(Value::Unit)
            },
        ))
        .unwrap();
    runtime.invoke_bound_host(id, &[a_value]).unwrap();
    assert!(
        runtime
            .invoke_bound_host(id, std::slice::from_ref(&b_value))
            .is_err()
    );
    let scope = runtime.host_scope(&[]).unwrap();
    let a_token = scope
        .borrows()
        .borrow_shared(HostObjectId(a_ty.index() as u64), a_ty)
        .unwrap();
    let b_token = scope
        .borrows()
        .borrow_shared(HostObjectId(b_ty.index() as u64), b_ty)
        .unwrap();
    runtime
        .invoke_bound_host(id, &[Value::host_ref(a_token)])
        .unwrap();
    assert!(
        runtime
            .invoke_bound_host(id, &[Value::host_ref(b_token)])
            .is_err()
    );
    assert_eq!(calls.get(), 2);
    drop(scope);

    let observed = calls.clone();
    let result = runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new(
                "host.wrong",
                vec![],
                HostValueType::Tuple(vec![HostValueType::Opaque(a)]),
            ),
            move |_, _| {
                observed.set(observed.get() + 1);
                Ok(Value::Tuple(vec![b_value.clone()]))
            },
        ))
        .unwrap();
    assert!(runtime.invoke_bound_host(result, &[]).is_err());
    assert_eq!(calls.get(), 3);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert!(!runtime.is_quarantined());
}

#[test]
fn identical_root_numbers_in_another_runtime_do_not_grant_access() {
    let mut local = runtime();
    let mut foreign = runtime();
    let (_, local_value) = register(&mut local, "game.Player", host_type_identity("game.Player"));
    let (_, foreign_value) = register(
        &mut foreign,
        "game.Player",
        host_type_identity("game.Player"),
    );
    let (Value::HostRoot(a), Value::HostRoot(b)) = (&local_value, &foreign_value) else {
        unreachable!()
    };
    assert_eq!(a.object_id(), b.object_id());
    assert_eq!(a.type_id(), b.type_id());
    assert_eq!(a.schema_epoch(), b.schema_epoch());
    assert_eq!(a.abi_fingerprint(), b.abi_fingerprint());
    assert_ne!(a, b);
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let id = local
        .register_host_function(HostFunction::new(
            function(
                HostValueType::Tuple(vec![HostValueType::opaque("game.Player")]),
                HostPassingStyle::Owned,
            ),
            move |_, _| {
                observed.set(observed.get() + 1);
                Ok(Value::Unit)
            },
        ))
        .unwrap();
    local
        .invoke_bound_host(id, &[Value::Tuple(vec![local_value])])
        .unwrap();
    assert!(
        local
            .invoke_bound_host(id, &[Value::Tuple(vec![foreign_value.clone()])])
            .is_err()
    );
    assert!(
        local
            .host_scope(std::slice::from_ref(&foreign_value))
            .is_err()
    );
    assert_eq!(calls.get(), 1);
    let observed = calls.clone();
    let result = local
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new(
                "host.foreign",
                vec![],
                HostValueType::opaque("game.Player"),
            ),
            move |_, _| {
                observed.set(observed.get() + 1);
                Ok(foreign_value.clone())
            },
        ))
        .unwrap();
    assert!(local.invoke_bound_host(result, &[]).is_err());
    assert_eq!(calls.get(), 2);
    assert_eq!(local.gc().active_roots(), 0);
    assert!(!local.is_quarantined());
}

#[test]
fn foreign_path_views_cannot_be_chained_through_matching_local_slots() {
    use kagari_runtime::{
        AbiFingerprint, DynamicPathArguments, HostPathDescriptorRegistration,
        HostPathSegmentRegistration,
    };
    let mut local = runtime();
    let mut foreign = runtime();
    let mut views = Vec::new();
    for runtime in [&mut local, &mut foreign] {
        let (ty, Value::HostRoot(root)) =
            register(runtime, "game.Player", host_type_identity("game.Player"))
        else {
            unreachable!()
        };
        let descriptor = runtime
            .register_host_path_descriptor(HostPathDescriptorRegistration {
                root_type: ty,
                result_type: ty,
                segments: vec![HostPathSegmentRegistration::Virtual {
                    name: "self".into(),
                    result_type: ty,
                    access: PathAccess::ReadOnly,
                    abi_fingerprint: AbiFingerprint(1),
                }],
                access: PathAccess::ReadOnly,
                schema_epoch: HostSchemaEpoch::new(0),
                capability_requirements: Default::default(),
            })
            .unwrap();
        let view = runtime
            .host()
            .make_path_view(root, descriptor, DynamicPathArguments::empty())
            .unwrap();
        views.push(view);
    }
    assert_eq!(views[0].descriptor_id(), views[1].descriptor_id());
    assert_eq!(views[0].result_type(), views[1].result_type());
    let id = views[0].descriptor_id();
    let local_view = Value::HostPathView(views[0].clone());
    let foreign_view = Value::HostPathView(views[1].clone());
    assert!(
        local
            .host()
            .make_path_view_from_value(&local_view, id, vec![])
            .is_ok()
    );
    assert!(
        local
            .host()
            .make_path_view_from_value(&foreign_view, id, vec![])
            .is_err()
    );
    assert!(local.host_scope(&[local_view]).is_ok());
    assert!(local.host_scope(&[foreign_view]).is_err());
}
#[test]
fn path_fields_are_derived_from_nominal_declarations() {
    use kagari_common::host_interface::{HostFieldDeclaration, HostTypeDeclaration};
    use kagari_runtime::{
        HostPathDescriptorRegistration, HostPathSegment, HostPathSegmentRegistration, Visibility,
    };
    let mut runtime = runtime();
    let mut owner = HostTypeDeclaration::new("game.Player");
    owner.ownership = HostTypeOwnership::HostRoot;
    owner.path_access = PathAccess::ReadWrite;
    let mut hp = HostFieldDeclaration::new(&owner.id, "hp", HostValueType::I32);
    hp.path_access = PathAccess::ReadOnly;
    let mut hidden = HostFieldDeclaration::new(&owner.id, "hidden", HostValueType::I32);
    hidden.path_access = PathAccess::ReadOnly;
    hidden.visibility = Visibility::Private;
    let disabled = HostFieldDeclaration::new(&owner.id, "disabled", HostValueType::I32);
    owner.fields = vec![hp.clone(), hidden.clone(), disabled.clone()];
    let owner_type = runtime
        .register_host_type(HostTypeRegistration::new(owner.clone(), "Player"))
        .unwrap();
    let scalar = runtime.types().get(owner_type).unwrap().fields[0].ty;
    let registration = |declaration, access, result_type| HostPathDescriptorRegistration {
        root_type: owner_type,
        result_type,
        segments: vec![HostPathSegmentRegistration::Field { declaration }],
        access,
        schema_epoch: HostSchemaEpoch::new(0),
        capability_requirements: CapabilitySet::default(),
    };
    let other = HostTypeDeclaration::new("other.Player");
    let foreign_hp = HostFieldDeclaration::new(&other.id, "hp", HostValueType::I32);
    for bad in [
        registration(foreign_hp.id, PathAccess::ReadOnly, scalar),
        registration(hidden.id, PathAccess::ReadOnly, scalar),
        registration(disabled.id, PathAccess::ReadOnly, scalar),
        registration(hp.id.clone(), PathAccess::ReadWrite, scalar),
        registration(hp.id.clone(), PathAccess::ReadOnly, owner_type),
    ] {
        assert_eq!(
            runtime
                .register_host_path_descriptor(bad)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::TypedPathValidation
        );
        assert_eq!(runtime.host().path_descriptors().count(), 0);
    }
    let id = runtime
        .register_host_path_descriptor(registration(hp.id, PathAccess::ReadOnly, scalar))
        .unwrap();
    let descriptor = runtime.host().path_descriptor(id).unwrap();
    let metadata = runtime.types().get(owner_type).unwrap();
    assert_eq!(
        descriptor.segments,
        vec![HostPathSegment::Field {
            name: "hp".into(),
            field_id: metadata.fields[0].id,
            owner_type,
            result_type: scalar,
            access: PathAccess::ReadOnly,
            abi_fingerprint: metadata.fields[0].abi_fingerprint,
        }]
    );
}
#[test]
fn path_fingerprints_ignore_runtime_slots_and_track_contract_changes() {
    use kagari_common::host_interface::{HostFieldDeclaration, HostTypeDeclaration};
    use kagari_runtime::{
        HostPathDescriptorRegistration, HostPathSegmentRegistration, TypeKind, TypeRegistration,
    };
    let fingerprint = |padding: usize,
                       docs: &str,
                       writable: bool,
                       epoch: usize,
                       capability: bool,
                       ty: HostValueType| {
        let mut runtime = runtime();
        for index in 0..padding {
            runtime
                .types()
                .register(TypeRegistration::new(
                    format!("unused{index}"),
                    TypeKind::Primitive,
                ))
                .unwrap();
        }
        let mut owner = HostTypeDeclaration::new("game.Player");
        owner.ownership = HostTypeOwnership::HostRoot;
        owner.path_access = PathAccess::ReadWrite;
        owner.documentation = docs.into();
        let mut field = HostFieldDeclaration::new(&owner.id, "生命", ty);
        field.writable = writable;
        field.path_access = if writable {
            PathAccess::ReadWrite
        } else {
            PathAccess::ReadOnly
        };
        field.documentation = docs.into();
        let declaration = field.id.clone();
        owner.fields.push(field);
        let root_type = runtime
            .register_host_type(HostTypeRegistration::new(owner, "Player"))
            .unwrap();
        let result_type = runtime.types().get(root_type).unwrap().fields[0].ty;
        let id = runtime
            .register_host_path_descriptor(HostPathDescriptorRegistration {
                root_type,
                result_type,
                segments: vec![HostPathSegmentRegistration::Field { declaration }],
                access: PathAccess::ReadOnly,
                schema_epoch: HostSchemaEpoch::new(epoch),
                capability_requirements: CapabilitySet {
                    fs_read: capability,
                    ..Default::default()
                },
            })
            .unwrap();
        runtime.host().path_descriptor(id).unwrap().abi_fingerprint
    };
    let base = fingerprint(0, "", false, 0, false, HostValueType::I32);
    assert_eq!(
        base,
        fingerprint(3, "changed docs", false, 0, false, HostValueType::I32)
    );
    assert_ne!(base, fingerprint(0, "", true, 0, false, HostValueType::I32));
    assert_ne!(
        base,
        fingerprint(0, "", false, 1, false, HostValueType::I32)
    );
    assert_ne!(base, fingerprint(0, "", false, 0, true, HostValueType::I32));
    assert_ne!(
        base,
        fingerprint(0, "", false, 0, false, HostValueType::I64)
    );
}
#[test]
fn paths_reject_types_without_portable_contracts_before_publication() {
    use kagari_runtime::{
        AbiFingerprint, HostPathDescriptorRegistration, HostPathSegmentRegistration, TypeKind,
        TypeRegistration,
    };
    let mut runtime = runtime();
    let mut owner = kagari_common::host_interface::HostTypeDeclaration::new("game.Player");
    owner.ownership = HostTypeOwnership::HostRoot;
    owner.path_access = PathAccess::ReadOnly;
    let root_type = runtime
        .register_host_type(HostTypeRegistration::new(owner, "Player"))
        .unwrap();
    let result_type = runtime
        .types()
        .register(TypeRegistration::new("Unspecified", TypeKind::Primitive))
        .unwrap();
    let result = runtime.register_host_path_descriptor(HostPathDescriptorRegistration {
        root_type,
        result_type,
        segments: vec![HostPathSegmentRegistration::Virtual {
            name: "value".into(),
            result_type,
            access: PathAccess::ReadOnly,
            abi_fingerprint: AbiFingerprint(1),
        }],
        access: PathAccess::ReadOnly,
        schema_epoch: HostSchemaEpoch::new(0),
        capability_requirements: Default::default(),
    });
    assert_eq!(
        result.unwrap_err().kind(),
        RuntimeErrorKind::TypedPathValidation
    );
    assert_eq!(runtime.host().path_descriptors().count(), 0);
}
