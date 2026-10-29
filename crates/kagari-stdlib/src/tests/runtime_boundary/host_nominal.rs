//! Nominal host identity and path validation retain one shared declaration owner.
use super::support;

use crate::declarations::StandardDeclarations;
use kagari_bytecode::{
    artifact::KbcArtifact,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    host::{
        DynamicPathArguments, HostCallContext, HostError, HostObjectId, HostPathAdapter,
        HostPathDescriptorRegistration, HostPathSegment, HostSchemaEpoch, PreparedHostPathWrite,
    },
    metadata::{TypeId, TypeKind, TypeRegistration},
    native::{
        builder::ModuleBuilder, catalog::DeclarationCatalog, declarations::FunctionDecl,
        host::HostCallback, module::NativeModule, types::Type,
    },
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    declaration::{
        FieldDef, TypeDeclaration,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostPassingStyle,
            HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        path::{HostPathDeclaration, HostPathSegmentDeclaration, HostVirtualSegmentDeclaration},
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::native::{invoke::invoke, program::free_function_program};

fn field(name: &str, ty: Ty, access: PathAccess, visibility: Visibility) -> FieldDef {
    FieldDef {
        name: name.into(),
        ty,
        visibility,
        mutable: access == PathAccess::ReadWrite,
        host_policy: Some(HostFieldPolicy {
            readable: true,
            path_access: access,
        }),
    }
}

fn object_module(identity: &str, fields: Vec<FieldDef>) -> (NativeModule, Type) {
    let mut module = ModuleBuilder::new(identity, &Default::default()).unwrap();
    let mut player = module.define_type("Player").unwrap();
    player
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: PathAccess::ReadWrite,
        })
        .unwrap();
    for field in fields {
        player
            .field(
                DefinitionMetadata::checked(
                    DefinitionContext::new().unwrap().snapshot(),
                    field,
                    &Default::default(),
                )
                .unwrap(),
            )
            .unwrap();
    }
    let player = player.finish().unwrap().apply([]).unwrap();
    (module.finish().unwrap(), player)
}

fn schema(module: &NativeModule) -> TypeDeclaration {
    TypeDeclaration {
        declaration: module
            .declaration()
            .definition(DefinitionKind::Struct, "Player", module.definitions())
            .unwrap(),
        definition: module.declaration().types[0].clone(),
    }
}

fn root(runtime: &mut Runtime, name: &str, object: u64) -> (TypeId, Value) {
    let ty = runtime.types().get_by_name(name).unwrap().id;
    let root = runtime
        .register_host_root(HostObjectId(object), ty, HostSchemaEpoch::new(0))
        .unwrap();
    (
        ty,
        unsafe { runtime.gc().alloc_host_root_transport(root) }.unwrap(),
    )
}

fn function(name: &str, parameter: Type, passing: HostPassingStyle) -> FunctionDecl {
    FunctionDecl::new(name)
        .parameter("value", parameter)
        .host_policy(HostCallPolicy {
            passing: vec![passing],
            effects: Default::default(),
        })
}

fn program(
    module: &NativeModule,
    providers: &[&NativeModule],
) -> DefinitionMetadata<BytecodeProgram> {
    let context = DefinitionContext::new().unwrap();
    let mut records = None;
    for module in std::iter::once(module).chain(providers.iter().copied()) {
        let current = free_function_program(&module.to_declaration())
            .import_into(&context, &Default::default())
            .unwrap()
            .into_records();
        if let Some(program) = &mut records {
            let program: &mut BytecodeProgram = program;
            program.modules.extend(current.modules);
        } else {
            records = Some(current);
        }
    }
    let mut records = records.unwrap();
    let external_types: Vec<_> = records
        .modules
        .iter()
        .flat_map(|module| module.external_types.iter().cloned())
        .collect();
    records.modules[0].external_types = external_types;
    records.modules[0].dependencies = (1..records.modules.len()).map(ModuleRef::new).collect();
    DefinitionMetadata::checked(context.snapshot(), records, &Default::default()).unwrap()
}

fn path(
    module: &NativeModule,
    segments: Vec<HostPathSegmentDeclaration>,
    access: PathAccess,
    epoch: u64,
) -> DefinitionMetadata<HostPathDeclaration> {
    DefinitionMetadata::checked(
        module.definitions().clone(),
        HostPathDeclaration {
            root: schema(module).declaration,
            segments,
            access,
            schema_epoch: epoch,
        },
        &Default::default(),
    )
    .unwrap()
}

fn with_path(module: &NativeModule, path: DefinitionMetadata<HostPathDeclaration>) -> NativeModule {
    let host_result = matches!(
        path.records()
            .contract(&[schema(module)], path.definitions())
            .unwrap()
            .result,
        Ty::Host(_)
    );
    let mut builder = ModuleBuilder::from_declaration(
        module.to_declaration(),
        &Default::default(),
        BTreeMap::new(),
    )
    .unwrap();
    let path = builder.define_path(path).unwrap();
    let mut adapter = unsafe {
        // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
        HostPathAdapter::new().with_read_transport(move |cx, path| {
            Ok(if host_result {
                cx.runtime()
                    .gc()
                    .alloc_host_root_transport(path.root)
                    .unwrap()
            } else {
                Value::I32(10)
            })
        })
    };
    if path.records().access == PathAccess::ReadWrite {
        adapter = unsafe {
            // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
            adapter.with_prepare_write_transport(|_, _, _| Ok(PreparedHostPathWrite::new(|| {})))
        };
    }
    builder.bind_path(&path, adapter).unwrap();
    builder.finish().unwrap()
}

#[test]
fn declaration_conflicts_and_invalid_identities_do_not_partially_register_metadata() {
    let (original, _) = object_module("game", vec![]);
    let mut runtime = Runtime::default();
    original.install(&mut runtime).unwrap();
    let before = runtime.types().len();
    let player = runtime.types().get_by_name("game::Player").unwrap();
    let (conflict, _) = object_module(
        "game",
        vec![field(
            "hp",
            Ty::Builtin(BuiltinType::I32),
            PathAccess::ReadOnly,
            Visibility::Public,
        )],
    );
    assert_eq!(
        conflict.install(&mut runtime).unwrap_err().kind(),
        RuntimeErrorKind::MetadataConflict
    );
    assert_eq!(runtime.types().len(), before);
    assert_eq!(runtime.types().get_by_name("game::Player").unwrap(), player);
    let (definitions, mut invalid) = original.to_declaration().into_parts();
    invalid.types[0].name.clear();
    let invalid = DefinitionMetadata::checked(definitions, invalid, &Default::default()).unwrap();
    assert!(
        ModuleBuilder::from_declaration(invalid, &Default::default(), BTreeMap::new())
            .unwrap()
            .finish()
            .is_err()
    );
    assert_eq!(runtime.types().len(), before);
    let (next, _) = object_module("other", vec![]);
    next.install(&mut runtime).unwrap();
    assert_eq!(
        runtime
            .types()
            .get_by_name("other::Player")
            .unwrap()
            .id
            .index(),
        before
    );
    root(&mut runtime, "game::Player", 1);
    root(&mut runtime, "other::Player", 2);
}

#[test]
fn nested_signature_types_must_be_bound_before_program_publication() {
    let (owner, player) = object_module("game", vec![]);
    let standard = StandardDeclarations::default();
    let mut module = ModuleBuilder::new("host", &standard.catalog().unwrap())
        .unwrap()
        .with_modules(&[&owner])
        .unwrap();
    let parameter = Type::tuple([standard.option(player).unwrap()]).unwrap();
    let take = module
        .define_function(function("take", parameter, HostPassingStyle::Owned))
        .unwrap();
    module
        .bind(take, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(
                |_: &HostCallContext<'_>, _: &[Value]| -> Result<Value, HostError> {
                    panic!("linking cannot invoke callbacks")
                },
            )
        })
        .unwrap();
    let module = module.finish().unwrap();
    let mut declarations = standard.declarations().unwrap().to_vec();
    declarations.extend([
        Arc::new(owner.to_declaration()),
        Arc::new(module.to_declaration()),
    ]);
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(declarations);
    let mut sources = SourceDatabase::default();
    let file = sources.set("nested.kgr", "use game::Player; use host; pub fn call(value: (Option<Player>,)) { host::take(value); }".into(), SourceLayer::Base).unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(file, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(
        &lower_program_to_mir(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .map(|program| program.into_unverified())
    .unwrap();
    let context = DefinitionContext::new().unwrap();
    let bytecode = bytecode.import_into(&context, &Default::default()).unwrap();
    let artifact = KbcArtifact::from_program(
        bytecode.into_records(),
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let decoded =
        KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap()).unwrap();
    let decoded = decoded.import_into(&context, &Default::default()).unwrap();
    decoded
        .records()
        .validate_for_loader(&Default::default(), &context, &Default::default())
        .unwrap();
    let (definitions, decoded) = decoded.into_parts();
    let program =
        DefinitionMetadata::checked(definitions, decoded.program, &Default::default()).unwrap();
    let mut runtime = Runtime::default();
    NativeModule::install_all(&crate::modules().unwrap(), &mut runtime).unwrap();
    assert!(module.install(&mut runtime).is_err());
    assert!(runtime.load_program("nominal", program.clone()).is_err());
    assert_eq!(runtime.modules().loaded_count(), 0);
    NativeModule::install_all(&[owner, module], &mut runtime).unwrap();
    runtime.load_program("nominal", program).unwrap();
}

#[test]
fn roots_and_borrows_match_nominal_declarations_instead_of_names_or_categories() {
    let (first, a) = object_module("first", vec![]);
    let (second, _) = object_module("second", vec![]);
    let calls = Arc::new(AtomicUsize::new(0));
    let wrong = Arc::new(Mutex::new(Value::Unit));
    let providers = DeclarationCatalog::from_modules(&[&first, &second]).unwrap();
    let mut module = ModuleBuilder::new("host", &providers).unwrap();
    let take = module
        .define_function(function("take", a.clone(), HostPassingStyle::SharedBorrow))
        .unwrap();
    let observed = calls.clone();
    module
        .bind(take, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |_: &HostCallContext<'_>, _: &[Value]| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(Value::Unit)
            })
        })
        .unwrap();
    let result = module
        .define_function(
            FunctionDecl::new("wrong")
                .returns(Type::tuple([a]).unwrap())
                .host_policy(HostCallPolicy {
                    passing: vec![],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    let captured = wrong.clone();
    let observed = calls.clone();
    module
        .bind(result, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |cx: &HostCallContext<'_>, _: &[Value]| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(cx
                    .runtime()
                    .gc()
                    .alloc_tuple_transport(vec![*captured.lock().unwrap()])
                    .unwrap())
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let program = program(&module, &[&first, &second]);
    let mut runtime = Runtime::default();
    NativeModule::install_all(&[first, second, module], &mut runtime).unwrap();
    let (a_ty, a_value) = root(&mut runtime, "first::Player", 1);
    let (b_ty, b_value) = root(&mut runtime, "second::Player", 2);
    assert_ne!(a_ty, b_ty);
    *wrong.lock().unwrap() = b_value;
    let loaded = runtime.load_program("nominal", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    invoke(&runtime, &loaded, 0, &[a_value]).unwrap();
    assert!(invoke(&runtime, &loaded, 0, &[b_value]).is_err());
    let scope = unsafe {
        // SAFETY: Empty inputs; this guard owns only host borrow/session leases.
        runtime.host_scope_transport(&[])
    }
    .unwrap();
    let a_token = scope
        .borrows()
        .borrow_shared(HostObjectId(1), a_ty)
        .unwrap();
    let b_token = scope
        .borrows()
        .borrow_shared(HostObjectId(2), b_ty)
        .unwrap();
    invoke(
        &runtime,
        &loaded,
        0,
        &[unsafe { runtime.gc().alloc_host_ref_transport(a_token) }.unwrap()],
    )
    .unwrap();
    assert!(
        invoke(
            &runtime,
            &loaded,
            0,
            &[unsafe { runtime.gc().alloc_host_ref_transport(b_token) }.unwrap()]
        )
        .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    drop(scope);
    assert!(invoke(&runtime, &loaded, 1, &[]).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert!(!runtime.is_quarantined());
}

#[test]
fn identical_root_numbers_in_another_runtime_do_not_grant_access() {
    let (owner, player) = object_module("game", vec![]);
    let foreign_value = Arc::new(Mutex::new(Value::Unit));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut module = ModuleBuilder::new(
        "host",
        &DeclarationCatalog::from_modules(&[&owner]).unwrap(),
    )
    .unwrap();
    let take = module
        .define_function(function(
            "take",
            Type::tuple([player.clone()]).unwrap(),
            HostPassingStyle::Owned,
        ))
        .unwrap();
    let observed = calls.clone();
    module
        .bind(take, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |_: &HostCallContext<'_>, _: &[Value]| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(Value::Unit)
            })
        })
        .unwrap();
    let result =
        module
            .define_function(FunctionDecl::new("foreign").returns(player).host_policy(
                HostCallPolicy {
                    passing: vec![],
                    effects: Default::default(),
                },
            ))
            .unwrap();
    let captured = foreign_value.clone();
    let observed = calls.clone();
    module
        .bind(result, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |_: &HostCallContext<'_>, _: &[Value]| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(*captured.lock().unwrap())
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let program = program(&module, &[&owner]);
    let mut local = Runtime::default();
    let mut foreign = Runtime::default();
    for runtime in [&mut local, &mut foreign] {
        NativeModule::install_all(&[owner.clone(), module.clone()], runtime).unwrap();
    }
    let (_, local_value) = root(&mut local, "game::Player", 1);
    let (_, other) = root(&mut foreign, "game::Player", 1);
    *foreign_value.lock().unwrap() = other;
    let (Value::HostRoot(a), Value::HostRoot(b)) = (local_value, other) else {
        unreachable!()
    };
    let a = unsafe { local.gc().host_root_transport(a) }.unwrap();
    let b = unsafe { foreign.gc().host_root_transport(b) }.unwrap();
    assert_eq!(a.object_id(), b.object_id());
    assert_eq!(a.type_id(), b.type_id());
    assert_eq!(a.schema_epoch(), b.schema_epoch());
    assert_eq!(a.abi_fingerprint(), b.abi_fingerprint());
    assert_ne!(a, b);
    let loaded = local.load_program("owner", program).unwrap();
    let _loaded_program = loaded;
    let loaded = local.select_program(&_loaded_program).unwrap();
    invoke(
        &local,
        &loaded,
        0,
        &[unsafe { local.gc().alloc_tuple_transport(vec![local_value]) }.unwrap()],
    )
    .unwrap();
    assert!(
        invoke(
            &local,
            &loaded,
            0,
            &[unsafe { foreign.gc().alloc_tuple_transport(vec![other]) }.unwrap()]
        )
        .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(invoke(&local, &loaded, 1, &[]).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(local.gc().active_roots(), 0);
    assert!(!local.is_quarantined());
}

#[test]
fn foreign_path_views_cannot_be_chained_through_matching_local_slots() {
    let (owner, player) = object_module("game", vec![]);
    let declaration = path(
        &owner,
        vec![HostPathSegmentDeclaration::Virtual(
            HostVirtualSegmentDeclaration {
                name: "self".into(),
                result: player.abi().clone(),
                access: PathAccess::ReadOnly,
            },
        )],
        PathAccess::ReadOnly,
        0,
    );
    let module = with_path(&owner, declaration.clone());
    let mut local = Runtime::default();
    let mut foreign = Runtime::default();
    let mut views = Vec::new();
    for runtime in [&mut local, &mut foreign] {
        module.install(runtime).unwrap();
        let (_, Value::HostRoot(root_id)) = root(runtime, "game::Player", 1) else {
            unreachable!()
        };
        let descriptor = runtime.resolve_host_path(&declaration).unwrap();
        views.push(
            unsafe {
                runtime.make_host_path_view_transport(
                    runtime.gc().host_root_transport(root_id).unwrap(),
                    descriptor,
                    DynamicPathArguments::empty(),
                )
            }
            .unwrap(),
        );
    }
    assert_eq!(views[0].descriptor_id(), views[1].descriptor_id());
    assert_eq!(views[0].result_type(), views[1].result_type());
    let id = views[0].descriptor_id();
    let local_view = unsafe { local.gc().alloc_host_path_transport(views[0].clone()) }.unwrap();
    let foreign_view = unsafe { foreign.gc().alloc_host_path_transport(views[1].clone()) }.unwrap();
    assert!(
        unsafe { local.make_host_path_view_from_value_transport(&local_view, id, vec![]) }.is_ok()
    );
    assert!(
        unsafe { local.make_host_path_view_from_value_transport(&foreign_view, id, vec![]) }
            .is_err()
    );
    assert!(
        unsafe {
            // SAFETY: Both runtimes keep these wrappers live; foreign-owner admission rejects before access.
            local.host_scope_transport(&[local_view])
        }
        .is_ok()
    );
    assert!(
        unsafe {
            // SAFETY: Both runtimes keep these wrappers live; foreign-owner admission rejects before access.
            local.host_scope_transport(&[foreign_view])
        }
        .is_err()
    );
}

#[test]
fn path_fields_are_derived_from_nominal_declarations() {
    let (owner, _) = object_module(
        "game",
        vec![
            field(
                "hp",
                Ty::Builtin(BuiltinType::I32),
                PathAccess::ReadOnly,
                Visibility::Public,
            ),
            field(
                "hidden",
                Ty::Builtin(BuiltinType::I32),
                PathAccess::ReadOnly,
                Visibility::Private,
            ),
            field(
                "disabled",
                Ty::Builtin(BuiltinType::I32),
                PathAccess::None,
                Visibility::Public,
            ),
        ],
    );
    let definition = schema(&owner);
    let field_id = |name| {
        owner
            .definitions()
            .lookup_child(definition.declaration, DefinitionKind::Field, name, 0)
            .unwrap()
    };
    let declaration = path(
        &owner,
        vec![HostPathSegmentDeclaration::Field(field_id("hp"))],
        PathAccess::ReadOnly,
        0,
    );
    let contract = declaration
        .records()
        .contract(std::slice::from_ref(&definition), declaration.definitions())
        .unwrap();
    for (name, access) in [
        ("hidden", PathAccess::ReadOnly),
        ("disabled", PathAccess::ReadOnly),
        ("hp", PathAccess::ReadWrite),
    ] {
        let bad = path(
            &owner,
            vec![HostPathSegmentDeclaration::Field(field_id(name))],
            access,
            0,
        );
        assert!(
            bad.records()
                .contract(std::slice::from_ref(&definition), bad.definitions())
                .is_err()
        );
        let mut builder = ModuleBuilder::from_declaration(
            owner.to_declaration(),
            &Default::default(),
            BTreeMap::new(),
        )
        .unwrap();
        let bad = builder.define_path(bad).unwrap();
        let mut adapter = unsafe {
            // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
            HostPathAdapter::new().with_read_transport(|_, _| Ok(Value::I32(10)))
        };
        if access == PathAccess::ReadWrite {
            adapter = unsafe {
                // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
                adapter
                    .with_prepare_write_transport(|_, _, _| Ok(PreparedHostPathWrite::new(|| {})))
            };
        }
        builder.bind_path(&bad, adapter).unwrap();
        assert!(builder.finish().is_err());
    }
    let module = with_path(&owner, declaration.clone());
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    let id = runtime.resolve_host_path(&declaration).unwrap();
    let descriptor = runtime.host().path_descriptor(id).unwrap();
    assert_eq!(
        descriptor.abi_fingerprint.0,
        contract.fingerprint(declaration.definitions()).unwrap()
    );
    let metadata = runtime.types().get_by_name("game::Player").unwrap();
    assert_eq!(
        descriptor.segments,
        vec![HostPathSegment::Field {
            name: "hp".into(),
            field_id: metadata.fields[0].id,
            owner_type: metadata.id,
            result_type: metadata.fields[0].ty,
            access: PathAccess::ReadOnly,
            abi_fingerprint: metadata.fields[0].abi_fingerprint
        }]
    );
    let (other, _) = object_module(
        "other",
        vec![field(
            "hp",
            Ty::Builtin(BuiltinType::I32),
            PathAccess::ReadOnly,
            Visibility::Public,
        )],
    );
    let foreign = other
        .definitions()
        .lookup_child(schema(&other).declaration, DefinitionKind::Field, "hp", 0)
        .unwrap();
    // Low-level path execution admission still rejects foreign field ownership
    // and a physical result slot that disagrees with its declared field.
    for (definitions, field, result) in [
        (other.definitions().clone(), foreign, metadata.fields[0].ty),
        (owner.definitions().clone(), field_id("hp"), metadata.id),
    ] {
        assert_eq!(
            runtime
                .register_host_path_descriptor(HostPathDescriptorRegistration {
                    definitions,
                    root_type: metadata.id,
                    result_type: result,
                    segments: vec![HostPathSegmentDeclaration::Field(field)],
                    access: PathAccess::ReadOnly,
                    schema_epoch: HostSchemaEpoch::new(0)
                })
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::TypedPathValidation
        );
        assert_eq!(runtime.host().path_descriptors().count(), 1);
    }
}

#[test]
fn path_fingerprints_ignore_runtime_slots_and_track_contract_changes() {
    let fingerprint = |padding: usize, docs: &str, writable: bool, epoch: u64, ty: BuiltinType| {
        let (owner, _) = object_module(
            "game",
            vec![field(
                "hp",
                Ty::Builtin(ty),
                if writable {
                    PathAccess::ReadWrite
                } else {
                    PathAccess::ReadOnly
                },
                Visibility::Public,
            )],
        );
        let declaration_id = schema(&owner).declaration;
        let field = owner
            .definitions()
            .lookup_child(declaration_id, DefinitionKind::Field, "hp", 0)
            .unwrap();
        let declaration = path(
            &owner,
            vec![
                HostPathSegmentDeclaration::Field(field),
                HostPathSegmentDeclaration::Virtual(HostVirtualSegmentDeclaration {
                    name: "生命".into(),
                    result: Ty::Builtin(ty),
                    access: PathAccess::ReadOnly,
                }),
            ],
            PathAccess::ReadOnly,
            epoch,
        );
        let (definitions, mut module) = owner.to_declaration().into_parts();
        module.documentation.insert(declaration_id, docs.into());
        module.documentation.insert(field, docs.into());
        let module = DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap();
        let mut builder =
            ModuleBuilder::from_declaration(module, &Default::default(), BTreeMap::new()).unwrap();
        let declaration = builder.define_path(declaration).unwrap();
        builder
            .bind_path(&declaration, unsafe {
                // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
                HostPathAdapter::new().with_read_transport(|_, _| Ok(Value::I32(10)))
            })
            .unwrap();
        let module = builder.finish().unwrap();
        let mut runtime = Runtime::default();
        for index in 0..padding {
            runtime
                .types()
                .register(TypeRegistration::new(
                    format!("unused{index}"),
                    TypeKind::Primitive,
                ))
                .unwrap();
        }
        module.install(&mut runtime).unwrap();
        let id = runtime.resolve_host_path(&declaration).unwrap();
        (
            runtime.host().path_descriptor(id).unwrap().abi_fingerprint,
            runtime.types().get_by_name("game::Player").unwrap().id,
        )
    };
    let (base, slot) = fingerprint(0, "", false, 0, BuiltinType::I32);
    let (padded, shifted) = fingerprint(3, "changed docs", false, 0, BuiltinType::I32);
    assert_eq!(base, padded);
    assert_ne!(slot, shifted);
    assert_ne!(base, fingerprint(0, "", true, 0, BuiltinType::I32).0);
    assert_ne!(base, fingerprint(0, "", false, 1, BuiltinType::I32).0);
    assert_ne!(base, fingerprint(0, "", false, 0, BuiltinType::I64).0);
}

#[test]
fn paths_reject_types_without_portable_contracts_before_publication() {
    let (owner, _) = object_module("game", vec![]);
    let context = DefinitionContext::new().unwrap();
    let declaration = owner
        .to_declaration()
        .import_into(&context, &Default::default())
        .unwrap();
    declaration.records().collect_definitions(&context).unwrap();
    let root = context
        .intern_root(&declaration.records().identity)
        .unwrap();
    let missing: DefinitionId = context
        .intern_child(root, DefinitionKind::Struct, "Unregistered", 0)
        .unwrap();
    let mut declaration = declaration.into_records();
    let player = declaration
        .definition(DefinitionKind::Struct, "Player", &context.snapshot())
        .unwrap();
    declaration.paths.push(HostPathDeclaration {
        root: player,
        segments: vec![HostPathSegmentDeclaration::Virtual(
            HostVirtualSegmentDeclaration {
                name: "value".into(),
                result: Ty::Host(missing),
                access: PathAccess::ReadOnly,
            },
        )],
        access: PathAccess::ReadOnly,
        schema_epoch: 0,
    });
    let path = declaration.paths[0].clone();
    let metadata =
        DefinitionMetadata::checked(context.snapshot(), declaration, &Default::default()).unwrap();
    let mut builder =
        ModuleBuilder::from_declaration(metadata, &Default::default(), BTreeMap::new()).unwrap();
    builder
        .bind_path(
            &DefinitionMetadata::checked(context.snapshot(), path, &Default::default()).unwrap(),
            unsafe {
                // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
                HostPathAdapter::new().with_read_transport(|_, _| Ok(Value::Unit))
            },
        )
        .unwrap();
    assert!(builder.finish().is_err());
    let mut runtime = Runtime::default();
    owner.install(&mut runtime).unwrap();
    let root_type = runtime.types().get_by_name("game::Player").unwrap().id;
    let unspecified = runtime
        .types()
        .register(TypeRegistration::new("Unspecified", TypeKind::Primitive))
        .unwrap();
    assert_eq!(
        runtime
            .register_host_path_descriptor(HostPathDescriptorRegistration {
                definitions: context.snapshot(),
                root_type,
                result_type: unspecified,
                segments: vec![HostPathSegmentDeclaration::Virtual(
                    HostVirtualSegmentDeclaration {
                        name: "value".into(),
                        result: Ty::Host(missing),
                        access: PathAccess::ReadOnly
                    }
                )],
                access: PathAccess::ReadOnly,
                schema_epoch: HostSchemaEpoch::new(0)
            })
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );
    assert_eq!(runtime.host().path_descriptors().count(), 0);
}
