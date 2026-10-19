use crate::tests::bytecode::{path_fixtures::actor_path_module, *};
use kagari_abi::version::{KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION};
use kagari_bytecode::{
    artifact::KBC_ARTIFACT_FORMAT_VERSION,
    disassembly::PreparedDisassembly,
    module::CallableTarget,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_types::{
    declaration::host::PathAccess,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::collections::BTreeSet;

#[test]
fn const_abi_uses_evaluated_values_and_preserves_float_bits() {
    let context = DefinitionContext::new().unwrap();
    let artifact = |source: &str| {
        KbcArtifact::from_program(
            common::adopt_bytecode(&context, common::bytecode_ok(source)),
            Default::default(),
            &context,
        )
        .unwrap()
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
    for version in 1..KBC_ARTIFACT_FORMAT_VERSION {
        let mut old = literal.clone();
        old.header.format_version = version;
        assert!(
            KbcArtifact::from_bytes(&old.to_bytes(&context.snapshot()).unwrap())
                .map(|metadata| common::adopt(&context, metadata))
                .is_err()
        );
        assert!(matches!(
            old.validate_for_loader(
                &ArtifactCompatibility {
                    format_version: version,
                    ..Default::default()
                },
                &context
            ),
            Err(ArtifactValidationError::FormatVersionMismatch { .. })
        ));
    }
}

#[test]
fn builds_versioned_kbc_artifact_metadata() {
    let context = DefinitionContext::new().unwrap();
    let mut module = common::adopt_bytecode(
        &context,
        common::bytecode_ok(
            r#"
fn add(a: i32, b: i32) -> i32 { a + b }
fn main() -> i32 { add(1, 2) }
"#,
        ),
    );
    let identity = ModuleIdentity {
        package: PackageId("pkg".into()),
        path: vec!["main".into()],
    };
    let previous = module.modules[module.root.index()].identity.clone();
    let namespace = module.exports.namespaces.remove(&previous).unwrap();
    module
        .exports
        .namespaces
        .insert(identity.clone(), namespace);
    common::edit_definition(&context, module.exports.entry.as_mut().unwrap(), |path| {
        path.module = identity.clone();
    });
    module.modules[module.root.index()].identity = identity.clone();
    for function in &mut module.modules[module.root.index()].functions {
        common::edit_definition(
            &context,
            &mut function.identity.as_mut().unwrap().declaration,
            |path| path.module = identity.clone(),
        );
    }
    for record in &mut module.modules[module.root.index()].function_table {
        common::edit_definition(
            &context,
            &mut record.identity.as_mut().unwrap().declaration,
            |path| path.module = identity.clone(),
        );
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
        fingerprint: ArtifactFingerprint::of_module(&dependency_module, &context.snapshot())
            .unwrap(),
    };
    let dependency_slot = ModuleRef::new(module.modules.len());
    module.modules[module.root.index()]
        .dependencies
        .push(dependency_slot);
    module.modules.push(dependency_module);
    let dependencies: Vec<_> = module
        .modules
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != module.root.index())
        .map(|(_, member)| DependencyFingerprint {
            module_id: member.identity.clone(),
            fingerprint: ArtifactFingerprint::of_module(member, &context.snapshot()).unwrap(),
        })
        .collect();
    assert!(dependencies.contains(&dependency));
    let artifact = KbcArtifact::from_program(
        module,
        ArtifactBuildOptions {
            ..Default::default()
        },
        &context,
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

    let requirements = ArtifactCompatibility {
        module_identity: Some(identity),
        dependency_fingerprints: Some(artifact.verification.loader.dependency_fingerprints.clone()),

        ..Default::default()
    };
    assert!(
        artifact
            .validate_for_loader(&requirements, &context)
            .is_ok()
    );
}

#[test]
fn serializes_kbc_artifact_bytes_for_loader_execution() {
    let context = DefinitionContext::new().unwrap();
    let module = common::adopt_bytecode(&context, common::bytecode_ok("fn main() -> i32 { 1 }"));
    let artifact =
        KbcArtifact::from_program(module, ArtifactBuildOptions::default(), &context).unwrap();

    let bytes = artifact
        .to_bytes(&context.snapshot())
        .expect("artifact should encode");
    let decoded = KbcArtifact::from_bytes(&bytes)
        .map(|metadata| common::adopt(&context, metadata))
        .expect("artifact should decode");

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
        .validate_for_loader(&ArtifactCompatibility::default(), &context)
        .expect("decoded artifact should validate");
}

#[test]
fn disassembles_compiler_generated_artifact_without_runtime() {
    // Reuse the generic interface fixture from bytecode/interfaces.rs, aggregates
    // from lower.rs, captured cells from patterns.rs and the loop from analysis.rs.
    let bytes = {
        let context = DefinitionContext::new().unwrap();
        let program = common::adopt_bytecode(
            &context,
            common::bytecode_ok(
                r#"
pub trait Get { fn get(self) -> i32; }
pub struct Holder<T> { val value: T }
impl<T> Get for Holder<T> { fn get(self) -> i32 { 42 } }
fn read(x: Get) -> i32 { x.get() }
fn dispatch() -> (i32, i32) {
    (read(Holder { value: 1 }), read(Holder { value: "a" }))
}

struct Cell<T> { var value: T }
enum Packet<T> { Data(T) }
fn get<T>(x: Cell<T>) -> T { Cell { ..x }.value }
fn aggregates() -> (i32, bool) {
    val a = Cell { value: 1 };
    val b = Cell { value: 2 };
    val c = Cell { value: true };
    val p = Packet::Data(a);
    (get(b), c.value)
}

struct Pair { val left: (i32, i32), val right: i32 }
fn make() -> Pair { Pair { left: (20, 0), right: 22 } }
fn captured() -> i32 {
    var Pair { right: second, left: (first, _) } = make();
    val next = || { first = first + 1; second = second + first; second };
    next()
}

fn main(flag: bool) -> (i32, bool) {
    val keep = (1, true);
    var n = 0;
    while n < 3 { n += 1; }
    if flag { keep } else { (2, false) }
}
"#,
            ),
        );
        KbcArtifact::from_program(program, ArtifactBuildOptions::default(), &context)
            .unwrap()
            .to_bytes(&context.snapshot())
            .unwrap()
    };
    // Compilation and its definition context are gone; only serialized KBC enters
    // the offline path. No engine, provider installation or execution is needed.
    let prepared = PreparedDisassembly::from_bytes(&bytes).unwrap();
    let render = |selector: Option<&str>| {
        // A bounded writer makes accidental callee expansion fail rather than
        // allowing an unbounded output allocation.
        let mut buffer = vec![0; 256 * 1024];
        let capacity = buffer.len();
        let mut writer = buffer.as_mut_slice();
        prepared.write_disassembly(&mut writer, selector).unwrap();
        let written = capacity - writer.len();
        buffer.truncate(written);
        String::from_utf8(buffer).unwrap()
    };
    let all = render(None);
    let program = prepared.program().program();
    assert_eq!(
        all.lines().filter(|line| line.starts_with("fn @")).count(),
        program
            .modules
            .iter()
            .map(|module| module.functions.len())
            .sum::<usize>()
    );

    let owner = program.root.index();
    let module = &program.modules[owner];
    let mut seen = BTreeSet::new();
    for (slot, function) in module.functions.iter().enumerate() {
        let selected = render(Some(&format!("@{owner}:{slot}")));
        assert_eq!(
            selected
                .lines()
                .filter(|line| line.starts_with("fn @"))
                .count(),
            1
        );
        let lines: Vec<_> = selected
            .lines()
            .filter(|line| {
                line.split_whitespace()
                    .next()
                    .is_some_and(|word| word.parse::<usize>().is_ok())
            })
            .collect();
        assert_eq!(
            lines.len(),
            function.instructions.len(),
            "{}",
            function.name
        );
        for (index, instruction) in function.instructions.iter().enumerate() {
            assert!(lines[index].starts_with(&format!("  {index:04}  ")));
            let reference = match instruction {
                BytecodeInstruction::Call { callee, .. } => {
                    let target = match callee {
                        CallTarget::Function(target) => Some((owner, *target, "function")),
                        CallTarget::ModuleFunction { module, function } => {
                            Some((module.index(), *function, "module_function"))
                        }
                        CallTarget::Shared {
                            module,
                            target: CallableTarget::Script(target),
                            ..
                        } => Some((module.index(), *target, "shared.script")),
                        _ => None,
                    };
                    if let Some((target_owner, target, kind)) = target {
                        let callee = &program.modules[target_owner].functions[target.index()];
                        if callee
                            .identity
                            .as_ref()
                            .is_some_and(|identity| !identity.arguments.is_empty())
                        {
                            seen.insert("generic call");
                        }
                        format!("target={kind} @{target_owner}:{} ", target.index())
                    } else {
                        match callee {
                            CallTarget::InterfaceMethod { module, contract } => {
                                seen.insert("trait dispatch");
                                assert!(
                                    lines[index]
                                        .contains(&format!("method_slot={}", contract.method_slot))
                                );
                                format!("target=interface_method module=@{},", module.index())
                            }
                            CallTarget::ClosureRegister { register, .. } => {
                                seen.insert("closure call");
                                format!("target=closure_register s{},", register.index())
                            }
                            _ => continue,
                        }
                    }
                }
                BytecodeInstruction::MakeClosure {
                    function, captures, ..
                } => {
                    assert!(!captures.is_empty());
                    seen.insert("captured closure");
                    format!("function=@{owner}:{} ", function.index())
                }
                BytecodeInstruction::MakeInterface {
                    module,
                    implementation,
                    ..
                } => {
                    seen.insert("interface construction");
                    format!("@{}:interface#{},", module.index(), implementation.index())
                }
                BytecodeInstruction::MakeStruct { structure, .. } => {
                    seen.insert("struct");
                    format!("@{owner}:struct#{},", structure.index())
                }
                BytecodeInstruction::MakeEnum { enumeration, .. } => {
                    seen.insert("enum");
                    format!("@{owner}:enum#{},", enumeration.index())
                }
                BytecodeInstruction::MakeTuple { .. } => {
                    seen.insert("tuple");
                    "make_tuple ".into()
                }
                BytecodeInstruction::BinaryOperands {
                    op: BinaryOp::Add, ..
                } => {
                    seen.insert("numeric add");
                    "binary_operands.add ".into()
                }
                BytecodeInstruction::BinaryOperands {
                    op: BinaryOp::Lt, ..
                } => {
                    seen.insert("numeric comparison");
                    "binary_operands.lt ".into()
                }
                BytecodeInstruction::PredicateBranch {
                    op: BinaryOp::Lt, ..
                } => {
                    seen.insert("numeric comparison");
                    "predicate_branch.lt ".into()
                }
                BytecodeInstruction::Binary {
                    op: BinaryOp::Add, ..
                } => {
                    seen.insert("numeric add");
                    "binary.add ".into()
                }
                BytecodeInstruction::Binary {
                    op: BinaryOp::Lt, ..
                } => {
                    seen.insert("numeric comparison");
                    "binary.lt ".into()
                }
                BytecodeInstruction::Jump { target } if target.index() <= index => {
                    seen.insert("loop backedge");
                    assert!(
                        selected
                            .lines()
                            .any(|line| line == format!("L{}:", target.index()))
                    );
                    format!("jump L{}", target.index())
                }
                _ => continue,
            };
            assert!(
                lines[index].contains(&reference),
                "{}: expected {reference} in {}",
                function.name,
                lines[index]
            );
        }
    }
    for expected in [
        "generic call",
        "trait dispatch",
        "closure call",
        "captured closure",
        "interface construction",
        "struct",
        "enum",
        "tuple",
        "numeric add",
        "numeric comparison",
        "loop backedge",
    ] {
        assert!(seen.contains(expected), "compiler did not emit {expected}");
    }
}

#[test]
fn fingerprints_public_module_abi_records() {
    let context = DefinitionContext::new().unwrap();

    let module = common::adopt_bytecode(
        &context,
        common::bytecode_ok(
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
        ),
    );

    let player = Ty::Struct(NominalTy {
        associated_types: Default::default(),
        declaration: module.modules[module.root.index()]
            .structures
            .iter()
            .find(|layout| layout.name(&context.snapshot()) == Some("Player"))
            .unwrap()
            .declaration,
        arguments: vec![],
    });
    assert!(module.modules[module.root.index()].public_items.iter().any(|item| matches!(
        item,
        PublicItem::Const(item)
            if item.name == "VERSION" && item.ty == Ty::Builtin(BuiltinType::I32) && item.value == "const-v1:i32:1"
    )));
    assert!(module.modules[module.root.index()].public_items.iter().any(|item| matches!(
        item,
        PublicItem::Type(item)
            if item.name == "Player"
                && item.kind == TypeDefKind::Struct
                && item.fields.iter().any(|field| {
                    field.name == "score" && field.ty == Ty::Builtin(BuiltinType::I32) && field.mutable
                })
    )));
    assert!(
        module.modules[module.root.index()]
            .public_items
            .iter()
            .any(|item| matches!(
                item,
                PublicItem::Type(item)
                    if item.name == "Status"
                        && item.kind == TypeDefKind::Enum
                        && item.variants.iter().any(|variant| variant.name == "Ready")
            ))
    );
    assert!(module.modules[module.root.index()].public_items.iter().any(|item| matches!(
        item,
        PublicItem::Trait(item)
            if item.name == "Display"
                && item.methods.iter().any(|method| {
                    method.name == "show" && method.return_type == Ty::Builtin(BuiltinType::String)
                })
    )));
    assert!(module.modules[module.root.index()].public_items.iter().any(|item| matches!(
        item,
        PublicItem::InterfaceTable(item)
            if matches!(&item.trait_type, Ty::Trait(ty) if *context.snapshot().resolve(ty.declaration).unwrap().module() == module.modules[module.root.index()].identity && context.snapshot().resolve(ty.declaration).unwrap().last().unwrap().name == "Display")
                && item.for_type == player
                && item.methods.iter().any(|method| method.name == "show")
    )));
    let table = module.modules[module.root.index()]
        .public_items
        .iter()
        .find(|item| matches!(item, PublicItem::InterfaceTable(_)))
        .unwrap();
    let mut same_label = table.clone();
    let PublicItem::InterfaceTable(other) = &mut same_label else {
        unreachable!()
    };
    common::edit_definition(&context, &mut other.declaration, |path| {
        path.module.package.0 = "other-package".into()
    });
    assert_eq!(table.name(), same_label.name());
    assert_ne!(
        table.fingerprint_name(&context.snapshot()),
        same_label.fingerprint_name(&context.snapshot())
    );
    assert!(
        module.modules[module.root.index()]
            .public_items
            .iter()
            .any(|item| matches!(
                item,
                PublicItem::Function(item)
                    if item.name == "greet"
                        && item.params.len() == 1
                        && item.params[0].ty == player
                        && item.return_type == Ty::Builtin(BuiltinType::String)
            ))
    );

    let artifact =
        KbcArtifact::from_program(module, ArtifactBuildOptions::default(), &context).unwrap();
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
    let context = DefinitionContext::new().unwrap();
    let first = KbcArtifact::from_program(
        common::adopt_bytecode(&context, common::bytecode_ok("pub fn main() -> i32 { 1 }")),
        ArtifactBuildOptions::default(),
        &context,
    )
    .unwrap();
    let second = KbcArtifact::from_program(
        common::adopt_bytecode(
            &context,
            common::bytecode_ok("pub fn main(value: i32) -> i32 { value }"),
        ),
        ArtifactBuildOptions::default(),
        &context,
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
        BytecodeProgram {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                types: vec![ValueType::HostHandle, ValueType::I32],
                ..actor_path_module(&context, "health", PathAccess::ReadWrite)
            }],
        },
        ArtifactBuildOptions::default(),
        &context,
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
    let renamed =
        KbcArtifact::from_program(renamed, ArtifactBuildOptions::default(), &context).unwrap();
    assert_eq!(
        path_artifact.verification.typed_path_fingerprints,
        renamed.verification.typed_path_fingerprints
    );
    let mut changed = renamed.program;
    let module = &mut changed.modules[0];
    module.path_declarations[0].schema_epoch += 1;
    let definitions = context.snapshot();
    module.paths[0].contract_fingerprint = module.path_declarations[0]
        .contract(&module.external_types, &definitions)
        .unwrap()
        .fingerprint(&definitions)
        .unwrap();
    let changed =
        KbcArtifact::from_program(changed, ArtifactBuildOptions::default(), &context).unwrap();
    assert_ne!(
        path_artifact.verification.typed_path_fingerprints,
        changed.verification.typed_path_fingerprints
    );
}

#[test]
fn rejects_previous_runtime_abis_even_when_loader_requests_them() {
    let context = DefinitionContext::new().unwrap();
    let program = common::adopt_bytecode(&context, common::bytecode_ok("fn main() -> i32 { 1 }"));
    for version in 5..33 {
        let previous = format!("kagari-runtime-abi-v{version}");
        let artifact = KbcArtifact::from_program(
            program.clone(),
            ArtifactBuildOptions {
                runtime_abi_version: previous.clone(),
                ..Default::default()
            },
            &context,
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
            .map(|metadata| common::adopt(&context, metadata))
            .unwrap();
        for runtime_abi_version in [KAGARI_RUNTIME_ABI_VERSION, previous.as_str()] {
            let requirements = ArtifactCompatibility {
                runtime_abi_version: runtime_abi_version.into(),
                ..Default::default()
            };
            assert!(matches!(
                decoded.validate_for_loader(&requirements, &context),
                Err(ArtifactValidationError::RuntimeAbiMismatch { .. })
            ));
        }
    }
}

#[test]
fn rejects_helper_abis_without_commit_fault_or_cancellation_status() {
    let context = DefinitionContext::new().unwrap();
    let program = common::adopt_bytecode(&context, common::bytecode_ok("fn main() -> i32 { 1 }"));
    for previous in [
        "kagari-runtime-helper-abi-v3",
        "kagari-runtime-helper-abi-v4",
    ] {
        let artifact = KbcArtifact::from_program(
            program.clone(),
            ArtifactBuildOptions {
                runtime_helper_abi_version: previous.into(),
                ..Default::default()
            },
            &context,
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
            .map(|metadata| common::adopt(&context, metadata))
            .unwrap();
        for version in [previous, KAGARI_RUNTIME_HELPER_ABI_VERSION] {
            let requirements = ArtifactCompatibility {
                runtime_helper_abi_version: version.into(),
                ..Default::default()
            };
            assert!(matches!(
                decoded.validate_for_loader(&requirements, &context),
                Err(ArtifactValidationError::RuntimeHelperAbiMismatch { .. })
            ));
        }
    }
}

#[test]
fn rejects_incompatible_kbc_artifact_metadata_before_loading() {
    let context = DefinitionContext::new().unwrap();
    let module = common::adopt_bytecode(&context, common::bytecode_ok("fn main() -> i32 { 1 }"));
    let mut artifact =
        KbcArtifact::from_program(module, ArtifactBuildOptions::default(), &context).unwrap();
    let requirements = ArtifactCompatibility {
        runtime_abi_version: "other-runtime".to_owned(),
        ..Default::default()
    };

    assert!(matches!(
        artifact.validate_for_loader(&requirements, &context),
        Err(ArtifactValidationError::RuntimeAbiMismatch { .. })
    ));
    assert_eq!(
        artifact
            .validate_for_loader(&requirements, &context)
            .unwrap_err()
            .code(),
        "KG_ARTIFACT_RUNTIME_ABI_MISMATCH"
    );

    let requirements = ArtifactCompatibility::default();
    artifact.program.modules[artifact.program.root.index()]
        .source_name
        .push_str("changed");
    assert!(matches!(
        artifact.validate_for_loader(&requirements, &context),
        Err(ArtifactValidationError::ContentHashMismatch)
    ));
    assert_eq!(
        artifact
            .validate_for_loader(&requirements, &context)
            .unwrap_err()
            .code(),
        "KG_ARTIFACT_CONTENT_HASH_MISMATCH"
    );

    let artifact = KbcArtifact::from_program(
        common::adopt_bytecode(&context, common::bytecode_ok("fn main() -> i32 { 1 }")),
        Default::default(),
        &context,
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
        artifact.validate_for_loader(&requirements, &context),
        Err(ArtifactValidationError::DependencyFingerprintMismatch)
    ));
    assert_eq!(
        artifact
            .validate_for_loader(&requirements, &context)
            .unwrap_err()
            .code(),
        "KG_ARTIFACT_DEPENDENCY_FINGERPRINT_MISMATCH"
    );
}
