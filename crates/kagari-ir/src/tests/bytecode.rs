use crate::{
    bytecode::{
        ArtifactBuildOptions, ArtifactCompatibility, ArtifactFingerprint, ArtifactSectionId,
        ArtifactValidationError, BinaryOp, BytecodeFunction, BytecodeInstruction, BytecodeModule,
        BytecodeVerificationError, CallTarget, DebugMetadata, DependencyFingerprint, FieldRef,
        FunctionMetadata, FunctionRef, JumpTarget, KBC_MAGIC, KbcArtifact, LocalSlot, PathId,
        PathRecord, Register, RuntimeHelper, SafeDebugPointKind, StandardIntrinsic, StructId,
        UnaryOp, verify_module,
    },
    module::{PublicAbiItem, TypeAbiKind, ValueType},
    tests::common,
};
use kagari_common::identity::{ModuleIdentity, PackageId};

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
fn verifier_rejects_iter_get_scalar_result_and_wrong_arity() {
    let module = common::bytecode_ok(
        "fn main() -> bool { val a = [7]; std::iter::get(a, a.len()).is_none() }",
    );
    let mut scalar_result = module.clone();
    let function = &mut scalar_result.functions[0];
    let dst = function
        .instructions
        .iter()
        .find_map(|instruction| {
            if let BytecodeInstruction::Call {
                dst,
                callee: CallTarget::StandardIntrinsic(StandardIntrinsic::IterGet),
                ..
            } = instruction
            {
                *dst
            } else {
                None
            }
        })
        .unwrap();
    function.metadata.registers[dst.index()] = ValueType::I32;
    assert!(matches!(
        verify_module(&scalar_result),
        Err(BytecodeVerificationError::TypeMismatch {
            expected: ValueType::HeapObject,
            found: ValueType::I32,
            ..
        })
    ));

    let mut wrong_arity = module;
    for instruction in &mut wrong_arity.functions[0].instructions {
        if let BytecodeInstruction::Call {
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::IterGet),
            args,
            ..
        } = instruction
        {
            args.pop();
        }
    }
    assert!(matches!(
        verify_module(&wrong_arity),
        Err(
            BytecodeVerificationError::StandardIntrinsicSignatureMismatch {
                intrinsic: StandardIntrinsic::IterGet,
                ..
            }
        )
    ));
}

#[test]
fn const_abi_uses_evaluated_values_and_preserves_float_bits() {
    let artifact = |source: &str| {
        KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![common::bytecode_ok(source)],
            },
            Default::default(),
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
    for version in 1..crate::bytecode::KBC_ARTIFACT_FORMAT_VERSION {
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
fn lowers_function_metadata_into_bytecode() {
    let bytecode = common::bytecode_ok("fn add(a: i32, b: i32) -> i32 { val c = a + b; c }");
    let function = &bytecode.functions[0];

    assert_eq!(function.id, FunctionRef::new(0));
    assert_eq!(function.name, "add");
    assert_eq!(function.parameter_count, 2);
    assert_eq!(function.local_count, 3);
    assert!(function.register_count >= 4);
    assert_eq!(
        function.metadata.params,
        vec![ValueType::I32, ValueType::I32]
    );
    assert_eq!(function.metadata.return_type, ValueType::I32);
    assert_eq!(
        function.metadata.locals[..3],
        [ValueType::I32, ValueType::I32, ValueType::I32]
    );
    assert_eq!(
        function.metadata.registers.len(),
        usize::from(function.register_count)
    );
}

#[test]
fn lowers_debugger_metadata_into_bytecode() {
    let bytecode = common::bytecode_ok(
        r#"
fn main(value: i32) -> i32 {
    val next = value + 1;
    print("debug");
    next
}
"#,
    );
    let function = &bytecode.functions[0];
    let debug = &function.metadata.debug;

    assert_eq!(debug.source_spans.len(), function.instructions.len());
    assert_eq!(debug.line_table.len(), function.instructions.len());
    assert_eq!(debug.frame_layout.locals, function.metadata.locals);
    assert_eq!(debug.frame_layout.registers, function.metadata.registers);
    assert!(
        debug
            .safe_debug_points
            .iter()
            .any(|point| point.kind == SafeDebugPointKind::FunctionEntry)
    );
    assert!(
        debug
            .safe_debug_points
            .iter()
            .any(|point| point.kind == SafeDebugPointKind::CallBoundary)
    );
    assert!(
        debug
            .safe_debug_points
            .iter()
            .any(|point| point.kind == SafeDebugPointKind::FunctionReturn)
    );
    assert!(
        debug
            .local_live_ranges
            .iter()
            .any(|range| range.name == "value" && range.is_parameter)
    );
    assert!(
        debug
            .local_live_ranges
            .iter()
            .any(|range| range.name == "next" && !range.is_parameter)
    );

    let artifact_debug = DebugMetadata::from_module(&bytecode);
    assert!(!artifact_debug.stripped);
    assert_eq!(artifact_debug.functions.len(), bytecode.functions.len());
    assert!(artifact_debug.debug_names.iter().any(|name| name == "main"));
}

#[test]
fn populates_bytecode_tables_and_effect_metadata() {
    let bytecode = common::bytecode_ok(
        r#"
fn add(a: i32, b: i32) -> i32 { a + b }

fn main() -> i32 {
    print("ok");
    add(1, 2)
}
"#,
    );
    let main = bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");

    assert!(bytecode.constants.iter().any(|constant| matches!(
        constant,
        crate::bytecode::ConstantOperand::Str(text) if text == "ok"
    )));
    assert!(bytecode.types.contains(&ValueType::I32));
    assert!(bytecode.types.contains(&ValueType::Str));
    assert_eq!(bytecode.function_table.len(), bytecode.functions.len());
    assert_eq!(bytecode.function_table[0].name, "add");
    assert_eq!(
        bytecode.function_table[0].params,
        vec![ValueType::I32, ValueType::I32]
    );
    assert_eq!(bytecode.function_table[0].return_type, ValueType::I32);
    assert!(main.metadata.effects.calls);
    assert!(main.metadata.effects.touches_runtime);
    assert!(verify_module(&bytecode).is_ok());
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
    module.identity = identity.clone();
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
    module.dependencies = vec![crate::bytecode::ModuleRef::new(0)];
    let artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(1),
            modules: vec![dependency_module, module],
        },
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
        vec![dependency]
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
    let artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();

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
    use crate::module::abi::{AbiType, NominalAbiType};
    use kagari_hir::types::BuiltinType;
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
        declaration: module
            .structures
            .iter()
            .find(|layout| layout.name() == "Player")
            .unwrap()
            .declaration
            .clone(),
        arguments: vec![],
    });
    assert!(module.public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::Const(item)
            if item.name == "VERSION" && item.ty == AbiType::Builtin(BuiltinType::I32) && item.value == "const-v1:i32:1"
    )));
    assert!(module.public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::Type(item)
            if item.name == "Player"
                && item.kind == TypeAbiKind::Struct
                && item.fields.iter().any(|field| {
                    field.name == "score" && field.ty == AbiType::Builtin(BuiltinType::I32) && field.mutable
                })
    )));
    assert!(module.public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::Type(item)
            if item.name == "Status"
                && item.kind == TypeAbiKind::Enum
                && item.variants.iter().any(|variant| variant.name == "Ready")
    )));
    assert!(module.public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::Trait(item)
            if item.name == "Display"
                && item.methods.iter().any(|method| {
                    method.name == "show" && method.return_type == AbiType::Builtin(BuiltinType::String)
                })
    )));
    assert!(module.public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::InterfaceTable(item)
            if matches!(&item.trait_type, AbiType::Trait(ty) if ty.declaration.module == module.identity && ty.declaration.path.last().unwrap().name == "Display")
                && item.for_type == player
                && item.methods.iter().any(|method| method.name == "show")
    )));
    assert!(module.public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::Function(item)
            if item.name == "greet"
                && item.params.len() == 1
                && item.params[0].ty == player
                && item.return_type == AbiType::Builtin(BuiltinType::String)
    )));

    let artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
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
    assert!(names.contains(&"interface_table:Player as Display"));
    assert!(names.contains(&"function:greet"));
}

#[test]
fn abi_fingerprints_change_with_public_signatures_and_path_descriptors() {
    let first = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![common::bytecode_ok("pub fn main() -> i32 { 1 }")],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let second = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![common::bytecode_ok(
                "pub fn main(value: i32) -> i32 { value }",
            )],
        },
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
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![BytecodeModule {
                types: vec![ValueType::HostHandle, ValueType::I32],
                paths: vec![PathRecord {
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
}

#[test]
fn rejects_previous_runtime_abis_even_when_loader_requests_them() {
    for previous in [
        "kagari-runtime-abi-v5",
        "kagari-runtime-abi-v6",
        "kagari-runtime-abi-v7",
        "kagari-runtime-abi-v8",
        "kagari-runtime-abi-v9",
        "kagari-runtime-abi-v10",
        "kagari-runtime-abi-v11",
        "kagari-runtime-abi-v12",
        "kagari-runtime-abi-v13",
        "kagari-runtime-abi-v14",
        "kagari-runtime-abi-v15",
        "kagari-runtime-abi-v16",
        "kagari-runtime-abi-v17",
        "kagari-runtime-abi-v18",
        "kagari-runtime-abi-v19",
    ] {
        let artifact = KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![common::bytecode_ok("fn main() -> i32 { 1 }")],
            },
            ArtifactBuildOptions {
                runtime_abi_version: previous.into(),
                ..Default::default()
            },
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        for runtime_abi_version in [crate::bytecode::KAGARI_RUNTIME_ABI_VERSION, previous] {
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
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![common::bytecode_ok("fn main() -> i32 { 1 }")],
            },
            ArtifactBuildOptions {
                runtime_helper_abi_version: previous.into(),
                ..Default::default()
            },
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        for version in [previous, crate::bytecode::KAGARI_RUNTIME_HELPER_ABI_VERSION] {
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
    let mut artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
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
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![common::bytecode_ok("fn main() -> i32 { 1 }")],
        },
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

#[test]
fn verifier_rejects_malformed_register_local_and_control_flow_bytecode() {
    let mut invalid_register = common::bytecode_ok("fn main() -> i32 { 1 }");
    invalid_register.functions[0].instructions[0] = BytecodeInstruction::LoadConst {
        dst: Register::new(999),
        constant: crate::bytecode::ConstantOperand::I32(1),
    };
    assert!(matches!(
        verify_module(&invalid_register),
        Err(BytecodeVerificationError::InvalidRegister { .. })
    ));
    assert_eq!(
        verify_module(&invalid_register).unwrap_err().code(),
        "KG_BYTECODE_INVALID_REGISTER"
    );

    let mut invalid_local = common::bytecode_ok("fn main() -> i32 { val value = 1; value }");
    invalid_local.functions[0].instructions[1] = BytecodeInstruction::StoreLocal {
        local: LocalSlot::new(999),
        src: Register::new(0),
    };
    assert!(matches!(
        verify_module(&invalid_local),
        Err(BytecodeVerificationError::InvalidLocal { .. })
    ));
    assert_eq!(
        verify_module(&invalid_local).unwrap_err().code(),
        "KG_BYTECODE_INVALID_LOCAL"
    );

    let mut invalid_jump = common::bytecode_ok("fn main() -> i32 { if true { 1 } else { 2 } }");
    invalid_jump.functions[0]
        .metadata
        .control_flow_targets
        .push(JumpTarget::new(usize::MAX));
    assert!(matches!(
        verify_module(&invalid_jump),
        Err(BytecodeVerificationError::InvalidJumpTarget { .. })
    ));
    assert_eq!(
        verify_module(&invalid_jump).unwrap_err().code(),
        "KG_BYTECODE_INVALID_JUMP_TARGET"
    );
}

#[test]
fn verifier_rejects_type_inconsistent_bytecode() {
    let mut bytecode = common::bytecode_ok("fn main() -> i32 { 1 }");
    bytecode.functions[0].metadata.return_type = ValueType::Bool;
    bytecode.function_table[0].return_type = ValueType::Bool;
    bytecode.types.push(ValueType::Bool);

    assert!(matches!(
        verify_module(&bytecode),
        Err(BytecodeVerificationError::TypeMismatch {
            context: "return value",
            expected: ValueType::Bool,
            found: ValueType::I32,
            ..
        })
    ));
}

#[test]
fn stdlib_verifier_rejects_invalid_standard_intrinsic_signatures() {
    let mut bytecode = common::bytecode_ok(
        r#"
fn main(value: String) -> usize {
    value.len_chars()
}
"#,
    );
    let call = bytecode.functions[0]
        .instructions
        .iter_mut()
        .find_map(|instruction| {
            let BytecodeInstruction::Call { callee, .. } = instruction else {
                return None;
            };
            Some(callee)
        })
        .expect("expected standard intrinsic call");
    *call = CallTarget::StandardIntrinsic(StandardIntrinsic::MathSqrt);

    assert!(matches!(
        verify_module(&bytecode),
        Err(BytecodeVerificationError::TypeMismatch {
            context: "standard intrinsic argument",
            expected: ValueType::F64,
            found: ValueType::Str,
            ..
        })
    ));

    let artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![bytecode],
        },
        ArtifactBuildOptions::default(),
    );
    assert!(matches!(
        artifact,
        Err(ArtifactValidationError::Bytecode(
            BytecodeVerificationError::TypeMismatch {
                context: "standard intrinsic argument",
                ..
            }
        ))
    ));
}

#[test]
fn verifier_rejects_invalid_aggregate_writes() {
    let module = BytecodeModule {
        types: vec![
            ValueType::Unit,
            ValueType::Bool,
            ValueType::I32,
            ValueType::HeapObject,
        ],
        structures: common::bytecode_ok("struct Point { var x: i32 }").structures,
        function_table: vec![crate::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            name: "write_bad_field".to_owned(),
            params: Vec::new(),
            return_type: ValueType::Unit,
            effects: crate::module::EffectSet::aggregate_write(),
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            name: "write_bad_field".to_owned(),
            parameter_count: 0,
            local_count: 0,
            register_count: 2,
            metadata: FunctionMetadata {
                return_type: ValueType::Unit,
                registers: vec![ValueType::HeapObject, ValueType::Bool],
                effects: crate::module::EffectSet::aggregate_write(),
                ..Default::default()
            },
            instructions: vec![
                BytecodeInstruction::WriteAggregateField {
                    base: Register::new(0),
                    field: FieldRef {
                        structure: StructId::new(0),
                        slot: 0,
                    },
                    value: Register::new(1),
                },
                BytecodeInstruction::Return(None),
            ],
        }],
        ..Default::default()
    };

    assert!(matches!(
        verify_module(&module),
        Err(BytecodeVerificationError::TypeMismatch {
            context: "aggregate field value",
            expected: ValueType::I32,
            found: ValueType::Bool,
            ..
        })
    ));
}

#[test]
fn verifier_rejects_unresolved_and_read_only_typed_paths() {
    let unresolved_path = BytecodeModule {
        types: vec![ValueType::HostHandle, ValueType::I32],
        paths: vec![PathRecord {
            id: PathId::new(0),
            root_ty: ValueType::HostHandle,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "Actor.health".to_owned(),
        }],
        function_table: vec![crate::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            name: "read_missing_path".to_owned(),
            params: vec![ValueType::HostHandle],
            return_type: ValueType::I32,
            effects: crate::module::EffectSet::path_read(),
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            name: "read_missing_path".to_owned(),
            parameter_count: 1,
            local_count: 1,
            register_count: 2,
            metadata: FunctionMetadata {
                params: vec![ValueType::HostHandle],
                return_type: ValueType::I32,
                locals: vec![ValueType::HostHandle],
                registers: vec![ValueType::HostHandle, ValueType::I32],
                effects: crate::module::EffectSet::path_read(),
                ..Default::default()
            },
            instructions: vec![
                BytecodeInstruction::ReadPath {
                    dst: Register::new(1),
                    root_or_view: Register::new(0),
                    path: PathId::new(99),
                    dynamic_args: Vec::new(),
                },
                BytecodeInstruction::Return(Some(Register::new(1))),
            ],
        }],
        ..Default::default()
    };
    assert!(matches!(
        verify_module(&unresolved_path),
        Err(BytecodeVerificationError::InvalidPathId { .. })
    ));

    let read_only_path = BytecodeModule {
        types: vec![ValueType::Unit, ValueType::HostHandle, ValueType::I32],
        paths: vec![PathRecord {
            id: PathId::new(0),
            root_ty: ValueType::HostHandle,
            result_ty: ValueType::I32,
            read_only: true,
            debug_name: "Actor.id".to_owned(),
        }],
        function_table: vec![crate::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            name: "write_readonly_path".to_owned(),
            params: vec![ValueType::HostHandle, ValueType::I32],
            return_type: ValueType::Unit,
            effects: crate::module::EffectSet::path_write(),
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            name: "write_readonly_path".to_owned(),
            parameter_count: 2,
            local_count: 2,
            register_count: 2,
            metadata: FunctionMetadata {
                params: vec![ValueType::HostHandle, ValueType::I32],
                return_type: ValueType::Unit,
                locals: vec![ValueType::HostHandle, ValueType::I32],
                registers: vec![ValueType::HostHandle, ValueType::I32],
                effects: crate::module::EffectSet::path_write(),
                ..Default::default()
            },
            instructions: vec![
                BytecodeInstruction::SetPath {
                    root_or_view: Register::new(0),
                    path: PathId::new(0),
                    dynamic_args: Vec::new(),
                    value: Register::new(1),
                },
                BytecodeInstruction::Return(None),
            ],
        }],
        ..Default::default()
    };
    assert!(matches!(
        verify_module(&read_only_path),
        Err(BytecodeVerificationError::ReadOnlyPath { .. })
    ));
}

#[test]
fn verifier_rejects_malformed_debug_metadata() {
    let mut bytecode = common::bytecode_ok("fn main() -> i32 { 1 }");
    let function = &mut bytecode.functions[0];
    let mut point = function.metadata.debug.safe_debug_points[0].clone();
    point.instruction_offset = function.instructions.len();
    function.metadata.debug.safe_debug_points.push(point);

    assert!(matches!(
        verify_module(&bytecode),
        Err(BytecodeVerificationError::InvalidJumpTarget { .. })
    ));
}

#[test]
fn lowers_arithmetic_into_real_bytecode_instructions() {
    let bytecode = common::bytecode_ok("fn add(a: i32, b: i32) -> i32 { val c = a + b; c }");
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Binary {
            op: BinaryOp::Add,
            ..
        }
    )));
}

#[test]
fn flattens_branch_targets_to_instruction_offsets() {
    let bytecode = common::bytecode_ok("fn main() -> i32 { if true { 1 } else { 2 } }");
    let function = &bytecode.functions[0];

    let targets = function
        .instructions
        .iter()
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::Branch {
                then_target,
                else_target,
                ..
            } => Some([then_target.index(), else_target.index()]),
            BytecodeInstruction::Jump { target } => Some([target.index(), target.index()]),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();

    assert!(!targets.is_empty());
    assert!(
        targets
            .iter()
            .all(|target| *target < function.instructions.len())
    );
}

#[test]
fn lowers_direct_function_calls_to_function_refs() {
    let bytecode = common::bytecode_ok(
        r#"
fn callee() -> i32 { 1 }
fn caller() -> i32 { callee() }
"#,
    );
    let function = &bytecode.functions[1];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::Function(_),
            ..
        }
    )));
}

#[test]
fn lowers_unary_and_short_circuit_expressions() {
    let bytecode = common::bytecode_ok("fn main() -> bool { !false && true }");
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Unary {
            op: UnaryOp::Not,
            ..
        }
    )));

    let branch_count = function
        .instructions
        .iter()
        .filter(|instruction| matches!(instruction, BytecodeInstruction::Branch { .. }))
        .count();
    assert!(branch_count >= 1);
}

#[test]
fn lowers_loops_and_loop_control_to_jumps() {
    let bytecode = common::bytecode_ok(
        r#"
fn main() -> () {
    while true { break; }
    loop { continue; }
}
"#,
    );
    let function = &bytecode.functions[0];

    let jump_count = function
        .instructions
        .iter()
        .filter(|instruction| matches!(instruction, BytecodeInstruction::Jump { .. }))
        .count();
    assert!(jump_count >= 3);

    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::Branch { .. }))
    );
}

#[test]
fn lowers_aggregate_and_access_instructions() {
    let bytecode = common::bytecode_ok(
        r#"
struct Point { var x: i32 }

fn main() -> () {
    val tuple = (1, 2);
    val array = [1, 2];
    val point = Point { x: 1 };
    tuple;
    array[0];
    point.x;
}
"#,
    );
    let function = &bytecode.functions[0];

    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::MakeTuple { .. }))
    );
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::MakeArray { .. }))
    );
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::MakeStruct { .. }))
    );
    assert!(
        function.instructions.iter().any(|instruction| matches!(
            instruction,
            BytecodeInstruction::ReadAggregateIndex { .. }
        ))
    );
    assert!(
        function.instructions.iter().any(|instruction| matches!(
            instruction,
            BytecodeInstruction::ReadAggregateField { .. }
        ))
    );
    assert!(
        bytecode
            .structures
            .iter()
            .flat_map(|layout| &layout.fields)
            .any(|field| field.name == "x")
    );
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::ReadAggregateField { field, .. }
            if bytecode.structures.get(field.structure.index()).and_then(|layout| layout.fields.get(field.slot as usize)).is_some_and(|record| record.name == "x")
    )));
}

#[test]
fn verifier_accepts_resolved_typed_path_instructions() {
    let mut module = BytecodeModule {
        types: vec![ValueType::HostHandle, ValueType::I32],
        paths: vec![PathRecord {
            id: PathId::new(0),
            root_ty: ValueType::HostHandle,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "Actor.health".to_owned(),
        }],
        function_table: vec![crate::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            name: "read_health".to_owned(),
            params: vec![ValueType::HostHandle],
            return_type: ValueType::I32,
            effects: crate::module::EffectSet::path_read(),
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            name: "read_health".to_owned(),
            parameter_count: 1,
            local_count: 1,
            register_count: 2,
            metadata: FunctionMetadata {
                params: vec![ValueType::HostHandle],
                return_type: ValueType::I32,
                locals: vec![ValueType::HostHandle],
                registers: vec![ValueType::HostHandle, ValueType::I32],
                effects: crate::module::EffectSet::path_read(),
                ..Default::default()
            },
            instructions: vec![
                BytecodeInstruction::ReadPath {
                    dst: Register::new(1),
                    root_or_view: Register::new(0),
                    path: PathId::new(0),
                    dynamic_args: Vec::new(),
                },
                BytecodeInstruction::Return(Some(Register::new(1))),
            ],
        }],
        ..Default::default()
    };

    assert!(verify_module(&module).is_ok());
    module.paths[0].root_ty = ValueType::HeapObject;
    assert_eq!(
        verify_module(&module),
        Err(BytecodeVerificationError::InvalidPathLayout)
    );
}

#[test]
fn lowers_named_match_pattern_to_local_traffic() {
    let bytecode =
        common::bytecode_ok("fn main(value: i32) -> i32 { match value { bound => bound } }");
    let function = &bytecode.functions[0];

    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::StoreLocal { .. }))
    );
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::LoadLocal { .. }))
    );
}

#[test]
fn lowers_type_of_builtin_to_runtime_helper_call() {
    let bytecode = common::bytecode_ok("fn main() -> String { type_of(7) }");
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectTypeOf),
            ..
        }
    )));
}

#[test]
fn lowers_reflection_field_builtins_to_runtime_helper_calls() {
    let bytecode = common::bytecode_ok(
        r#"
struct Point { var x: i32 }

fn main() -> Point {
    val point = Point { x: 1 };
    val next = set_field(point, "x", 9);
    get_field(next, "x");
    next
}
"#,
    );
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetField(field)),
            ..
        } if field == "x"
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectGetField(field)),
            ..
        } if field == "x"
    )));
}

#[test]
fn lowers_set_index_builtin_to_runtime_helper_call() {
    let bytecode = common::bytecode_ok(
        r#"
fn main(values: [i32]) -> [i32] {
    set_index(values, 0, 9)
}
"#,
    );
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetIndex),
            ..
        }
    )));
}

#[test]
fn lowers_place_assignments_to_aggregate_writes() {
    let bytecode = common::bytecode_ok(
        r#"
struct Point { var x: i32 }
struct Holder { var inner: Point }

fn main() -> i32 {
    var holder = Holder { inner: Point { x: 1 } };
    holder.inner.x = 7;
    var values = [1, 2];
    values[0] = 5;
    holder.inner.x + values[0]
}
"#,
    );
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::WriteAggregateField { field, .. }
            if bytecode.structures.get(field.structure.index()).and_then(|layout| layout.fields.get(field.slot as usize)).is_some_and(|record| record.name == "x")
    )));
    assert!(
        function.instructions.iter().any(|instruction| matches!(
            instruction,
            BytecodeInstruction::WriteAggregateIndex { .. }
        ))
    );
    assert!(!function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(
                RuntimeHelper::ReflectSetField(_) | RuntimeHelper::ReflectSetIndex
            ),
            ..
        }
    )));
    assert!(function.metadata.effects.writes_aggregate);
    assert!(!function.metadata.effects.calls);
}

#[test]
fn preserves_module_init_function_metadata_in_bytecode() {
    let bytecode = common::bytecode_ok(
        r#"
val boot = 1;

fn main() -> i32 { 1 }
"#,
    );

    assert!(bytecode.module_init.is_some());
}

#[test]
fn does_not_allocate_module_slots_for_const_items() {
    let bytecode = common::bytecode_ok(
        r#"
const BASE: i32 = 1;
const VALUE: i32 = BASE + 2;

fn main() -> i32 { VALUE }
"#,
    );
    let function = bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");

    assert!(bytecode.module_slots.is_empty());
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::LoadConst { .. }))
    );
    assert!(
        !function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::LoadModule { .. }))
    );
}

#[test]
fn stdlib_lowers_standard_library_calls_to_bytecode_intrinsic_ids() {
    let bytecode = common::bytecode_ok(
        r#"
fn main() -> usize {
    val values = [1, 2];
    values.push(3);
    values.pop();
    values.len()
}
"#,
    );
    let function = bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayPush),
            ..
        }
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayPop),
            ..
        }
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayLen),
            ..
        }
    )));
}

#[test]
fn artifact_loader_rejects_invalid_struct_layouts_slots_and_initializers() {
    let original = common::bytecode_ok(
        "struct P { var x: i32, val fixed: bool } fn main() -> i32 { val p = P { fixed: true, x: 1 }; p.x = 2; p.x }",
    );
    let valid = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![original.clone()],
        },
        Default::default(),
    )
    .unwrap();
    for corruption in 0..7 {
        let mut module = original.clone();
        match corruption {
            0 => module.structures.push(module.structures[0].clone()),
            1 => module.structures[0].fields[0]
                .declaration
                .module
                .path
                .push("foreign".into()),
            2 => module.structures[0].fields[0].mutable = false,
            _ => {
                for instruction in module
                    .functions
                    .iter_mut()
                    .flat_map(|function| &mut function.instructions)
                {
                    match instruction {
                        BytecodeInstruction::MakeStruct {
                            structure, fields, ..
                        } => match corruption {
                            3 => *structure = StructId::new(999),
                            4 => {
                                fields.pop();
                            }
                            5 => fields.swap(0, 1),
                            _ => {}
                        },
                        BytecodeInstruction::ReadAggregateField { field, .. }
                            if corruption == 6 =>
                        {
                            field.slot = u32::MAX
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(
            KbcArtifact::from_program(
                crate::bytecode::BytecodeProgram {
                    root: crate::bytecode::ModuleRef::new(0),
                    modules: vec![module.clone()],
                },
                ArtifactBuildOptions::default(),
            )
            .is_err()
        );
        let mut corrupted = valid.clone();
        corrupted.program.modules[0] = module;
        let bytes = corrupted.to_bytes().unwrap();
        let artifact = KbcArtifact::from_bytes(&bytes).unwrap();
        assert!(
            matches!(
                artifact.validate_for_loader(&ArtifactCompatibility::default()),
                Err(ArtifactValidationError::Bytecode(_))
            ),
            "corruption {corruption}"
        );
    }
}
