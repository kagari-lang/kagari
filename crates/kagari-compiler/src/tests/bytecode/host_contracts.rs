use crate::{source::program::lower_program_to_mir, tests::bytecode::*};
use kagari_bytecode::program::{BytecodeProgram, ModuleRef, verify_program};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::catalog as foundation_catalog;
use kagari_types::{
    declaration::host::HostTypeOwnership,
    host_interface::{
        type_declaration::{
            HostMethodDeclaration, HostTraitImplementationDeclaration, HostTraitMethodBinding,
            HostTypeDeclaration,
        },
        value_type::HostValueType,
    },
    scalar::BuiltinType,
    ty::Ty,
};

#[test]
fn host_imports_are_interned_and_checked_before_execution() {
    let context = DefinitionContext::new().unwrap();
    let module = common::adopt_bytecode(
        &context,
        common::bytecode_ok(r#"fn main() { print("one"); print("two"); }"#),
    );
    let declaration = common::adopt(
        &context,
        kagari_types::declaration::logging::declaration().unwrap(),
    );
    let provider = module
        .modules
        .iter()
        .position(|module| module.identity == declaration.identity)
        .unwrap();
    assert_eq!(module.modules[module.root.index()].native_imports.len(), 1);
    assert_eq!(
        module.modules[provider].native_declarations,
        declaration
            .native_declarations(&context.snapshot())
            .unwrap()
    );
    verify_program(&module, &context.snapshot()).unwrap();
    for mode in 0..4 {
        let mut invalid = module.clone();
        let declarations = &mut invalid.modules[provider].native_declarations;
        match mode {
            0 => declarations.clear(),
            1 => {
                let function = &mut declarations[0].function;
                function.params[0].ty = Ty::Builtin(BuiltinType::Bool);
                function.host_policy.as_mut().unwrap().passing[0] =
                    kagari_types::declaration::host::HostPassingStyle::Owned;
            }
            2 => {
                declarations[0].function.params.clear();
                declarations[0]
                    .function
                    .host_policy
                    .as_mut()
                    .unwrap()
                    .passing
                    .clear();
            }
            _ => declarations.push(declarations[0].clone()),
        }
        assert!(
            verify_program(&invalid, &context.snapshot()).is_err(),
            "mode {mode}"
        );
    }
}

#[test]
fn unsupported_dynamic_calls_fail_before_artifact_execution() {
    let context = DefinitionContext::new().unwrap();
    let module = common::adopt_bytecode(&context, common::bytecode_ok("fn main() {}"));
    let valid =
        KbcArtifact::from_program(module, ArtifactBuildOptions::default(), &context).unwrap();
    for callee in [
        CallTarget::Register(Register::new(0)),
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
            decoded.validate_for_loader(&ArtifactCompatibility::default(), &context),
            Err(ArtifactValidationError::Bytecode(
                BytecodeVerificationError::InvalidOperation { .. }
            ))
        ));
    }
}

#[test]
fn public_host_trait_tables_are_rechecked_after_artifact_decode() {
    let context = DefinitionContext::new().unwrap();

    let module = host_trait_test_program(
        "pub trait Readable<T> { fn get(self) -> T; } fn main() {}",
        &context,
    );
    assert!(
        module.modules[module.root.index()]
            .trait_contracts
            .is_empty()
    );
    verify_program(&module, &context.snapshot()).unwrap();

    let valid =
        KbcArtifact::from_program(module, ArtifactBuildOptions::default(), &context).unwrap();
    for corrupt in ["argument", "return", "method"] {
        let mut forged = valid.clone();
        let host = &mut forged.program.modules[forged.program.root.index()]
            .host_interface
            .types[0];
        match corrupt {
            "argument" => host.trait_implementations[0].trait_arguments = vec![HostValueType::Bool],
            "return" => {
                host.methods[0].return_type = HostValueType::Bool;
                forged.program.modules[forged.program.root.index()]
                    .host_interface
                    .functions[0]
                    .return_type = HostValueType::Bool;
            }
            "method" => host.trait_implementations[0].methods.clear(),
            _ => unreachable!(),
        }
        let decoded = KbcArtifact::from_bytes(&forged.to_bytes(&context.snapshot()).unwrap())
            .map(|metadata| common::adopt(&context, metadata))
            .unwrap();
        assert!(
            matches!(
                decoded.validate_for_loader(&ArtifactCompatibility::default(), &context),
                Err(ArtifactValidationError::Bytecode(
                    BytecodeVerificationError::InvalidHostInterface(_)
                ))
            ),
            "{corrupt}"
        );
    }

    let owner = &valid.program.modules[valid.program.root.index()];
    let mut importer = BytecodeModule {
        identity: ModuleIdentity {
            package: PackageId("pkg".into()),
            path: vec!["consumer".into()],
        },
        dependencies: vec![valid.program.root],
        host_interface: owner.host_interface.clone(),
        ..Default::default()
    };
    context.intern_root(&importer.identity).unwrap();
    let imported = |importer| {
        let mut program = valid.program.clone();
        program.root = ModuleRef::new(program.modules.len());
        program.modules.push(importer);
        program
    };
    verify_program(&imported(importer.clone()), &context.snapshot()).unwrap();
    importer.host_interface.types[0].trait_implementations[0].trait_arguments =
        vec![HostValueType::Bool];
    assert!(matches!(
        verify_program(&imported(importer), &context.snapshot()),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
}

#[test]
fn private_host_trait_contracts_survive_encoding_and_reject_tampering() {
    let context = DefinitionContext::new().unwrap();

    let module = host_trait_test_program(
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
    verify_program(&module, &context.snapshot()).unwrap();
    let valid =
        KbcArtifact::from_program(module, ArtifactBuildOptions::default(), &context).unwrap();
    let decoded = KbcArtifact::from_bytes(&valid.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| common::adopt(&context, metadata))
        .unwrap();
    decoded
        .validate_for_loader(&ArtifactCompatibility::default(), &context)
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
                decoded.validate_for_loader(&ArtifactCompatibility::default(), &context),
                Err(ArtifactValidationError::Bytecode(
                    BytecodeVerificationError::InvalidHostInterface(_)
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
        verify_program(&duplicate, &context.snapshot()),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    let mut public_collision = valid.program.clone();
    let member = &mut public_collision.modules[public_collision.root.index()];
    member
        .public_items
        .push(PublicItem::Trait(member.trait_contracts[0].abi.clone()));
    assert!(matches!(
        verify_program(&public_collision, &context.snapshot()),
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
    assert!(matches!(
        verify_program(&foreign_identity, &context.snapshot()),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
}

#[test]
fn host_trait_standard_bounds_are_rechecked_after_decode() {
    let context = DefinitionContext::new().unwrap();

    let module = host_trait_test_program(
        "use std::hash::{Hash};\ntrait Readable<T: Eq + Hash> { fn get(self) -> T; } fn main() {}",
        &context,
    );
    verify_program(&module, &context.snapshot()).unwrap();
    let valid =
        KbcArtifact::from_program(module, ArtifactBuildOptions::default(), &context).unwrap();
    let mut forged = valid.clone();
    let host = &mut forged.program.modules[forged.program.root.index()]
        .host_interface
        .types[0];
    host.trait_implementations[0].trait_arguments = vec![HostValueType::F32];
    host.methods[0].return_type = HostValueType::F32;
    forged.program.modules[forged.program.root.index()]
        .host_interface
        .functions[0]
        .return_type = HostValueType::F32;
    let decoded = KbcArtifact::from_bytes(&forged.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| common::adopt(&context, metadata))
        .unwrap();
    assert!(matches!(
        decoded.validate_for_loader(&ArtifactCompatibility::default(), &context),
        Err(ArtifactValidationError::Bytecode(
            BytecodeVerificationError::InvalidHostInterface(_)
        ))
    ));
}

#[test]
fn host_trait_bounds_accept_host_implementation_evidence() {
    let context = DefinitionContext::new().unwrap();

    let mut module = host_trait_test_program(
        "trait Marker { fn mark(self) -> i32; } trait Readable<T: Marker> { fn get(self) -> T; } fn main() {}",
        &context,
    );
    let member = &mut module.modules[module.root.index()];
    let host = &mut member.host_interface.types[0];
    let host_id = host.id;
    let method =
        HostMethodDeclaration::new(&context, host_id, "mark", vec![], HostValueType::I32).unwrap();
    host.methods.push(method.clone());
    let root = context.intern_root(&member.identity).unwrap();
    let trait_id = context
        .intern_child(root, DefinitionKind::Trait, "Marker", 0)
        .unwrap();
    let trait_method = context
        .intern_child(trait_id, DefinitionKind::Method, "mark", 0)
        .unwrap();
    context
        .intern_child(host.id, DefinitionKind::Impl, "", 1)
        .unwrap();
    host.trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id,
            vec![],
            vec![HostTraitMethodBinding {
                trait_method,
                host_method: method.id,
            }],
        ));
    host.trait_implementations[0].trait_arguments = vec![HostValueType::Opaque(host_id)];
    host.methods[0].return_type = HostValueType::Opaque(host_id);
    member.host_interface.functions[0].return_type = host.methods[0].return_type.clone();
    member.host_interface.functions.push(
        host.method_contract(&method.id, &context.snapshot())
            .unwrap(),
    );
    verify_program(&module, &context.snapshot()).unwrap();
    let artifact =
        KbcArtifact::from_program(module.clone(), ArtifactBuildOptions::default(), &context)
            .unwrap();
    KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| common::adopt(&context, metadata))
        .unwrap()
        .validate_for_loader(&ArtifactCompatibility::default(), &context)
        .unwrap();
    module.modules[module.root.index()].host_interface.types[0]
        .trait_implementations
        .pop();
    assert!(matches!(
        verify_program(&module, &context.snapshot()),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
}

#[test]
fn host_trait_script_bounds_are_rechecked_after_decode() {
    let context = DefinitionContext::new().unwrap();

    let module = host_trait_test_program(
        "trait Marker { fn mark(self) -> i32; } impl Marker for i32 { fn mark(self) -> i32 { self } } trait Readable<T: Marker> { fn get(self) -> T; } fn main() {}",
        &context,
    );
    verify_program(&module, &context.snapshot()).unwrap();
    let valid =
        KbcArtifact::from_program(module, ArtifactBuildOptions::default(), &context).unwrap();
    let mut forged = valid.clone();
    let host = &mut forged.program.modules[forged.program.root.index()]
        .host_interface
        .types[0];
    host.trait_implementations[0].trait_arguments = vec![HostValueType::Bool];
    host.methods[0].return_type = HostValueType::Bool;
    forged.program.modules[forged.program.root.index()]
        .host_interface
        .functions[0]
        .return_type = HostValueType::Bool;
    let decoded = KbcArtifact::from_bytes(&forged.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| common::adopt(&context, metadata))
        .unwrap();
    assert!(matches!(
        decoded.validate_for_loader(&ArtifactCompatibility::default(), &context),
        Err(ArtifactValidationError::Bytecode(
            BytecodeVerificationError::InvalidHostInterface(_)
        ))
    ));
}

#[test]
fn host_trait_bounds_use_imported_script_implementations() {
    let context = DefinitionContext::new().unwrap();

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
    let snapshot = analysis_database()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot
        .check_program(root.unwrap(), &Default::default())
        .unwrap();
    let ir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let mut program = common::adopt_bytecode(
        &context,
        crate::bytecode::lower_program_to_bytecode(&ir).unwrap(),
    );
    let root_index = program.root.index();
    add_readable_host(&mut program.modules[root_index], &context);
    verify_program(&program, &context.snapshot()).unwrap();
    let mut forged = program.clone();
    let root = &mut forged.modules[root_index];
    root.host_interface.types[0].trait_implementations[0].trait_arguments =
        vec![HostValueType::Bool];
    root.host_interface.types[0].methods[0].return_type = HostValueType::Bool;
    root.host_interface.functions[0].return_type = HostValueType::Bool;
    assert!(matches!(
        verify_program(&forged, &context.snapshot()),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
    let artifact =
        KbcArtifact::from_program(program, ArtifactBuildOptions::default(), &context).unwrap();
    KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| common::adopt(&context, metadata))
        .unwrap()
        .validate_for_loader(&ArtifactCompatibility::default(), &context)
        .unwrap();
}

#[test]
fn program_rejects_conflicting_host_types_before_linking() {
    let context = DefinitionContext::new().unwrap();

    let base = HostTypeDeclaration::new(&context, "demo.Counter").unwrap();
    let program = |other: HostTypeDeclaration| BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(1),
        modules: vec![
            BytecodeModule {
                identity: ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec!["owner".into()],
                },
                host_interface: kagari_types::host_interface::HostInterface {
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
                dependencies: vec![ModuleRef::new(0)],
                host_interface: kagari_types::host_interface::HostInterface {
                    types: vec![other],
                    ..Default::default()
                },
                ..Default::default()
            },
        ],
    };
    for module in &program(base.clone()).modules {
        context.intern_root(&module.identity).unwrap();
    }
    let mut documentation_only = base.clone();
    documentation_only.documentation = "editor help".into();
    verify_program(&program(documentation_only), &context.snapshot()).unwrap();

    let mut conflict = base.clone();
    conflict.ownership = HostTypeOwnership::HostRoot;
    assert!(matches!(
        verify_program(&program(conflict), &context.snapshot()),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
}

fn analysis_database() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(foundation_catalog::shared().unwrap());
    database
}
