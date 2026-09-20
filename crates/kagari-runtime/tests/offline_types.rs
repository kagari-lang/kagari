use kagari_common::host_interface::{
    HostFieldDeclaration, HostInterface, HostMethodDeclaration, HostParameter, HostPassingStyle,
    HostTypeDeclaration, HostTypeOwnership, HostValueType, PathAccess,
};
use kagari_ir::bytecode::{
    ArtifactFingerprint, BytecodeModule, BytecodeProgram, KbcArtifact, ModuleRef,
};
use kagari_runtime::{HostTypeRegistration, Runtime, RuntimeErrorKind, TypeKind};

fn declarations() -> (HostTypeDeclaration, HostTypeDeclaration) {
    let mut a = HostTypeDeclaration::new("model.Player");
    let mut b = HostTypeDeclaration::new("model.Team");
    a.ownership = HostTypeOwnership::HostRoot;
    a.path_access = PathAccess::ReadWrite;
    a.fields.push(HostFieldDeclaration::new(
        &a.id,
        "team",
        HostValueType::Opaque(b.id.clone()),
    ));
    b.fields.push(HostFieldDeclaration::new(
        &b.id,
        "members",
        HostValueType::Array(Box::new(HostValueType::Opaque(a.id.clone()))),
    ));
    let mut score = HostFieldDeclaration::new(&a.id, "score", HostValueType::I32);
    score.writable = true;
    score.path_access = PathAccess::ReadWrite;
    a.fields.push(score);
    let mut method = HostMethodDeclaration::new(
        &a.id,
        "add_score",
        vec![HostParameter {
            name: "amount".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::Result {
            ok: Box::new(HostValueType::I32),
            error: Box::new(HostValueType::String),
        },
    );
    method.receiver = HostPassingStyle::UniqueBorrow;
    method.effects.may_mutate_host_state = true;
    method.documentation = "Add score.".into();
    a.methods.push(method);
    (a, b)
}

#[test]
fn offline_members_generate_runtime_metadata_and_resolve_mutual_references() {
    let (a, b) = declarations();
    let interface = HostInterface {
        field_paths: vec![],
        types: vec![a.clone(), b.clone()],
        functions: vec![],
    };
    let decoded = HostInterface::from_bytes(&interface.to_bytes().unwrap()).unwrap();
    assert_eq!(decoded.types[0].methods[0].params[0].ty, HostValueType::I32);
    let mut runtime = Runtime::default();
    let ids = runtime
        .register_host_types(vec![
            HostTypeRegistration::new(b.clone(), "Team"),
            HostTypeRegistration::new(a.clone(), "Player"),
        ])
        .unwrap();
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
        a.fields[1].fingerprint().unwrap()
    );
    assert_eq!(
        player.methods[0].abi_fingerprint.0,
        a.methods[0].fingerprint().unwrap()
    );
    assert_eq!(
        runtime.host().interface().to_bytes().unwrap(),
        decoded.to_bytes().unwrap()
    );
    runtime.host().link_interface(&decoded).unwrap();
}

#[test]
fn unresolved_members_and_duplicate_batches_publish_no_metadata() {
    let (a, b) = declarations();
    let mut runtime = Runtime::default();
    // The first type is valid in isolation but references an absent batch member.
    assert_eq!(
        runtime
            .register_host_type(HostTypeRegistration::new(a.clone(), "Player"))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::MetadataConflict
    );
    assert!(runtime.types().is_empty());
    assert_eq!(runtime.host().host_types().count(), 0);
    assert!(
        runtime
            .register_host_types(vec![
                HostTypeRegistration::new(a.clone(), "Player"),
                HostTypeRegistration::new(a.clone(), "Again")
            ])
            .is_err()
    );
    assert!(runtime.types().is_empty());
    let mut invalid_b = b.clone();
    invalid_b.fields[0].id = a.fields[0].id.clone();
    assert!(
        runtime
            .register_host_types(vec![
                HostTypeRegistration::new(a.clone(), "Player"),
                HostTypeRegistration::new(invalid_b, "Team")
            ])
            .is_err()
    );
    assert!(runtime.types().is_empty());
    let ids = runtime
        .register_host_types(vec![
            HostTypeRegistration::new(a, "Player"),
            HostTypeRegistration::new(b, "Team"),
        ])
        .unwrap();
    assert_eq!(ids[0].index(), 0);
    assert_eq!(ids[1].index(), 1);
}

#[test]
fn type_contract_changes_reject_artifacts_before_publication_but_documentation_does_not() {
    let (a, b) = declarations();
    let interface = HostInterface {
        field_paths: vec![],
        types: vec![a.clone(), b.clone()],
        functions: vec![],
    };
    let program = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule {
            host_interface: interface.clone(),
            ..Default::default()
        }],
    };
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    let mut runtime = Runtime::default();
    runtime
        .register_host_types(vec![
            HostTypeRegistration::new(a, "Player"),
            HostTypeRegistration::new(b, "Team"),
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
            ArtifactFingerprint::of_host_interface(&interface),
            ArtifactFingerprint::of_host_interface(&changed)
        );
        let mut program = artifact.program.clone();
        program.modules[0].host_interface = changed;
        let changed = KbcArtifact::from_program(program, Default::default()).unwrap();
        let decoded = KbcArtifact::from_bytes(&changed.to_bytes().unwrap()).unwrap();
        decoded.validate_for_loader(&Default::default()).unwrap();
        assert!(runtime.load_program("types", decoded.program).is_err());
        assert_eq!(runtime.modules().loaded_count(), 0);
        assert_eq!(runtime.resources().counters().loaded_modules, 0);
    }
    let mut documented = interface;
    documented.types[0].documentation = "Updated type docs".into();
    documented.types[0].fields[0].documentation = "Updated field docs".into();
    documented.types[0].methods[0].documentation = "Updated method docs".into();
    assert_eq!(
        ArtifactFingerprint::of_host_interface(&documented),
        ArtifactFingerprint::of_host_interface(&artifact.program.modules[0].host_interface)
    );
    runtime.host().link_interface(&documented).unwrap();
    runtime.load_program("types", artifact.program).unwrap();
}
