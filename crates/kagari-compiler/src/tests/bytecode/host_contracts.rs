use crate::{
    source::{
        lower::MirLoweringError,
        program::{InstallationTraitRequirements, SourceProgramError},
    },
    tests::{bytecode::*, standard::io_module},
};
use kagari_bytecode::program::{ModuleRef, verify_program};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_contract::types::{named::NamedContract, verify};
use kagari_types::{
    declaration::{
        TypeDeclaration, TypeDef, TypeDefKind,
        host::{HostPassingStyle, HostTypeOwnership},
        native::NativeTypeConstructor,
        requirement::NativeCallableRequirement,
    },
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
};

mod fixtures;

#[test]
fn host_imports_are_interned_and_checked_before_execution() {
    let context = DefinitionContext::new().unwrap();
    let module = common::adopt_bytecode(
        &context,
        common::bytecode_ok_with_io(include_str!("../../../tests/fixtures/native-print.kgr")),
    );
    let declaration = common::adopt(&context, io_module().to_declaration());
    let provider = module
        .modules
        .iter()
        .position(|module| module.identity == declaration.identity)
        .unwrap();
    let root = &module.modules[module.root.index()];
    assert!(root.dependencies.contains(&ModuleRef::new(provider)));
    let [import] = root.native_imports.as_slice() else {
        panic!("both calls must share one native import");
    };
    let definitions = context.snapshot();
    let target = definitions.resolve(import.instance.declaration).unwrap();
    assert_eq!(target.module(), &declaration.identity);
    assert_eq!(target.last().unwrap().name, "print");
    let calls: Vec<_> = root
        .functions
        .iter()
        .flat_map(|function| &function.instructions)
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::Call {
                callee: CallTarget::Native(id),
                ..
            } => Some(id.index()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, [0, 0]);
    assert!(module.modules[provider].dependencies.iter().any(|edge| {
        module.modules[edge.index()].identity == NativeTypeConstructor::String.declaration().module
    }));
    assert_eq!(
        module.modules[provider].native_declarations,
        declaration
            .native_declarations(&context.snapshot())
            .unwrap()
    );
    verify_program(&module, &context.snapshot(), &Default::default()).unwrap();
    for mode in 0..4 {
        let mut invalid = module.clone();
        let declarations = &mut invalid.modules[provider].native_declarations;
        let print = declarations
            .iter()
            .position(|declaration| declaration.declaration == import.instance.declaration)
            .unwrap();
        match mode {
            0 => {
                declarations.remove(print);
            }
            1 => {
                let function = &mut declarations[print].function;
                function.params[0].ty = Ty::Builtin(BuiltinType::Bool);
                function.host_policy.as_mut().unwrap().passing[0] = HostPassingStyle::Owned;
            }
            2 => {
                let function = &mut declarations[print].function;
                function.params.clear();
                function.host_policy.as_mut().unwrap().passing.clear();
            }
            _ => declarations.push(declarations[print].clone()),
        }
        assert!(
            verify_program(&invalid, &context.snapshot(), &Default::default()).is_err(),
            "mode {mode}"
        );
    }
}

#[test]
fn unsupported_dynamic_calls_fail_before_artifact_execution() {
    let context = DefinitionContext::new().unwrap();
    let module = common::adopt_bytecode(&context, common::bytecode_ok("fn main() {}"));
    let valid = KbcArtifact::from_program(
        module,
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    for callee in [
        CallTarget::Slot(Slot::new(0)),
        CallTarget::RuntimeHelper(RuntimeHelper::DynamicCall),
    ] {
        let mut forged = valid.clone();
        forged.program.modules[forged.program.root.index()].functions[0]
            .instructions
            .insert(
                0,
                BytecodeInstruction::Call {
                    dst: None,
                    callee,
                    args: vec![],
                },
            );

        let decoded = KbcArtifact::from_bytes(&forged.to_bytes(&context.snapshot()).unwrap())
            .map(|metadata| common::adopt(&context, metadata))
            .unwrap();
        assert!(matches!(
            decoded.validate_for_loader(
                &ArtifactCompatibility::default(),
                &context,
                &Default::default()
            ),
            Err(ArtifactValidationError::Bytecode(
                BytecodeVerificationError::InvalidOperation { .. }
            ))
        ));
    }
}

#[test]
fn public_host_trait_tables_are_rechecked_after_artifact_decode() {
    let context = DefinitionContext::new().unwrap();

    let module = fixtures::readable_program(
        "pub trait Readable<T> { fn get(self) -> T; } fn main() {}",
        &context,
    );
    assert!(
        module.modules[module.root.index()]
            .trait_contracts
            .is_empty()
    );
    verify_program(&module, &context.snapshot(), &Default::default()).unwrap();

    let valid = KbcArtifact::from_program(
        module,
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let provider = valid
        .program
        .modules
        .iter()
        .position(|module| module.identity == fixtures::provider_owner())
        .unwrap();
    for corrupt in ["argument", "return", "method"] {
        let mut forged = valid.clone();
        let host = forged.program.modules[provider]
            .public_items
            .iter_mut()
            .find_map(|item| match item {
                PublicItem::InterfaceTable(table) => Some(table),
                _ => None,
            })
            .unwrap();
        match corrupt {
            "argument" => {
                let Ty::Trait(trait_type) = &mut host.trait_type else {
                    panic!("trait instance")
                };
                trait_type.arguments = vec![Ty::Builtin(BuiltinType::Bool)];
            }
            "return" => {
                host.methods[0].return_type = Ty::Builtin(BuiltinType::Bool);
            }
            "method" => host.methods.clear(),
            _ => unreachable!(),
        }
        let declaration = host.declaration;
        let trait_type = host.trait_type.clone();
        for record in &mut forged.program.modules[provider].named_declarations {
            if record.declaration == declaration {
                let NamedContract::Implementation(header) = &mut record.contract else {
                    panic!("implementation header")
                };
                header.trait_type = Some(trait_type.clone());
            }
        }
        let decoded = KbcArtifact::from_bytes(&forged.to_bytes(&context.snapshot()).unwrap())
            .map(|metadata| common::adopt(&context, metadata))
            .unwrap();
        assert!(
            matches!(
                decoded.validate_for_loader(
                    &ArtifactCompatibility::default(),
                    &context,
                    &Default::default()
                ),
                Err(ArtifactValidationError::Bytecode(
                    BytecodeVerificationError::InvalidInterfaceTable
                ))
            ),
            "{corrupt}"
        );
    }

    let owner = &valid.program.modules[provider];
    let mut importer = BytecodeModule {
        identity: ModuleIdentity {
            package: PackageId("pkg".into()),
            path: vec!["consumer".into()],
        },
        dependencies: vec![ModuleRef::new(provider)],
        trait_references: owner.trait_references.clone(),
        ..Default::default()
    };
    context.intern_root(&importer.identity).unwrap();
    let imported = |importer| {
        let mut program = valid.program.clone();
        program.root = ModuleRef::new(program.modules.len());
        program.exports = Default::default();
        program.modules.push(importer);
        program
    };
    verify_program(
        &imported(importer.clone()),
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    importer.trait_references[0].abi.methods[0].return_type = Ty::Builtin(BuiltinType::Bool);
    assert!(matches!(
        verify_program(
            &imported(importer),
            &context.snapshot(),
            &Default::default()
        ),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
}

#[test]
fn provider_emits_bound_callable_and_mandatory_reference_only_requirements() {
    let context = DefinitionContext::new().unwrap();
    let source = include_str!("../../../tests/fixtures/native-reference-requirements.kgr");
    let compile = |source: &str| {
        fixtures::readable_sources_with(&[("root", source)], false, &context, |provider| {
            let definitions = context.snapshot();
            let source = definitions.lookup_root(&fixtures::root_owner()).unwrap();
            let interface = context
                .intern_child(source, DefinitionKind::Trait, "CallableOnly", 0)
                .unwrap();
            let member = context
                .intern_child(interface, DefinitionKind::Method, "check", 0)
                .unwrap();
            let implementation = provider.implementation_id(0, &definitions).unwrap();
            let get = definitions
                .lookup_child(implementation, DefinitionKind::Method, "get", 0)
                .unwrap();
            provider.callable_requirements.insert(
                get,
                vec![NativeCallableRequirement {
                    receiver: Ty::Builtin(BuiltinType::I32),
                    interface: NominalTy {
                        declaration: interface,
                        arguments: vec![],
                        associated_types: Default::default(),
                    },
                    member,
                    arguments: vec![],
                }],
            );
            let bounded = context
                .intern_child(
                    definitions.lookup_root(&provider.identity).unwrap(),
                    DefinitionKind::Struct,
                    "Bounded",
                    0,
                )
                .unwrap();
            let parameter = GenericParam {
                owner: bounded,
                position: 0,
            };
            provider.types.push(TypeDef {
                name: "Bounded".into(),
                kind: TypeDefKind::Struct,
                bounds: vec![GenericBound {
                    ty: parameter.as_type(),
                    constraints: vec![NominalTy {
                        declaration: context
                            .intern_child(source, DefinitionKind::Trait, "BoundOnly", 0)
                            .unwrap(),
                        arguments: vec![],
                        associated_types: Default::default(),
                    }],
                }],
                generic_params: vec![parameter],
                fields: vec![],
                variants: vec![],
            });
            let mandatory = context
                .intern_child(source, DefinitionKind::Trait, "ReferenceOnly", 0)
                .unwrap();
            let path = context.snapshot().resolve(mandatory).unwrap().to_path();
            InstallationTraitRequirements {
                by_importer: [(provider.identity.clone(), [path].into())].into(),
            }
        })
    };
    let program = compile(source).unwrap();
    let provider = program
        .modules
        .iter()
        .find(|m| m.identity == fixtures::provider_owner())
        .unwrap();
    let names: Vec<_> = provider
        .trait_references
        .iter()
        .map(|r| r.abi.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["BoundOnly", "CallableOnly", "Readable", "ReferenceOnly"]
    );
    let pin = &provider.trait_references.last().unwrap().declaration;
    let path = context.snapshot().resolve(*pin).unwrap().to_path();
    let hidden = source.replace("pub trait ReferenceOnly", "trait ReferenceOnly");
    assert!(
        matches!(compile(&hidden), Err(SourceProgramError::Lowering {
        module, error: MirLoweringError::MissingHostDeclaration(missing),
    }) if *module == provider.identity && missing == path)
    );
}

#[test]
fn private_host_trait_contracts_survive_encoding_and_reject_tampering() {
    let context = DefinitionContext::new().unwrap();

    let module = fixtures::readable_program(
        "trait Readable<T> { fn get(self) -> T; } fn main() {}",
        &context,
    );
    assert!(
        !module.modules[module.root.index()]
            .public_items
            .iter()
            .any(|item| matches!(item, PublicItem::Trait(_)))
    );
    assert_eq!(module.modules[module.root.index()].trait_contracts.len(), 1);
    verify_program(&module, &context.snapshot(), &Default::default()).unwrap();
    let valid = KbcArtifact::from_program(
        module,
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let decoded = KbcArtifact::from_bytes(&valid.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| common::adopt(&context, metadata))
        .unwrap();
    decoded
        .validate_for_loader(
            &ArtifactCompatibility::default(),
            &context,
            &Default::default(),
        )
        .unwrap();
    for corruption in ["missing", "signature"] {
        let mut forged = valid.clone();
        let module = &mut forged.program.modules[forged.program.root.index()];
        match corruption {
            "missing" => module.trait_contracts.clear(),
            "signature" => {
                module.trait_contracts[0].abi.methods[0].return_type =
                    Ty::Builtin(BuiltinType::Bool)
            }
            _ => unreachable!(),
        }
        let decoded = KbcArtifact::from_bytes(&forged.to_bytes(&context.snapshot()).unwrap())
            .map(|metadata| common::adopt(&context, metadata))
            .unwrap();
        assert!(
            matches!(
                decoded.validate_for_loader(
                    &ArtifactCompatibility::default(),
                    &context,
                    &Default::default()
                ),
                Err(ArtifactValidationError::Bytecode(
                    BytecodeVerificationError::InvalidInterfaceTable
                ))
            ),
            "{corruption}"
        );
    }
    let mut duplicate = valid.program.clone();
    let member = &mut duplicate.modules[duplicate.root.index()];
    member
        .trait_contracts
        .push(member.trait_contracts[0].clone());
    assert!(matches!(
        verify_program(&duplicate, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    let mut public_collision = valid.program.clone();
    let member = &mut public_collision.modules[public_collision.root.index()];
    member
        .public_items
        .push(PublicItem::Trait(member.trait_contracts[0].abi.clone()));
    assert!(matches!(
        verify_program(&public_collision, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    let mut foreign_identity = valid.program.clone();
    let member = &mut foreign_identity.modules[foreign_identity.root.index()];
    common::edit_definition(
        &context,
        &mut member.trait_contracts[0].declaration,
        |path| {
            path.module = ModuleIdentity {
                package: PackageId("foreign".into()),
                path: vec!["api".into()],
            }
        },
    );
    assert!(
        verify::validate_trait_contracts(
            &member.trait_contracts,
            &member.public_items,
            &member.identity,
            &Default::default(),
            &context.snapshot(),
        )
        .is_err()
    );
    let error =
        verify_program(&foreign_identity, &context.snapshot(), &Default::default()).unwrap_err();
    // Graph preparation validates every module's own ABI before linked proofs.
    // The forged foreign owner is rejected at that structural boundary.
    assert!(
        matches!(error, BytecodeVerificationError::InvalidPublicAbi),
        "{error:?}"
    );
}

#[test]
fn host_trait_standard_bounds_are_rechecked_after_decode() {
    let context = DefinitionContext::new().unwrap();

    let module = fixtures::readable_program(
        "use std::hash::{Hash};\ntrait Readable<T: Eq + Hash> { fn get(self) -> T; } fn main() {}",
        &context,
    );
    verify_program(&module, &context.snapshot(), &Default::default()).unwrap();
    let valid = KbcArtifact::from_program(
        module,
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let mut forged = valid.clone();
    fixtures::replace_readable_result(&mut forged.program, Ty::Builtin(BuiltinType::F32));
    let decoded = KbcArtifact::from_bytes(&forged.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| common::adopt(&context, metadata))
        .unwrap();
    assert!(matches!(
        decoded.validate_for_loader(
            &ArtifactCompatibility::default(),
            &context,
            &Default::default()
        ),
        Err(ArtifactValidationError::Bytecode(
            BytecodeVerificationError::InvalidInterfaceTable
        ))
    ));
}

#[test]
fn host_trait_bounds_accept_host_implementation_evidence() {
    let context = DefinitionContext::new().unwrap();

    let mut module = fixtures::readable_sources(
        &[(
            "root",
            "trait Marker { fn mark(self) -> i32; } trait Readable<T: Marker> { fn get(self) -> T; } fn main() {}",
        )],
        true,
        &context,
    );
    verify_program(&module, &context.snapshot(), &Default::default()).unwrap();
    let artifact = KbcArtifact::from_program(
        module.clone(),
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| common::adopt(&context, metadata))
        .unwrap()
        .validate_for_loader(
            &ArtifactCompatibility::default(),
            &context,
            &Default::default(),
        )
        .unwrap();
    let provider = module
        .modules
        .iter_mut()
        .find(|module| module.identity == fixtures::provider_owner())
        .unwrap();
    provider.public_items.retain(|item| !matches!(item,
        PublicItem::InterfaceTable(table) if table.methods.iter().any(|method| method.name == "mark")
    ));
    assert!(matches!(
        verify_program(&module, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
}

#[test]
fn host_trait_script_bounds_are_rechecked_after_decode() {
    let context = DefinitionContext::new().unwrap();

    let module = fixtures::readable_program(
        "trait Marker { fn mark(self) -> i32; } impl Marker for i32 { fn mark(self) -> i32 { self } } trait Readable<T: Marker> { fn get(self) -> T; } fn main() {}",
        &context,
    );
    verify_program(&module, &context.snapshot(), &Default::default()).unwrap();
    let valid = KbcArtifact::from_program(
        module,
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let mut forged = valid.clone();
    fixtures::replace_readable_result(&mut forged.program, Ty::Builtin(BuiltinType::Bool));
    let decoded = KbcArtifact::from_bytes(&forged.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| common::adopt(&context, metadata))
        .unwrap();
    assert!(matches!(
        decoded.validate_for_loader(
            &ArtifactCompatibility::default(),
            &context,
            &Default::default()
        ),
        Err(ArtifactValidationError::Bytecode(
            BytecodeVerificationError::InvalidInterfaceTable
        ))
    ));
}

#[test]
fn host_trait_bounds_use_imported_script_implementations() {
    let context = DefinitionContext::new().unwrap();

    let program = fixtures::readable_sources(
        &[
            (
                "dependency",
                "pub trait Marker { fn mark(self) -> i32; } impl Marker for i32 { fn mark(self) -> i32 { self } }",
            ),
            (
                "root",
                "use pkg::dependency::Marker; trait Readable<T: Marker> { fn get(self) -> T; } fn main() {}",
            ),
        ],
        false,
        &context,
    );
    verify_program(&program, &context.snapshot(), &Default::default()).unwrap();
    // The root only uses the foreign Marker as Readable's type-parameter bound.
    assert!(
        program.modules[program.root.index()]
            .trait_references
            .iter()
            .any(|reference| reference.abi.name == "Marker")
    );
    let mut forged = program.clone();
    fixtures::replace_readable_result(&mut forged, Ty::Builtin(BuiltinType::Bool));
    assert!(matches!(
        verify_program(&forged, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
    let artifact = KbcArtifact::from_program(
        program,
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| common::adopt(&context, metadata))
        .unwrap()
        .validate_for_loader(
            &ArtifactCompatibility::default(),
            &context,
            &Default::default(),
        )
        .unwrap();
}

#[test]
fn program_rejects_conflicting_host_types_before_linking() {
    let context = DefinitionContext::new().unwrap();
    let mut program = fixtures::readable_program(
        "pub trait Readable<T> { fn get(self) -> T; } fn main() {}",
        &context,
    );
    let provider = program
        .modules
        .iter()
        .find(|module| module.identity == fixtures::provider_owner())
        .unwrap();
    let definition = provider
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::Type(definition) if definition.name == "Counter" => {
                Some(definition.clone())
            }
            _ => None,
        })
        .unwrap();
    let owner = context.snapshot().lookup_root(&provider.identity).unwrap();
    let declaration = context
        .snapshot()
        .lookup_child(owner, DefinitionKind::Struct, "Counter", 0)
        .unwrap();
    let root = program.root.index();
    program.modules[root].external_types.push(TypeDeclaration {
        declaration,
        definition,
    });
    verify_program(&program, &context.snapshot(), &Default::default()).unwrap();

    let TypeDefKind::Host(policy) = &mut program.modules[root]
        .external_types
        .last_mut()
        .unwrap()
        .definition
        .kind
    else {
        panic!("external schema")
    };
    policy.ownership = HostTypeOwnership::HostRoot;
    assert!(matches!(
        verify_program(&program, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
}
