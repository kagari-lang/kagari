use crate::tests::bytecode::*;
use kagari_abi::version::{KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION};
use kagari_bytecode as bytecode;

#[test]
fn const_abi_uses_evaluated_values_and_preserves_float_bits() {
    let artifact = |source: &str| {
        KbcArtifact::from_program(common::bytecode_ok(source), Default::default()).unwrap()
    };
    let expression = artifact("pub const VALUE: i32 = 6 * 7;");
    let literal = artifact("pub const VALUE: i32 = 42;");
    assert_eq!(
        expression.verification.public_abi_fingerprints,
        literal.verification.public_abi_fingerprints
    );
    let positive_zero = artifact("pub const VALUE: f32 = 0.0;");
    let negative_zero = artifact("pub const VALUE: f32 = -0.0;");
    assert_ne!(
        positive_zero.verification.public_abi_fingerprints,
        negative_zero.verification.public_abi_fingerprints
    );
    for version in 1..kagari_bytecode::KBC_ARTIFACT_FORMAT_VERSION {
        let mut old = literal.clone();
        old.header.format_version = version;
        assert!(KbcArtifact::from_bytes(&old.to_bytes().unwrap()).is_err());
        assert!(matches!(
            old.validate_for_loader(&ArtifactCompatibility {
                format_version: version,
                ..Default::default()
            }),
            Err(ArtifactValidationError::FormatVersionMismatch { .. })
        ));
    }
}

#[test]
fn builds_versioned_kbc_artifact_metadata() {
    let mut module = common::bytecode_ok(
        r#"
fn add(a: i32, b: i32) -> i32 { a + b }
fn main() -> i32 { add(1, 2) }
"#,
    );
    let identity = ModuleIdentity {
        package: PackageId("pkg".into()),
        path: vec!["main".into()],
    };
    module.modules[module.root.index()].identity = identity.clone();
    for function in &mut module.modules[module.root.index()].functions {
        function.identity.as_mut().unwrap().declaration.module = identity.clone();
    }
    for record in &mut module.modules[module.root.index()].function_table {
        record.identity.as_mut().unwrap().declaration.module = identity.clone();
    }
    let dependency_module = BytecodeModule {
        identity: ModuleIdentity {
            package: PackageId("pkg".into()),
            path: vec!["math".into()],
        },
        ..Default::default()
    };
    let dependency = DependencyFingerprint {
        module_id: dependency_module.identity.clone(),
        fingerprint: ArtifactFingerprint::of_serialized(&dependency_module),
    };
    let dependency_slot = bytecode::ModuleRef::new(module.modules.len());
    module.modules[module.root.index()]
        .dependencies
        .push(dependency_slot);
    module.modules.push(dependency_module);
    let mut dependencies: Vec<_> = module
        .modules
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != module.root.index())
        .map(|(_, member)| DependencyFingerprint {
            module_id: member.identity.clone(),
            fingerprint: ArtifactFingerprint::of_serialized(member),
        })
        .collect();
    dependencies.sort_by(|a, b| a.module_id.cmp(&b.module_id));
    assert!(dependencies.contains(&dependency));
    let artifact = KbcArtifact::from_program(
        module,
        ArtifactBuildOptions {
            security_profile: Some("dev".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(artifact.header.magic, KBC_MAGIC);
    assert_eq!(artifact.header.module_identity, identity);
    assert!(artifact.header.content_hash != ArtifactFingerprint::empty());
    assert!(
        artifact.tables.sections.iter().any(|section| {
            section.id == ArtifactSectionId::Constants && section.record_count > 0
        })
    );
    assert!(artifact.tables.sections.iter().any(|section| {
        section.id == ArtifactSectionId::Functions && section.record_count == 2
    }));
    assert!(artifact.tables.sections.iter().any(|section| {
        section.id == ArtifactSectionId::Verification && section.record_count == 2
    }));
    assert!(artifact.verification.bytecode_verified);
    assert_eq!(artifact.verification.function_layouts.len(), 2);
    assert_eq!(
        artifact.verification.loader.dependency_fingerprints,
        dependencies
    );
    assert_eq!(
        artifact.verification.loader.security_profile.as_deref(),
        Some("dev")
    );

    let requirements = ArtifactCompatibility {
        module_identity: Some(identity),
        dependency_fingerprints: Some(artifact.verification.loader.dependency_fingerprints.clone()),
        security_profile: Some("dev".to_owned()),
        ..Default::default()
    };
    assert!(artifact.validate_for_loader(&requirements).is_ok());
}

#[test]
fn serializes_kbc_artifact_bytes_for_loader_execution() {
    let module = common::bytecode_ok("fn main() -> i32 { 1 }");
    let artifact = KbcArtifact::from_program(module, ArtifactBuildOptions::default()).unwrap();

    let bytes = artifact.to_bytes().expect("artifact should encode");
    let decoded = KbcArtifact::from_bytes(&bytes).expect("artifact should decode");

    assert_eq!(decoded.header, artifact.header);
    assert_eq!(
        decoded.program.modules[decoded.program.root.index()]
            .functions
            .len(),
        artifact.program.modules[artifact.program.root.index()]
            .functions
            .len()
    );
    decoded
        .validate_for_loader(&ArtifactCompatibility::default())
        .expect("decoded artifact should validate");
}

#[test]
fn fingerprints_public_module_abi_records() {
    use kagari_abi::{
        scalar::BuiltinType,
        types::{AbiType, NominalAbiType},
    };
    let module = common::bytecode_ok(
        r#"
pub const VERSION: i32 = 1;

pub struct Player {
    val name: String,
    var score: i32,
}

pub enum Status {
    Ready,
    Waiting,
}

pub trait Display {
    fn show(self) -> String;
}

impl Display for Player {
    fn show(self) -> String {
        self.name
    }
}

pub fn greet(player: Player) -> String {
    player.name
}
"#,
    );

    let player = AbiType::Struct(NominalAbiType {
        associated_types: Default::default(),
        declaration: module.modules[module.root.index()]
            .structures
            .iter()
            .find(|layout| layout.name() == "Player")
            .unwrap()
            .declaration
            .clone(),
        arguments: vec![],
    });
    assert!(module.modules[module.root.index()].public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::Const(item)
            if item.name == "VERSION" && item.ty == AbiType::Builtin(BuiltinType::I32) && item.value == "const-v1:i32:1"
    )));
    assert!(module.modules[module.root.index()].public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::Type(item)
            if item.name == "Player"
                && item.kind == TypeAbiKind::Struct
                && item.fields.iter().any(|field| {
                    field.name == "score" && field.ty == AbiType::Builtin(BuiltinType::I32) && field.mutable
                })
    )));
    assert!(
        module.modules[module.root.index()]
            .public_items
            .iter()
            .any(|item| matches!(
                item,
                PublicAbiItem::Type(item)
                    if item.name == "Status"
                        && item.kind == TypeAbiKind::Enum
                        && item.variants.iter().any(|variant| variant.name == "Ready")
            ))
    );
    assert!(module.modules[module.root.index()].public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::Trait(item)
            if item.name == "Display"
                && item.methods.iter().any(|method| {
                    method.name == "show" && method.return_type == AbiType::Builtin(BuiltinType::String)
                })
    )));
    assert!(module.modules[module.root.index()].public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::InterfaceTable(item)
            if matches!(&item.trait_type, AbiType::Trait(ty) if ty.declaration.module == module.modules[module.root.index()].identity && ty.declaration.path.last().unwrap().name == "Display")
                && item.for_type == player
                && item.methods.iter().any(|method| method.name == "show")
    )));
    let table = module.modules[module.root.index()]
        .public_items
        .iter()
        .find(|item| matches!(item, PublicAbiItem::InterfaceTable(_)))
        .unwrap();
    let mut same_label = table.clone();
    let PublicAbiItem::InterfaceTable(other) = &mut same_label else {
        unreachable!()
    };
    other.declaration.module.package.0 = "other-package".into();
    assert_eq!(table.name(), same_label.name());
    assert_ne!(table.fingerprint_name(), same_label.fingerprint_name());
    assert!(
        module.modules[module.root.index()]
            .public_items
            .iter()
            .any(|item| matches!(
                item,
                PublicAbiItem::Function(item)
                    if item.name == "greet"
                        && item.params.len() == 1
                        && item.params[0].ty == player
                        && item.return_type == AbiType::Builtin(BuiltinType::String)
            ))
    );

    let artifact = KbcArtifact::from_program(module, ArtifactBuildOptions::default()).unwrap();
    let names = artifact
        .verification
        .public_abi_fingerprints
        .iter()
        .map(|fingerprint| fingerprint.name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"const:VERSION"));
    assert!(names.contains(&"type:Player"));
    assert!(names.contains(&"type:Status"));
    assert!(names.contains(&"trait:Display"));
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("interface_table:"))
            .count(),
        1
    );
    assert!(!names.contains(&"interface_table:Player as Display"));
    assert!(names.contains(&"function:greet"));
}

#[test]
fn abi_fingerprints_change_with_public_signatures_and_path_descriptors() {
    let first = KbcArtifact::from_program(
        common::bytecode_ok("pub fn main() -> i32 { 1 }"),
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let second = KbcArtifact::from_program(
        common::bytecode_ok("pub fn main(value: i32) -> i32 { value }"),
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let first_main = first
        .verification
        .public_abi_fingerprints
        .iter()
        .find(|fingerprint| fingerprint.name == "function:main")
        .expect("public main ABI should be fingerprinted");
    let second_main = second
        .verification
        .public_abi_fingerprints
        .iter()
        .find(|fingerprint| fingerprint.name == "function:main")
        .expect("public main ABI should be fingerprinted");
    assert_ne!(first_main.fingerprint, second_main.fingerprint);

    let path_artifact = KbcArtifact::from_program(
        kagari_bytecode::BytecodeProgram {
            root: kagari_bytecode::ModuleRef::new(0),
            modules: vec![BytecodeModule {
                types: vec![ValueType::HostHandle, ValueType::I32],
                paths: vec![PathRecord {
                    contract_fingerprint: 0,
                    id: PathId::new(0),
                    root_ty: ValueType::HostHandle,
                    result_ty: ValueType::I32,
                    read_only: false,
                    debug_name: "Actor.health".to_owned(),
                }],
                ..Default::default()
            }],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    assert_eq!(path_artifact.verification.typed_path_fingerprints.len(), 1);
    assert_ne!(
        path_artifact.verification.typed_path_fingerprints[0].fingerprint,
        ArtifactFingerprint::empty()
    );
    assert_eq!(
        path_artifact.verification.typed_path_fingerprints,
        path_artifact.verification.loader.typed_path_fingerprints
    );
    let mut renamed = path_artifact.program.clone();
    renamed.modules[0].paths[0].debug_name = "diagnostic label only".into();
    let renamed = KbcArtifact::from_program(renamed, ArtifactBuildOptions::default()).unwrap();
    assert_eq!(
        path_artifact.verification.typed_path_fingerprints,
        renamed.verification.typed_path_fingerprints
    );
    let mut changed = renamed.program;
    changed.modules[0].paths[0].contract_fingerprint = 42;
    let changed = KbcArtifact::from_program(changed, ArtifactBuildOptions::default()).unwrap();
    assert_ne!(
        path_artifact.verification.typed_path_fingerprints,
        changed.verification.typed_path_fingerprints
    );
}

#[test]
fn rejects_previous_runtime_abis_even_when_loader_requests_them() {
    for version in 5..33 {
        let previous = format!("kagari-runtime-abi-v{version}");
        let artifact = KbcArtifact::from_program(
            common::bytecode_ok("fn main() -> i32 { 1 }"),
            ArtifactBuildOptions {
                runtime_abi_version: previous.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        for runtime_abi_version in [KAGARI_RUNTIME_ABI_VERSION, previous.as_str()] {
            let requirements = ArtifactCompatibility {
                runtime_abi_version: runtime_abi_version.into(),
                ..Default::default()
            };
            assert!(matches!(
                decoded.validate_for_loader(&requirements),
                Err(ArtifactValidationError::RuntimeAbiMismatch { .. })
            ));
        }
    }
}

#[test]
fn rejects_helper_abis_without_commit_fault_or_cancellation_status() {
    for previous in [
        "kagari-runtime-helper-abi-v3",
        "kagari-runtime-helper-abi-v4",
    ] {
        let artifact = KbcArtifact::from_program(
            common::bytecode_ok("fn main() -> i32 { 1 }"),
            ArtifactBuildOptions {
                runtime_helper_abi_version: previous.into(),
                ..Default::default()
            },
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        for version in [previous, KAGARI_RUNTIME_HELPER_ABI_VERSION] {
            let requirements = ArtifactCompatibility {
                runtime_helper_abi_version: version.into(),
                ..Default::default()
            };
            assert!(matches!(
                decoded.validate_for_loader(&requirements),
                Err(ArtifactValidationError::RuntimeHelperAbiMismatch { .. })
            ));
        }
    }
}

#[test]
fn rejects_incompatible_kbc_artifact_metadata_before_loading() {
    let module = common::bytecode_ok("fn main() -> i32 { 1 }");
    let mut artifact = KbcArtifact::from_program(module, ArtifactBuildOptions::default()).unwrap();
    let requirements = ArtifactCompatibility {
        runtime_abi_version: "other-runtime".to_owned(),
        ..Default::default()
    };

    assert!(matches!(
        artifact.validate_for_loader(&requirements),
        Err(ArtifactValidationError::RuntimeAbiMismatch { .. })
    ));
    assert_eq!(
        artifact
            .validate_for_loader(&requirements)
            .unwrap_err()
            .code(),
        "KG_ARTIFACT_RUNTIME_ABI_MISMATCH"
    );

    let requirements = ArtifactCompatibility::default();
    artifact.program.modules[artifact.program.root.index()]
        .source_name
        .push_str("changed");
    assert!(matches!(
        artifact.validate_for_loader(&requirements),
        Err(ArtifactValidationError::ContentHashMismatch)
    ));
    assert_eq!(
        artifact
            .validate_for_loader(&requirements)
            .unwrap_err()
            .code(),
        "KG_ARTIFACT_CONTENT_HASH_MISMATCH"
    );

    let artifact = KbcArtifact::from_program(
        common::bytecode_ok("fn main() -> i32 { 1 }"),
        Default::default(),
    )
    .unwrap();
    let requirements = ArtifactCompatibility {
        dependency_fingerprints: Some(vec![DependencyFingerprint {
            module_id: ModuleIdentity::single_file("missing.kgr"),
            fingerprint: ArtifactFingerprint::of_str("expected dependency"),
        }]),
        ..Default::default()
    };
    assert!(matches!(
        artifact.validate_for_loader(&requirements),
        Err(ArtifactValidationError::DependencyFingerprintMismatch)
    ));
    assert_eq!(
        artifact
            .validate_for_loader(&requirements)
            .unwrap_err()
            .code(),
        "KG_ARTIFACT_DEPENDENCY_FINGERPRINT_MISMATCH"
    );
}
