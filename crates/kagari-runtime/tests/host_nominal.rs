use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext, metadata::DefinitionMetadata, scoped::DefinitionReferences,
        table::DefinitionId,
    },
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    host::{
        DynamicPathArguments, HostFunction, HostObjectId, HostPathDescriptorRegistration,
        HostPathSegment, HostSchemaEpoch, HostTypeRegistration,
    },
    metadata::{TypeId, TypeKind, TypeRegistration},
    value::Value,
};
use kagari_types::ty::Ty;
use kagari_types::{
    declaration::host::{HostPassingStyle, HostTypeOwnership, PathAccess},
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, host_type_identity,
        path::{HostPathDeclaration, HostPathSegmentDeclaration, HostVirtualSegmentDeclaration},
        type_declaration::{HostFieldDeclaration, HostTypeDeclaration, Visibility},
        value_type::HostValueType,
    },
    language::binding,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

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

fn path_registration(
    context: &DefinitionContext,
    root_type: TypeId,
    result_type: TypeId,
    segments: Vec<HostPathSegmentDeclaration>,
    access: PathAccess,
    schema_epoch: HostSchemaEpoch,
) -> HostPathDescriptorRegistration {
    let (definitions, segments) = scoped(context, &segments).into_parts();
    HostPathDescriptorRegistration {
        definitions,
        root_type,
        result_type,
        segments,
        access,
        schema_epoch,
    }
}

fn runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        ..Default::default()
    })
}

fn register(
    context: &DefinitionContext,
    runtime: &mut Runtime,
    symbol: &str,
    declaration: DefinitionId,
) -> (TypeId, Value) {
    let mut input = HostTypeDeclaration::new(context, symbol).unwrap();
    input.id = declaration;
    input.ownership = HostTypeOwnership::HostRoot;
    input.path_access = PathAccess::ReadWrite;
    let ty = runtime
        .register_host_type(HostTypeRegistration::new(scoped(context, &input), "Object"))
        .unwrap();
    let root = runtime
        .register_host_root(HostObjectId(ty.index() as u64), ty, HostSchemaEpoch::new(0))
        .unwrap();
    let value = runtime.gc().alloc_host_root(root).unwrap();
    (ty, value)
}

fn function(
    context: &DefinitionContext,
    ty: HostValueType,
    passing: HostPassingStyle,
) -> HostFunctionDeclaration {
    HostFunctionDeclaration::new(
        context,
        "host.take",
        vec![HostParameter {
            name: "value".into(),
            ty,
            passing,
        }],
        HostValueType::Unit,
    )
    .unwrap()
}

#[test]
fn declaration_conflicts_and_invalid_identities_do_not_partially_register_metadata() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = runtime();
    let declaration = context.intern(&host_type_identity("game.Player")).unwrap();
    let (ty, _) = register(&context, &mut runtime, "renamed.Export", declaration);
    assert_eq!(
        runtime
            .host()
            .host_type_by_declaration(
                &runtime
                    .host()
                    .definitions()
                    .lookup(&host_type_identity("game.Player"))
                    .unwrap()
            )
            .unwrap()
            .type_id,
        ty
    );
    let before = runtime.types().len();
    let mut duplicate = HostTypeDeclaration::new(&context, "other.Export").unwrap();
    duplicate.id = declaration;
    let duplicate = HostTypeRegistration::new(scoped(&context, &duplicate), "Other");
    assert_eq!(
        runtime.register_host_type(duplicate).unwrap_err().kind(),
        RuntimeErrorKind::MetadataConflict
    );
    assert_eq!(runtime.types().len(), before);
    assert!(runtime.types().get_by_name("other.Export").is_none());
    let mut invalid = HostTypeDeclaration::new(&context, "bad.Export").unwrap();
    invalid.id = context
        .intern_root(context.snapshot().resolve(invalid.id).unwrap().module())
        .unwrap();
    let invalid = HostTypeRegistration::new(scoped(&context, &invalid), "Other");
    assert!(runtime.register_host_type(invalid).is_err());
    assert_eq!(runtime.types().len(), before);
    let (next, _) = register(
        &context,
        &mut runtime,
        "other.Export",
        context.intern(&host_type_identity("other.Player")).unwrap(),
    );
    assert_eq!(next.index(), before);
}

#[test]
fn nested_signature_types_must_be_bound_before_program_publication() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = runtime();
    let declaration = function(
        &context,
        HostValueType::Tuple(vec![HostValueType::Option(
            context.intern(&binding::option_declaration()).unwrap(),
            Box::new(HostValueType::Opaque(
                context.intern(&host_type_identity("game.Player")).unwrap(),
            )),
        )]),
        HostPassingStyle::Owned,
    );
    runtime
        .register_host_function(HostFunction::new(scoped(&context, &declaration), |_, _| {
            panic!("linking cannot invoke callbacks")
        }))
        .unwrap();
    let mut expected_type = HostTypeDeclaration::new(&context, "export.Alias").unwrap();
    expected_type.id = context.intern(&host_type_identity("game.Player")).unwrap();
    expected_type.ownership = HostTypeOwnership::HostRoot;
    expected_type.path_access = PathAccess::ReadWrite;
    let program = scoped(
        &context,
        &BytecodeProgram {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                host_interface: HostInterface {
                    paths: vec![],
                    types: vec![expected_type],
                    functions: vec![declaration],
                },
                ..Default::default()
            }],
        },
    );
    assert!(runtime.load_program("nominal", program.clone()).is_err());
    assert_eq!(runtime.modules().loaded_count(), 0);
    register(
        &context,
        &mut runtime,
        "export.Alias",
        context.intern(&host_type_identity("game.Player")).unwrap(),
    );
    runtime.load_program("nominal", program).unwrap();
}

#[test]
fn roots_and_borrows_match_nominal_declarations_instead_of_names_or_categories() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = runtime();
    let a = context.intern(&host_type_identity("first.Player")).unwrap();
    let b = context
        .intern(&host_type_identity("second.Player"))
        .unwrap();
    assert!(
        context.snapshot().resolve(a).unwrap().segments().eq(context
            .snapshot()
            .resolve(b)
            .unwrap()
            .segments())
    );
    let (a_ty, a_value) = register(&context, &mut runtime, "export.First", a);
    let (b_ty, b_value) = register(&context, &mut runtime, "export.Second", b);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let id = runtime
        .register_host_function(HostFunction::new(
            scoped(
                &context,
                &function(
                    &context,
                    HostValueType::Opaque(a),
                    HostPassingStyle::SharedBorrow,
                ),
            ),
            move |_, _| {
                observed.fetch_add(1, Ordering::SeqCst);
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
        .invoke_bound_host(id, &[runtime.gc().alloc_host_ref(a_token).unwrap()])
        .unwrap();
    assert!(
        runtime
            .invoke_bound_host(id, &[runtime.gc().alloc_host_ref(b_token).unwrap()])
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    drop(scope);

    let observed = calls.clone();
    let result = runtime
        .register_host_function(HostFunction::new(
            scoped(
                &context,
                &HostFunctionDeclaration::new(
                    &context,
                    "host.wrong",
                    vec![],
                    HostValueType::Tuple(vec![HostValueType::Opaque(a)]),
                )
                .unwrap(),
            ),
            move |cx, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(cx.runtime().gc().alloc_tuple(vec![b_value]).unwrap())
            },
        ))
        .unwrap();
    assert!(runtime.invoke_bound_host(result, &[]).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert!(!runtime.is_quarantined());
}

#[test]
fn identical_root_numbers_in_another_runtime_do_not_grant_access() {
    let context = DefinitionContext::new().unwrap();
    let mut local = runtime();
    let mut foreign = runtime();
    let (_, local_value) = register(
        &context,
        &mut local,
        "game.Player",
        context.intern(&host_type_identity("game.Player")).unwrap(),
    );
    let (_, foreign_value) = register(
        &context,
        &mut foreign,
        "game.Player",
        context.intern(&host_type_identity("game.Player")).unwrap(),
    );
    let (Value::HostRoot(a), Value::HostRoot(b)) = (&local_value, &foreign_value) else {
        unreachable!()
    };
    let a = local.gc().host_root(*a).unwrap();
    let b = foreign.gc().host_root(*b).unwrap();
    assert_eq!(a.object_id(), b.object_id());
    assert_eq!(a.type_id(), b.type_id());
    assert_eq!(a.schema_epoch(), b.schema_epoch());
    assert_eq!(a.abi_fingerprint(), b.abi_fingerprint());
    assert_ne!(a, b);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let id = local
        .register_host_function(HostFunction::new(
            scoped(
                &context,
                &function(
                    &context,
                    HostValueType::Tuple(vec![HostValueType::Opaque(
                        context.intern(&host_type_identity("game.Player")).unwrap(),
                    )]),
                    HostPassingStyle::Owned,
                ),
            ),
            move |_, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(Value::Unit)
            },
        ))
        .unwrap();
    local
        .invoke_bound_host(id, &[local.gc().alloc_tuple(vec![local_value]).unwrap()])
        .unwrap();
    assert!(
        local
            .invoke_bound_host(
                id,
                &[foreign.gc().alloc_tuple(vec![foreign_value]).unwrap()]
            )
            .is_err()
    );
    assert!(
        local
            .host_scope(std::slice::from_ref(&foreign_value))
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let observed = calls.clone();
    let result = local
        .register_host_function(HostFunction::new(
            scoped(
                &context,
                &HostFunctionDeclaration::new(
                    &context,
                    "host.foreign",
                    vec![],
                    HostValueType::Opaque(
                        context.intern(&host_type_identity("game.Player")).unwrap(),
                    ),
                )
                .unwrap(),
            ),
            move |_, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(foreign_value)
            },
        ))
        .unwrap();
    assert!(local.invoke_bound_host(result, &[]).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(local.gc().active_roots(), 0);
    assert!(!local.is_quarantined());
}

#[test]
fn foreign_path_views_cannot_be_chained_through_matching_local_slots() {
    let context = DefinitionContext::new().unwrap();
    let mut local = runtime();
    let mut foreign = runtime();
    let mut views = Vec::new();
    for runtime in [&mut local, &mut foreign] {
        let (ty, Value::HostRoot(root)) = register(
            &context,
            runtime,
            "game.Player",
            context.intern(&host_type_identity("game.Player")).unwrap(),
        ) else {
            unreachable!()
        };
        let descriptor = runtime
            .register_host_path_descriptor(path_registration(
                &context,
                ty,
                ty,
                vec![HostPathSegmentDeclaration::Virtual(
                    HostVirtualSegmentDeclaration {
                        name: "self".into(),
                        result: Ty::from_host_type(
                            &(HostValueType::Opaque(
                                context.intern(&host_type_identity("game.Player")).unwrap(),
                            )),
                        ),
                        access: PathAccess::ReadOnly,
                    },
                )],
                PathAccess::ReadOnly,
                HostSchemaEpoch::new(0),
            ))
            .unwrap();
        let view = runtime
            .host()
            .make_path_view(
                runtime.gc(),
                runtime.gc().host_root(root).unwrap(),
                descriptor,
                DynamicPathArguments::empty(),
            )
            .unwrap();
        views.push(view);
    }
    assert_eq!(views[0].descriptor_id(), views[1].descriptor_id());
    assert_eq!(views[0].result_type(), views[1].result_type());
    let id = views[0].descriptor_id();
    let local_view = local.gc().alloc_host_path(views[0].clone()).unwrap();
    let foreign_view = foreign.gc().alloc_host_path(views[1].clone()).unwrap();
    assert!(
        local
            .host()
            .make_path_view_from_value(local.gc(), &local_view, id, vec![])
            .is_ok()
    );
    assert!(
        local
            .host()
            .make_path_view_from_value(local.gc(), &foreign_view, id, vec![])
            .is_err()
    );
    assert!(local.host_scope(&[local_view]).is_ok());
    assert!(local.host_scope(&[foreign_view]).is_err());
}

#[test]
fn path_fields_are_derived_from_nominal_declarations() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = runtime();
    let mut owner = HostTypeDeclaration::new(&context, "game.Player").unwrap();
    owner.ownership = HostTypeOwnership::HostRoot;
    owner.path_access = PathAccess::ReadWrite;
    let mut hp = HostFieldDeclaration::new(&context, owner.id, "hp", HostValueType::I32).unwrap();
    hp.path_access = PathAccess::ReadOnly;
    let mut hidden =
        HostFieldDeclaration::new(&context, owner.id, "hidden", HostValueType::I32).unwrap();
    hidden.path_access = PathAccess::ReadOnly;
    hidden.visibility = Visibility::Private;
    let disabled =
        HostFieldDeclaration::new(&context, owner.id, "disabled", HostValueType::I32).unwrap();
    owner.fields = vec![hp.clone(), hidden.clone(), disabled.clone()];
    let catalog = HostInterface {
        paths: vec![HostPathDeclaration {
            root: owner.id,
            segments: vec![HostPathSegmentDeclaration::Field(hp.id)],
            access: PathAccess::ReadOnly,
            schema_epoch: 0,
        }],
        types: vec![owner.clone()],
        functions: vec![],
    };
    let catalog = scoped(&context, &catalog);
    let offline =
        HostInterface::from_bytes(&catalog.records().to_bytes(catalog.definitions()).unwrap())
            .unwrap()
            .import_into(&context, &CancellationToken::default())
            .unwrap();
    let contract = offline.records().paths[0]
        .contract(
            &offline.records().path_types(offline.definitions()).unwrap(),
            offline.definitions(),
        )
        .unwrap();
    for (field, access) in [
        (&hidden.id, PathAccess::ReadOnly),
        (&disabled.id, PathAccess::ReadOnly),
        (&hp.id, PathAccess::ReadWrite),
    ] {
        assert!(
            offline
                .records()
                .path_contract(
                    &HostPathDeclaration {
                        segments: vec![HostPathSegmentDeclaration::Field(*field)],
                        access,
                        ..offline.records().paths[0].clone()
                    },
                    offline.definitions()
                )
                .is_err()
        );
    }
    let owner_type = runtime
        .register_host_type(HostTypeRegistration::new(
            scoped(&context, &owner),
            "Player",
        ))
        .unwrap();
    let scalar = runtime.types().get(owner_type).unwrap().fields[0].ty;
    let registration = |declaration, access, result_type| {
        path_registration(
            &context,
            owner_type,
            result_type,
            vec![HostPathSegmentDeclaration::Field(declaration)],
            access,
            HostSchemaEpoch::new(0),
        )
    };
    let other = HostTypeDeclaration::new(&context, "other.Player").unwrap();
    let foreign_hp =
        HostFieldDeclaration::new(&context, other.id, "hp", HostValueType::I32).unwrap();
    for bad in [
        registration(foreign_hp.id, PathAccess::ReadOnly, scalar),
        registration(hidden.id, PathAccess::ReadOnly, scalar),
        registration(disabled.id, PathAccess::ReadOnly, scalar),
        registration(hp.id, PathAccess::ReadWrite, scalar),
        registration(hp.id, PathAccess::ReadOnly, owner_type),
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
    assert!(
        runtime
            .host()
            .link_interface(offline.records(), offline.definitions())
            .is_err()
    );
    let id = runtime
        .register_host_path(
            &DefinitionMetadata::checked(
                offline.definitions().clone(),
                offline.records().paths[0].clone(),
                &CancellationToken::default(),
            )
            .unwrap(),
        )
        .unwrap();
    assert!(
        runtime
            .host()
            .link_interface(offline.records(), offline.definitions())
            .is_ok()
    );
    let installed = runtime.host().interface();
    assert_eq!(
        installed
            .records()
            .to_bytes(installed.definitions())
            .unwrap(),
        offline.records().to_bytes(offline.definitions()).unwrap()
    );
    let descriptor = runtime.host().path_descriptor(id).unwrap();
    assert_eq!(
        descriptor.abi_fingerprint.0,
        contract.fingerprint(offline.definitions()).unwrap()
    );
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
    let context = DefinitionContext::new().unwrap();
    let fingerprint =
        |padding: usize, docs: &str, writable: bool, epoch: usize, ty: HostValueType| {
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
            let mut owner = HostTypeDeclaration::new(&context, "game.Player").unwrap();
            owner.ownership = HostTypeOwnership::HostRoot;
            owner.path_access = PathAccess::ReadWrite;
            owner.documentation = docs.into();
            let mut field = HostFieldDeclaration::new(&context, owner.id, "生命", ty).unwrap();
            field.writable = writable;
            field.path_access = if writable {
                PathAccess::ReadWrite
            } else {
                PathAccess::ReadOnly
            };
            field.documentation = docs.into();
            let declaration = field.id;
            owner.fields.push(field);
            let root_type = runtime
                .register_host_type(HostTypeRegistration::new(
                    scoped(&context, &owner),
                    "Player",
                ))
                .unwrap();
            let result_type = runtime.types().get(root_type).unwrap().fields[0].ty;
            let id = runtime
                .register_host_path_descriptor(path_registration(
                    &context,
                    root_type,
                    result_type,
                    vec![HostPathSegmentDeclaration::Field(declaration)],
                    PathAccess::ReadOnly,
                    HostSchemaEpoch::new(epoch),
                ))
                .unwrap();
            runtime.host().path_descriptor(id).unwrap().abi_fingerprint
        };
    let base = fingerprint(0, "", false, 0, HostValueType::I32);
    assert_eq!(
        base,
        fingerprint(3, "changed docs", false, 0, HostValueType::I32)
    );
    assert_ne!(base, fingerprint(0, "", true, 0, HostValueType::I32));
    assert_ne!(base, fingerprint(0, "", false, 1, HostValueType::I32));
    assert_ne!(base, fingerprint(0, "", false, 0, HostValueType::I64));
}

#[test]
fn paths_reject_types_without_portable_contracts_before_publication() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = runtime();
    let mut owner = HostTypeDeclaration::new(&context, "game.Player").unwrap();
    owner.ownership = HostTypeOwnership::HostRoot;
    owner.path_access = PathAccess::ReadOnly;
    let root_type = runtime
        .register_host_type(HostTypeRegistration::new(
            scoped(&context, &owner),
            "Player",
        ))
        .unwrap();
    let result_type = runtime
        .types()
        .register(TypeRegistration::new("Unspecified", TypeKind::Primitive))
        .unwrap();
    let result = runtime.register_host_path_descriptor(path_registration(
        &context,
        root_type,
        result_type,
        vec![HostPathSegmentDeclaration::Virtual(
            HostVirtualSegmentDeclaration {
                name: "value".into(),
                result: Ty::from_host_type(
                    &(HostValueType::Opaque(
                        context
                            .intern(&host_type_identity("game.Unregistered"))
                            .unwrap(),
                    )),
                ),
                access: PathAccess::ReadOnly,
            },
        )],
        PathAccess::ReadOnly,
        HostSchemaEpoch::new(0),
    ));
    assert_eq!(
        result.unwrap_err().kind(),
        RuntimeErrorKind::TypedPathValidation
    );
    assert_eq!(runtime.host().path_descriptors().count(), 0);
}
