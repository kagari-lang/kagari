use crate::source::program::lower_program_to_mir;
use crate::tests::bytecode::*;
use kagari_bytecode as bytecode;

#[test]
fn host_imports_are_interned_and_checked_before_execution() {
    let module = common::bytecode_ok(r#"fn main() { print("one"); print("two"); }"#);
    assert_eq!(
        module.host_interface.functions,
        vec![kagari_common::host_interface::standard_log()]
    );
    let mut absent = module.clone();
    absent.host_interface.functions.clear();
    assert!(matches!(
        verify_module(&absent),
        Err(BytecodeVerificationError::InvalidHostImport { .. })
    ));
    let mut wrong_parameter = module.clone();
    wrong_parameter.host_interface.functions[0].params[0].ty =
        kagari_common::host_interface::HostValueType::Bool;
    wrong_parameter.host_interface.functions[0].params[0].passing =
        kagari_common::host_interface::HostPassingStyle::Owned;
    assert!(matches!(
        verify_module(&wrong_parameter),
        Err(BytecodeVerificationError::TypeMismatch { .. })
    ));
    let mut wrong_arity = module.clone();
    wrong_arity.host_interface.functions[0].params.clear();
    assert!(matches!(
        verify_module(&wrong_arity),
        Err(BytecodeVerificationError::InvalidOperation { .. })
    ));
    let mut duplicate = module;
    duplicate
        .host_interface
        .functions
        .push(duplicate.host_interface.functions[0].clone());
    assert!(matches!(
        verify_module(&duplicate),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
}

#[test]
fn unsupported_dynamic_calls_fail_before_artifact_execution() {
    let module = common::bytecode_ok("fn main() {}");
    let valid = KbcArtifact::from_program(
        kagari_bytecode::BytecodeProgram {
            root: kagari_bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    for callee in [
        CallTarget::Register(Register::new(0)),
        CallTarget::RuntimeHelper(RuntimeHelper::DynamicCall),
    ] {
        let mut forged = valid.clone();
        forged.program.modules[0].functions[0].instructions.insert(
            0,
            BytecodeInstruction::Call {
                dst: None,
                callee,
                args: vec![],
            },
        );
        let decoded = KbcArtifact::from_bytes(&forged.to_bytes().unwrap()).unwrap();
        assert!(matches!(
            decoded.validate_for_loader(&ArtifactCompatibility::default()),
            Err(ArtifactValidationError::Bytecode(
                BytecodeVerificationError::InvalidOperation { .. }
            ))
        ));
    }
}

#[test]
fn public_host_trait_tables_are_rechecked_after_artifact_decode() {
    use kagari_common::host_interface::HostValueType;

    let module =
        host_trait_test_module("pub trait Readable<T> { fn get(self) -> T; } fn main() {}");
    assert!(module.trait_contracts.is_empty());
    verify_module(&module).unwrap();

    let valid = KbcArtifact::from_program(
        kagari_bytecode::BytecodeProgram {
            root: kagari_bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    for corrupt in ["argument", "return", "method"] {
        let mut forged = valid.clone();
        let host = &mut forged.program.modules[0].host_interface.types[0];
        match corrupt {
            "argument" => host.trait_implementations[0].trait_arguments = vec![HostValueType::Bool],
            "return" => {
                host.methods[0].return_type = HostValueType::Bool;
                forged.program.modules[0].host_interface.functions[0].return_type =
                    HostValueType::Bool;
            }
            "method" => host.trait_implementations[0].methods.clear(),
            _ => unreachable!(),
        }
        let decoded = KbcArtifact::from_bytes(&forged.to_bytes().unwrap()).unwrap();
        assert!(
            matches!(
                decoded.validate_for_loader(&ArtifactCompatibility::default()),
                Err(ArtifactValidationError::Bytecode(
                    BytecodeVerificationError::InvalidHostInterface(_)
                ))
            ),
            "{corrupt}"
        );
    }

    let owner = valid.program.modules[0].clone();
    let mut importer = BytecodeModule {
        identity: ModuleIdentity {
            package: PackageId("pkg".into()),
            path: vec!["consumer".into()],
        },
        dependencies: vec![bytecode::ModuleRef::new(0)],
        host_interface: owner.host_interface.clone(),
        ..Default::default()
    };
    assert!(
        bytecode::verify_program(&bytecode::BytecodeProgram {
            root: bytecode::ModuleRef::new(1),
            modules: vec![owner.clone(), importer.clone()],
        })
        .is_ok()
    );
    importer.host_interface.types[0].trait_implementations[0].trait_arguments =
        vec![HostValueType::Bool];
    assert!(matches!(
        bytecode::verify_program(&bytecode::BytecodeProgram {
            root: bytecode::ModuleRef::new(1),
            modules: vec![owner, importer],
        }),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
}

#[test]
fn private_host_trait_contracts_survive_encoding_and_reject_tampering() {
    use kagari_abi::scalar::BuiltinType;
    use kagari_abi::types::AbiType;

    let module = host_trait_test_module("trait Readable<T> { fn get(self) -> T; } fn main() {}");
    assert!(
        !module
            .public_items
            .iter()
            .any(|item| matches!(item, PublicAbiItem::Trait(_)))
    );
    assert_eq!(module.trait_contracts.len(), 1);
    verify_module(&module).unwrap();
    let valid = KbcArtifact::from_program(
        kagari_bytecode::BytecodeProgram {
            root: kagari_bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let decoded = KbcArtifact::from_bytes(&valid.to_bytes().unwrap()).unwrap();
    decoded
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();
    for corruption in ["missing", "signature"] {
        let mut forged = valid.clone();
        let module = &mut forged.program.modules[0];
        match corruption {
            "missing" => module.trait_contracts.clear(),
            "signature" => {
                module.trait_contracts[0].abi.methods[0].return_type =
                    AbiType::Builtin(BuiltinType::Bool)
            }
            _ => unreachable!(),
        }
        let decoded = KbcArtifact::from_bytes(&forged.to_bytes().unwrap()).unwrap();
        assert!(
            matches!(
                decoded.validate_for_loader(&ArtifactCompatibility::default()),
                Err(ArtifactValidationError::Bytecode(
                    BytecodeVerificationError::InvalidHostInterface(_)
                ))
            ),
            "{corruption}"
        );
    }
    let mut duplicate = valid.program.modules[0].clone();
    duplicate
        .trait_contracts
        .push(duplicate.trait_contracts[0].clone());
    assert!(matches!(
        verify_module(&duplicate),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    let mut public_collision = valid.program.modules[0].clone();
    public_collision.public_items.push(PublicAbiItem::Trait(
        public_collision.trait_contracts[0].abi.clone(),
    ));
    assert!(matches!(
        verify_module(&public_collision),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    let mut foreign_identity = valid.program.modules[0].clone();
    foreign_identity.trait_contracts[0].declaration.module = ModuleIdentity {
        package: PackageId("foreign".into()),
        path: vec!["api".into()],
    };
    assert!(matches!(
        verify_module(&foreign_identity),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
}

#[test]
fn host_trait_standard_bounds_are_rechecked_after_decode() {
    use kagari_common::host_interface::HostValueType;

    let module =
        host_trait_test_module("trait Readable<T: Eq + Hash> { fn get(self) -> T; } fn main() {}");
    verify_module(&module).unwrap();
    let valid = KbcArtifact::from_program(
        kagari_bytecode::BytecodeProgram {
            root: kagari_bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let mut forged = valid.clone();
    let host = &mut forged.program.modules[0].host_interface.types[0];
    host.trait_implementations[0].trait_arguments = vec![HostValueType::F32];
    host.methods[0].return_type = HostValueType::F32;
    forged.program.modules[0].host_interface.functions[0].return_type = HostValueType::F32;
    let decoded = KbcArtifact::from_bytes(&forged.to_bytes().unwrap()).unwrap();
    assert!(matches!(
        decoded.validate_for_loader(&ArtifactCompatibility::default()),
        Err(ArtifactValidationError::Bytecode(
            BytecodeVerificationError::InvalidHostInterface(_)
        ))
    ));
}

#[test]
fn host_trait_bounds_accept_host_implementation_evidence() {
    use kagari_common::{
        host_interface::{
            HostMethodDeclaration, HostTraitImplementationDeclaration, HostTraitMethodBinding,
            HostValueType,
        },
        identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
    };

    let mut module = host_trait_test_module(
        "trait Marker { fn mark(self) -> i32; } trait Readable<T: Marker> { fn get(self) -> T; } fn main() {}",
    );
    let host = &mut module.host_interface.types[0];
    let host_id = host.id.clone();
    let method = HostMethodDeclaration::new(&host_id, "mark", vec![], HostValueType::I32);
    host.methods.push(method.clone());
    let trait_id = DefinitionId {
        module: module.identity.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Marker".into(),
            occurrence: 0,
        }],
    };
    let mut trait_method = trait_id.clone();
    trait_method.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Method,
        name: "mark".into(),
        occurrence: 0,
    });
    host.trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id,
            vec![],
            vec![HostTraitMethodBinding {
                trait_method,
                host_method: method.id.clone(),
            }],
        ));
    host.trait_implementations[0].trait_arguments = vec![HostValueType::Opaque(host_id.clone())];
    host.methods[0].return_type = HostValueType::Opaque(host_id);
    module.host_interface.functions[0].return_type = host.methods[0].return_type.clone();
    module
        .host_interface
        .functions
        .push(host.method_contract(&method.id).unwrap());
    verify_module(&module).unwrap();
    let artifact = KbcArtifact::from_program(
        kagari_bytecode::BytecodeProgram {
            root: kagari_bytecode::ModuleRef::new(0),
            modules: vec![module.clone()],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    KbcArtifact::from_bytes(&artifact.to_bytes().unwrap())
        .unwrap()
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();
    module.host_interface.types[0].trait_implementations.pop();
    assert!(matches!(
        verify_module(&module),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
}

#[test]
fn host_trait_script_bounds_are_rechecked_after_decode() {
    use kagari_common::host_interface::HostValueType;

    let module = host_trait_test_module(
        "trait Marker { fn mark(self) -> i32; } impl Marker for i32 { fn mark(self) -> i32 { self } } trait Readable<T: Marker> { fn get(self) -> T; } fn main() {}",
    );
    verify_module(&module).unwrap();
    let valid = KbcArtifact::from_program(
        kagari_bytecode::BytecodeProgram {
            root: kagari_bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let mut forged = valid.clone();
    let host = &mut forged.program.modules[0].host_interface.types[0];
    host.trait_implementations[0].trait_arguments = vec![HostValueType::Bool];
    host.methods[0].return_type = HostValueType::Bool;
    forged.program.modules[0].host_interface.functions[0].return_type = HostValueType::Bool;
    let decoded = KbcArtifact::from_bytes(&forged.to_bytes().unwrap()).unwrap();
    assert!(matches!(
        decoded.validate_for_loader(&ArtifactCompatibility::default()),
        Err(ArtifactValidationError::Bytecode(
            BytecodeVerificationError::InvalidHostInterface(_)
        ))
    ));
}

#[test]
fn host_trait_bounds_use_imported_script_implementations() {
    use kagari_common::{
        host_interface::HostValueType,
        identity::{ModuleIdentity, PackageId},
        source_database::{SourceDatabase, SourceLayer},
    };

    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "dependency",
            "pub trait Marker { fn mark(self) -> i32; } impl Marker for i32 { fn mark(self) -> i32 { self } }",
        ),
        (
            "root",
            "use pkg::dependency::Marker; trait Readable<T: Marker> { fn get(self) -> T; } fn main() {}",
        ),
    ] {
        let uri = format!("mem://{name}");
        sources
            .bind_module(
                &uri,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        root = Some(sources.set(&uri, text.into(), SourceLayer::Base).unwrap());
    }
    let snapshot = kagari_hir::analysis::AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let checked = snapshot
        .check_program(root.unwrap(), &Default::default())
        .unwrap();
    let ir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let mut program = crate::bytecode::lower_program_to_bytecode(&ir).unwrap();
    let root_index = program.root.index();
    add_readable_host(&mut program.modules[root_index]);
    kagari_bytecode::verify_program(&program).unwrap();
    let mut forged = program.clone();
    let root = &mut forged.modules[root_index];
    root.host_interface.types[0].trait_implementations[0].trait_arguments =
        vec![HostValueType::Bool];
    root.host_interface.types[0].methods[0].return_type = HostValueType::Bool;
    root.host_interface.functions[0].return_type = HostValueType::Bool;
    assert!(matches!(
        bytecode::verify_program(&forged),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
    let artifact = KbcArtifact::from_program(program, ArtifactBuildOptions::default()).unwrap();
    KbcArtifact::from_bytes(&artifact.to_bytes().unwrap())
        .unwrap()
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();
}

#[test]
fn program_rejects_conflicting_host_types_before_linking() {
    use kagari_common::host_interface::{HostTypeDeclaration, HostTypeOwnership};

    let base = HostTypeDeclaration::new("demo.Counter");
    let program = |other: HostTypeDeclaration| kagari_bytecode::BytecodeProgram {
        root: kagari_bytecode::ModuleRef::new(1),
        modules: vec![
            BytecodeModule {
                identity: ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec!["owner".into()],
                },
                host_interface: kagari_common::host_interface::HostInterface {
                    types: vec![base.clone()],
                    ..Default::default()
                },
                ..Default::default()
            },
            BytecodeModule {
                identity: ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec!["consumer".into()],
                },
                dependencies: vec![bytecode::ModuleRef::new(0)],
                host_interface: kagari_common::host_interface::HostInterface {
                    types: vec![other],
                    ..Default::default()
                },
                ..Default::default()
            },
        ],
    };
    let mut documentation_only = base.clone();
    documentation_only.documentation = "editor help".into();
    kagari_bytecode::verify_program(&program(documentation_only)).unwrap();

    let mut conflict = base.clone();
    conflict.ownership = HostTypeOwnership::HostRoot;
    assert!(matches!(
        bytecode::verify_program(&program(conflict)),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
}
