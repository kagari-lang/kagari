use kagari_bytecode::{
    artifact::{ArtifactFingerprint, KbcArtifact},
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
        metadata::DefinitionMetadata, scoped::DefinitionReferences,
    },
};
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    host::{HostFunction, HostTypeRegistration},
    metadata::TypeKind,
    value::Value,
};
use kagari_types::{
    host_interface::{
        HostInterface, HostParameter, HostPassingStyle,
        type_declaration::{
            HostFieldDeclaration, HostMethodDeclaration, HostTraitImplementationDeclaration,
            HostTraitMethodBinding, HostTypeDeclaration, HostTypeOwnership, PathAccess,
        },
        value_type::HostValueType,
    },
    language::binding,
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

fn encoded(context: &DefinitionContext, interface: &HostInterface) -> Vec<u8> {
    interface.to_bytes(&context.snapshot()).unwrap()
}

fn fingerprint(context: &DefinitionContext, interface: &HostInterface) -> ArtifactFingerprint {
    ArtifactFingerprint::of_host_interface(interface, &context.snapshot()).unwrap()
}

fn artifact(
    source: &DefinitionContext,
    program: &BytecodeProgram,
) -> DefinitionMetadata<KbcArtifact> {
    let context = DefinitionContext::new().unwrap();
    let program = scoped(source, program)
        .import_into(&context, &CancellationToken::default())
        .unwrap();
    let artifact =
        KbcArtifact::from_program(program.into_records(), Default::default(), &context).unwrap();
    DefinitionMetadata::checked(context.snapshot(), artifact, &CancellationToken::default())
        .unwrap()
}

#[test]
fn host_trait_table_requires_bound_method_callbacks_before_linking() {
    let source = DefinitionContext::new().unwrap();
    let mut owner = HostTypeDeclaration::new(&source, "demo.Counter").unwrap();
    let method =
        HostMethodDeclaration::new(&source, owner.id, "read", vec![], HostValueType::I32).unwrap();
    owner.methods.push(method.clone());
    let module = source
        .intern_root(&ModuleIdentity {
            package: PackageId("pkg".into()),
            path: vec!["api".into()],
        })
        .unwrap();
    let trait_id = source
        .intern_child(module, DefinitionKind::Trait, "Readable", 0)
        .unwrap();
    let trait_method = source
        .intern_child(trait_id, DefinitionKind::Method, "get", 0)
        .unwrap();
    owner
        .trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id,
            vec![],
            vec![HostTraitMethodBinding {
                trait_method,
                host_method: method.id,
            }],
        ));
    let offline = HostInterface {
        paths: vec![],
        types: vec![owner.clone()],
        functions: vec![],
    };
    let decoded = HostInterface::from_bytes(&encoded(&source, &offline)).unwrap();
    let mut runtime = Runtime::default();
    runtime
        .register_host_type(HostTypeRegistration::new(
            scoped(&source, &owner),
            "Counter",
        ))
        .unwrap();
    assert_eq!(
        runtime
            .host()
            .link_interface(decoded.records(), decoded.definitions())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::MetadataConflict
    );
    let binding_owner = scoped(&source, &owner);
    runtime
        .register_host_function(
            HostFunction::method(&binding_owner, method.id, |_, _| Ok(Value::I32(1))).unwrap(),
        )
        .unwrap();
    runtime
        .host()
        .link_interface(decoded.records(), decoded.definitions())
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let source_owner = scoped(&source, &owner)
        .import_into(&context, &CancellationToken::default())
        .unwrap();
    let registered = runtime
        .host()
        .interface()
        .import_into(&context, &CancellationToken::default())
        .unwrap();
    assert_eq!(&registered.records().types[0], source_owner.records());

    let mut stale = owner.clone();
    stale.trait_implementations.clear();
    let mut mismatched = Runtime::default();
    mismatched
        .register_host_type(HostTypeRegistration::new(
            scoped(&source, &stale),
            "Counter",
        ))
        .unwrap();
    let binding_owner = scoped(&source, &stale);
    mismatched
        .register_host_function(
            HostFunction::method(&binding_owner, method.id, |_, _| Ok(Value::I32(1))).unwrap(),
        )
        .unwrap();
    assert_eq!(
        mismatched
            .host()
            .link_interface(decoded.records(), decoded.definitions())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::MetadataConflict
    );
}

fn declarations(context: &DefinitionContext) -> (HostTypeDeclaration, HostTypeDeclaration) {
    let mut a = HostTypeDeclaration::new(context, "model.Player").unwrap();
    let mut b = HostTypeDeclaration::new(context, "model.Team").unwrap();
    a.ownership = HostTypeOwnership::HostRoot;
    a.path_access = PathAccess::ReadWrite;
    a.fields.push(
        HostFieldDeclaration::new(context, a.id, "team", HostValueType::Opaque(b.id)).unwrap(),
    );
    b.fields.push(
        HostFieldDeclaration::new(
            context,
            b.id,
            "members",
            HostValueType::Array(Box::new(HostValueType::Opaque(a.id))),
        )
        .unwrap(),
    );
    let mut score = HostFieldDeclaration::new(context, a.id, "score", HostValueType::I32).unwrap();
    score.writable = true;
    score.path_access = PathAccess::ReadWrite;
    a.fields.push(score);
    let mut method = HostMethodDeclaration::new(
        context,
        a.id,
        "add_score",
        vec![HostParameter {
            name: "amount".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::Result {
            declaration: context.intern(&binding::result_declaration()).unwrap(),
            ok: Box::new(HostValueType::I32),
            error: Box::new(HostValueType::String),
        },
    )
    .unwrap();
    method.receiver = HostPassingStyle::UniqueBorrow;
    method.effects.may_mutate_host_state = true;
    method.documentation = "Add score.".into();
    a.methods.push(method);
    (a, b)
}

#[test]
fn offline_members_generate_runtime_metadata_and_resolve_mutual_references() {
    let source = DefinitionContext::new().unwrap();
    let (a, b) = declarations(&source);
    let interface = HostInterface {
        paths: vec![],
        types: vec![a.clone(), b.clone()],
        functions: vec![],
    };
    let decoded = HostInterface::from_bytes(&encoded(&source, &interface)).unwrap();
    assert_eq!(
        decoded.records().types[0].methods[0].params[0].ty,
        HostValueType::I32
    );
    let mut runtime = Runtime::default();
    let ids = runtime
        .register_host_types(vec![
            HostTypeRegistration::new(scoped(&source, &b), "Team"),
            HostTypeRegistration::new(scoped(&source, &a), "Player"),
        ])
        .unwrap();
    let source_metadata = scoped(&source, &a);
    let player = runtime.types().get(ids[1]).unwrap();
    assert_eq!(player.fields[0].ty, ids[0]);
    let score = player.fields[1].ty;
    assert_eq!(runtime.types().get(score).unwrap().name, "i32");
    assert_eq!(player.methods[0].params[0].ty, score);
    assert_eq!(
        runtime
            .types()
            .get(player.methods[0].return_type)
            .unwrap()
            .kind,
        TypeKind::Enum
    );
    assert_eq!(
        player.fields[1].abi_fingerprint.0,
        source_metadata.records().fields[1]
            .fingerprint(source_metadata.definitions())
            .unwrap()
    );
    assert_eq!(
        player.methods[0].abi_fingerprint.0,
        source_metadata.records().methods[0]
            .fingerprint(source_metadata.definitions())
            .unwrap()
    );
    assert_eq!(
        runtime
            .host()
            .interface()
            .records()
            .to_bytes(&runtime.host().definitions())
            .unwrap(),
        decoded.records().to_bytes(decoded.definitions()).unwrap()
    );
    runtime
        .host()
        .link_interface(decoded.records(), decoded.definitions())
        .unwrap();
}

#[test]
fn unresolved_members_and_duplicate_batches_publish_no_metadata() {
    let source = DefinitionContext::new().unwrap();
    let (a, b) = declarations(&source);
    let mut runtime = Runtime::default();
    // The first type is valid in isolation but references an absent batch member.
    assert_eq!(
        runtime
            .register_host_type(HostTypeRegistration::new(scoped(&source, &a), "Player"))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::MetadataConflict
    );
    assert!(runtime.types().is_empty());
    assert_eq!(runtime.host().host_types().count(), 0);
    assert!(
        runtime
            .register_host_types(vec![
                HostTypeRegistration::new(scoped(&source, &a), "Player"),
                HostTypeRegistration::new(scoped(&source, &a), "Again")
            ])
            .is_err()
    );
    assert!(runtime.types().is_empty());
    let mut invalid_b = b.clone();
    invalid_b.fields[0].id = a.fields[0].id;
    assert!(
        runtime
            .register_host_types(vec![
                HostTypeRegistration::new(scoped(&source, &a), "Player"),
                HostTypeRegistration::new(scoped(&source, &invalid_b), "Team")
            ])
            .is_err()
    );
    assert!(runtime.types().is_empty());
    let ids = runtime
        .register_host_types(vec![
            HostTypeRegistration::new(scoped(&source, &a), "Player"),
            HostTypeRegistration::new(scoped(&source, &b), "Team"),
        ])
        .unwrap();
    assert_eq!(ids[0].index(), 0);
    assert_eq!(ids[1].index(), 1);
}

#[test]
fn type_contract_changes_reject_artifacts_before_publication_but_documentation_does_not() {
    let source = DefinitionContext::new().unwrap();
    let (a, b) = declarations(&source);
    let interface = HostInterface {
        paths: vec![],
        types: vec![a.clone(), b.clone()],
        functions: vec![],
    };
    let program = BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule {
            host_interface: interface.clone(),
            ..Default::default()
        }],
    };
    let original = artifact(&source, &program);
    let mut runtime = Runtime::default();
    runtime
        .register_host_types(vec![
            HostTypeRegistration::new(scoped(&source, &a), "Player"),
            HostTypeRegistration::new(scoped(&source, &b), "Team"),
        ])
        .unwrap();
    for change in 0..4 {
        let mut changed = interface.clone();
        match change {
            0 => changed.types[0].fields[1].ty = HostValueType::I64,
            1 => {
                changed.types[0].fields[1].writable = false;
                changed.types[0].fields[1].path_access = PathAccess::ReadOnly;
            }
            2 => changed.types[0].methods[0].receiver = HostPassingStyle::SharedBorrow,
            _ => changed.types[0].methods[0].effects.may_mutate_host_state = false,
        }
        assert_ne!(
            fingerprint(&source, &interface),
            fingerprint(&source, &changed)
        );
        let mut program = program.clone();
        program.modules[0].host_interface = changed;
        let changed = artifact(&source, &program);
        let decoded =
            KbcArtifact::from_bytes(&changed.records().to_bytes(changed.definitions()).unwrap())
                .unwrap();
        let context = DefinitionContext::new().unwrap();
        let decoded = decoded
            .import_into(&context, &CancellationToken::default())
            .unwrap();
        decoded
            .records()
            .validate_for_loader(&Default::default(), &context)
            .unwrap();
        let (definitions, decoded) = decoded.into_parts();
        let program = DefinitionMetadata::checked(
            definitions,
            decoded.program,
            &CancellationToken::default(),
        )
        .unwrap();
        assert!(runtime.load_program("types", program).is_err());
        assert_eq!(runtime.modules().loaded_count(), 0);
    }
    let mut documented = interface;
    documented.types[0].documentation = "Updated type docs".into();
    documented.types[0].fields[0].documentation = "Updated field docs".into();
    documented.types[0].methods[0].documentation = "Updated method docs".into();
    assert_eq!(
        fingerprint(&source, &documented),
        ArtifactFingerprint::of_host_interface(
            &original.records().program.modules[0].host_interface,
            original.definitions()
        )
        .unwrap()
    );
    let documented = scoped(&source, &documented);
    runtime
        .host()
        .link_interface(documented.records(), documented.definitions())
        .unwrap();
    let (definitions, artifact) = original.into_parts();
    runtime
        .load_program(
            "types",
            DefinitionMetadata::checked(
                definitions,
                artifact.program,
                &CancellationToken::default(),
            )
            .unwrap(),
        )
        .unwrap();
}
