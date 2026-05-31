use crate::{
    builtin::array,
    bytecode::{
        ArtifactBuildOptions, ArtifactCompatibility, ArtifactFingerprint, ArtifactModuleIdentity,
        ArtifactSectionId, ArtifactValidationError, BinaryOp, BuiltinMethod, BytecodeFunction,
        BytecodeInstruction, BytecodeModule, BytecodeVerificationError, CallTarget, DebugMetadata,
        DependencyFingerprint, FieldId, FieldRecord, FunctionMetadata, FunctionRef, JumpTarget,
        KBC_MAGIC, KbcArtifact, LocalSlot, PathId, PathRecord, Register, RuntimeHelper,
        SafeDebugPointKind, UnaryOp, verify_module,
    },
    module::ValueType,
    tests::common,
};

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
    let module = common::bytecode_ok(
        r#"
fn add(a: i32, b: i32) -> i32 { a + b }
fn main() -> i32 { add(1, 2) }
"#,
    );
    let identity = ArtifactModuleIdentity {
        package_id: "pkg".to_owned(),
        module_path: "main".to_owned(),
        source_uri: "pkg://main.kg".to_owned(),
        module_id: "pkg/main".to_owned(),
    };
    let dependency = DependencyFingerprint {
        module_id: "pkg/math".to_owned(),
        fingerprint: ArtifactFingerprint::of_str("math-v1"),
    };
    let options = ArtifactBuildOptions {
        module_identity: identity.clone(),
        dependency_fingerprints: vec![dependency.clone()],
        host_registry_fingerprint: ArtifactFingerprint::of_str("host-v1"),
        security_profile: Some("dev".to_owned()),
        ..Default::default()
    };
    let artifact = KbcArtifact::from_module(module, options);

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
        dependency_fingerprints: artifact.verification.loader.dependency_fingerprints.clone(),
        host_registry_fingerprint: artifact.verification.loader.host_registry_fingerprint,
        security_profile: Some("dev".to_owned()),
        ..Default::default()
    };
    assert!(artifact.validate_for_loader(&requirements).is_ok());
}

#[test]
fn rejects_incompatible_kbc_artifact_metadata_before_loading() {
    let module = common::bytecode_ok("fn main() -> i32 { 1 }");
    let mut artifact = KbcArtifact::from_module(module, ArtifactBuildOptions::default());
    let requirements = ArtifactCompatibility {
        runtime_abi_version: "other-runtime".to_owned(),
        ..Default::default()
    };

    assert!(matches!(
        artifact.validate_for_loader(&requirements),
        Err(ArtifactValidationError::RuntimeAbiMismatch { .. })
    ));

    let requirements = ArtifactCompatibility::default();
    artifact.module.constants.clear();
    assert!(matches!(
        artifact.validate_for_loader(&requirements),
        Err(ArtifactValidationError::ContentHashMismatch)
    ));

    let artifact = KbcArtifact::from_module(
        common::bytecode_ok("fn main() -> i32 { 1 }"),
        ArtifactBuildOptions {
            dependency_fingerprints: vec![DependencyFingerprint {
                module_id: "pkg/dependency".to_owned(),
                fingerprint: ArtifactFingerprint::of_str("dependency-v1"),
            }],
            ..Default::default()
        },
    );
    assert!(matches!(
        artifact.validate_for_loader(&ArtifactCompatibility::default()),
        Err(ArtifactValidationError::DependencyFingerprintMismatch)
    ));
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

    let mut invalid_local = common::bytecode_ok("fn main() -> i32 { val value = 1; value }");
    invalid_local.functions[0].instructions[1] = BytecodeInstruction::StoreLocal {
        local: LocalSlot::new(999),
        src: Register::new(0),
    };
    assert!(matches!(
        verify_module(&invalid_local),
        Err(BytecodeVerificationError::InvalidLocal { .. })
    ));

    let mut invalid_jump = common::bytecode_ok("fn main() -> i32 { if true { 1 } else { 2 } }");
    invalid_jump.functions[0]
        .metadata
        .control_flow_targets
        .push(JumpTarget::new(usize::MAX));
    assert!(matches!(
        verify_module(&invalid_jump),
        Err(BytecodeVerificationError::InvalidJumpTarget { .. })
    ));
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
fn verifier_rejects_invalid_aggregate_writes() {
    let module = BytecodeModule {
        types: vec![
            ValueType::Unit,
            ValueType::Bool,
            ValueType::I32,
            ValueType::HeapObject,
        ],
        fields: vec![FieldRecord {
            id: FieldId::new(0),
            owner: "Point".to_owned(),
            name: "x".to_owned(),
            ty: ValueType::I32,
        }],
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
                    field: FieldId::new(0),
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
        types: vec![ValueType::HeapObject, ValueType::I32],
        paths: vec![PathRecord {
            id: PathId::new(0),
            root_ty: ValueType::HeapObject,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "Actor.health".to_owned(),
        }],
        function_table: vec![crate::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            name: "read_missing_path".to_owned(),
            params: vec![ValueType::HeapObject],
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
                params: vec![ValueType::HeapObject],
                return_type: ValueType::I32,
                locals: vec![ValueType::HeapObject],
                registers: vec![ValueType::HeapObject, ValueType::I32],
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
        types: vec![ValueType::Unit, ValueType::HeapObject, ValueType::I32],
        paths: vec![PathRecord {
            id: PathId::new(0),
            root_ty: ValueType::HeapObject,
            result_ty: ValueType::I32,
            read_only: true,
            debug_name: "Actor.id".to_owned(),
        }],
        function_table: vec![crate::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            name: "write_readonly_path".to_owned(),
            params: vec![ValueType::HeapObject, ValueType::I32],
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
                params: vec![ValueType::HeapObject, ValueType::I32],
                return_type: ValueType::Unit,
                locals: vec![ValueType::HeapObject, ValueType::I32],
                registers: vec![ValueType::HeapObject, ValueType::I32],
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
    assert!(bytecode.fields.iter().any(|field| field.name == "x"));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::ReadAggregateField { field, .. }
            if bytecode.fields.get(field.index()).is_some_and(|record| record.name == "x")
    )));
}

#[test]
fn verifier_accepts_resolved_typed_path_instructions() {
    let module = BytecodeModule {
        types: vec![ValueType::HeapObject, ValueType::I32],
        paths: vec![PathRecord {
            id: PathId::new(0),
            root_ty: ValueType::HeapObject,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "Actor.health".to_owned(),
        }],
        function_table: vec![crate::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            name: "read_health".to_owned(),
            params: vec![ValueType::HeapObject],
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
                params: vec![ValueType::HeapObject],
                return_type: ValueType::I32,
                locals: vec![ValueType::HeapObject],
                registers: vec![ValueType::HeapObject, ValueType::I32],
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
            if bytecode.fields.get(field.index()).is_some_and(|record| record.name == "x")
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
fn lowers_array_methods_to_builtin_method_calls() {
    let bytecode = common::bytecode_ok(
        r#"
fn main() -> usize {
    val values = [1, 2];
    values.push(3).pop().len()
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
            callee: CallTarget::BuiltinMethod(BuiltinMethod::Array(array::Method::Push)),
            ..
        }
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::BuiltinMethod(BuiltinMethod::Array(array::Method::Pop)),
            ..
        }
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::BuiltinMethod(BuiltinMethod::Array(array::Method::Len)),
            ..
        }
    )));
}
