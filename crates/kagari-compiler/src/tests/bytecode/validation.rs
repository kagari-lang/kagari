mod slot_facts;

use crate::tests::bytecode::{path_fixtures::actor_path_module, *};
use kagari_bytecode::instruction::Slot;
use kagari_bytecode::module::InstructionOrigin;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{ConstantId, ConstantOperand, NativeImportId},
    module::{FunctionRecord, LocalLiveRange, RootSlotLayout},
    program::verify_program,
};
use kagari_contract::{effects::EffectSet, standard::RuntimePrimitive};
use kagari_types::{declaration::host::PathAccess, range::RangeKind, scalar::BuiltinType, ty::Ty};

fn replace_fixture_code(function: &mut BytecodeFunction, instructions: Vec<BytecodeInstruction>) {
    function.metadata.origins = (0..instructions.len())
        .map(|instruction| InstructionOrigin {
            span: Default::default(),
            line: None,
            column: None,
            instruction,
        })
        .collect();
    function.metadata.debug = Default::default();
    function.metadata.instruction_origins.clear();
    function.metadata.control_flow_targets.clear();
    function.instructions = instructions;
}

#[test]
fn ordinary_slot_initialization_requires_the_first_entry_and_every_join_edge() {
    let context = DefinitionContext::new().unwrap();
    let valid = common::adopt_bytecode(
        &context,
        common::bytecode_ok(
            "fn probe(flag: bool, x: i32) -> i32 { x } fn main() -> i32 { probe(true, 1) }",
        ),
    );
    for loop_read in [false, true] {
        let mut forged = valid.clone();
        let module = &mut forged.modules[forged.root.index()];
        let function = module
            .functions
            .iter_mut()
            .find(|function| function.name == "probe")
            .unwrap();
        let flag = function.metadata.parameter_slots[0];
        let x = function.metadata.parameter_slots[1];
        let scratch = Slot::new(function.metadata.slots.len());
        function.metadata.slots.push(ValueType::I32);
        function
            .metadata
            .semantic
            .slots
            .insert(scratch.index(), Ty::Builtin(BuiltinType::I32));
        function.slot_count += 1;
        let instructions = if loop_read {
            vec![
                BytecodeInstruction::Jump {
                    target: JumpTarget::new(1),
                },
                BytecodeInstruction::Binary {
                    dst: scratch,
                    op: BinaryOp::Add,
                    lhs: scratch,
                    rhs: x,
                },
                BytecodeInstruction::Jump {
                    target: JumpTarget::new(1),
                },
            ]
        } else {
            vec![
                BytecodeInstruction::Branch {
                    cond: flag,
                    then_target: JumpTarget::new(1),
                    else_target: JumpTarget::new(2),
                },
                BytecodeInstruction::Move {
                    dst: scratch,
                    src: x,
                },
                BytecodeInstruction::Return(Some(scratch)),
            ]
        };
        replace_fixture_code(function, instructions);
        let expected = BytecodeVerificationError::InvalidOperation {
            function: function.id,
            reason: "uninitialized executable slot",
        };
        assert_eq!(
            verify_program(&forged, &context.snapshot(), &Default::default()),
            Err(expected.clone())
        );
        let mut artifact = KbcArtifact::from_program(
            valid.clone(),
            Default::default(),
            &context,
            &Default::default(),
        )
        .unwrap();
        artifact.program = forged;
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
            .map(|metadata| common::adopt(&context, metadata))
            .unwrap();
        assert!(
            matches!(decoded.validate_for_loader(&Default::default(), &context, &Default::default()), Err(ArtifactValidationError::Bytecode(error)) if error == expected)
        );
    }
}

#[test]
fn overwritten_and_joined_constant_facts_cannot_forge_tuple_element_types() {
    let context = DefinitionContext::new().unwrap();
    let valid = common::adopt_bytecode(
        &context,
        common::bytecode_ok(
            "fn probe(flag: bool) -> bool { val pair = (1, true); pair[1] } fn main() -> bool { probe(true) }",
        ),
    );
    for join in [false, true] {
        let mut forged = valid.clone();
        let module = &mut forged.modules[forged.root.index()];
        let zero = module
            .constants
            .iter()
            .position(|constant| matches!(constant, ConstantOperand::I32(0)))
            .unwrap_or_else(|| {
                module.constants.push(ConstantOperand::I32(0));
                module.constants.len() - 1
            });
        let one = module
            .constants
            .iter()
            .position(|constant| matches!(constant, ConstantOperand::I32(1)))
            .unwrap();
        let function = module
            .functions
            .iter_mut()
            .find(|function| function.name == "probe")
            .unwrap();
        let (pc, dst, index) = function
            .instructions
            .iter()
            .enumerate()
            .find_map(|(pc, instruction)| match instruction {
                BytecodeInstruction::ReadAggregateIndex { dst, index, .. } => {
                    Some((pc, *dst, *index))
                }
                _ => None,
            })
            .unwrap();
        let read = function.instructions[pc].clone();
        let mut code = function.instructions[..pc].to_vec();
        if join {
            let start = code.len();
            code.extend([
                BytecodeInstruction::Branch {
                    cond: function.metadata.parameter_slots[0],
                    then_target: JumpTarget::new(start + 1),
                    else_target: JumpTarget::new(start + 3),
                },
                BytecodeInstruction::LoadConst {
                    dst: index,
                    constant: ConstantId::new(zero),
                },
                BytecodeInstruction::Jump {
                    target: JumpTarget::new(start + 5),
                },
                BytecodeInstruction::LoadConst {
                    dst: index,
                    constant: ConstantId::new(one),
                },
                BytecodeInstruction::Jump {
                    target: JumpTarget::new(start + 5),
                },
            ]);
        } else {
            code.push(BytecodeInstruction::Binary {
                dst: index,
                op: BinaryOp::Add,
                lhs: index,
                rhs: index,
            });
        }
        code.extend([read, BytecodeInstruction::Return(Some(dst))]);
        replace_fixture_code(function, code);
        assert!(matches!(
            verify_program(&forged, &context.snapshot(), &Default::default()),
            Err(BytecodeVerificationError::InvalidOperation {
                reason: "invalid collection access flow",
                ..
            })
        ));
        let mut artifact = KbcArtifact::from_program(
            valid.clone(),
            Default::default(),
            &context,
            &Default::default(),
        )
        .unwrap();
        artifact.program = forged;
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
            .map(|metadata| common::adopt(&context, metadata))
            .unwrap();
        assert!(matches!(
            decoded.validate_for_loader(&Default::default(), &context, &Default::default()),
            Err(ArtifactValidationError::Bytecode(
                BytecodeVerificationError::InvalidOperation {
                    reason: "invalid collection access flow",
                    ..
                }
            ))
        ));
    }
}

#[test]
fn incompatible_semantic_types_at_a_join_do_not_become_unchecked_heap_values() {
    let context = DefinitionContext::new().unwrap();
    let valid = common::adopt_bytecode(
        &context,
        common::bytecode_ok(
            "fn probe(flag: bool, a: (i32, bool), b: (i32, i32)) -> bool { a[1] } fn main() -> bool { probe(true, (1, true), (1, 2)) }",
        ),
    );
    let mut forged = valid.clone();
    let function = forged.modules[forged.root.index()]
        .functions
        .iter_mut()
        .find(|function| function.name == "probe")
        .unwrap();
    let (pc, dst, index) = function
        .instructions
        .iter()
        .enumerate()
        .find_map(|(pc, instruction)| match instruction {
            BytecodeInstruction::ReadAggregateIndex { dst, index, .. } => Some((pc, *dst, *index)),
            _ => None,
        })
        .unwrap();
    let joined = Slot::new(function.metadata.slots.len());
    function.metadata.slots.push(ValueType::HeapObject);
    function.slot_count += 1;
    function.metadata.roots =
        kagari_bytecode::module::RootSlotLayout::from_types(&function.metadata.slots);
    let mut code = function.instructions[..pc].to_vec();
    let start = code.len();
    code.extend([
        BytecodeInstruction::Branch {
            cond: function.metadata.parameter_slots[0],
            then_target: JumpTarget::new(start + 1),
            else_target: JumpTarget::new(start + 3),
        },
        BytecodeInstruction::Move {
            dst: joined,
            src: function.metadata.parameter_slots[1],
        },
        BytecodeInstruction::Jump {
            target: JumpTarget::new(start + 5),
        },
        BytecodeInstruction::Move {
            dst: joined,
            src: function.metadata.parameter_slots[2],
        },
        BytecodeInstruction::Jump {
            target: JumpTarget::new(start + 5),
        },
        BytecodeInstruction::ReadAggregateIndex {
            dst,
            base: joined,
            index,
        },
        BytecodeInstruction::Return(Some(dst)),
    ]);
    replace_fixture_code(function, code);
    let expected = BytecodeVerificationError::InvalidOperation {
        function: function.id,
        reason: "invalid collection access flow",
    };
    assert_eq!(
        verify_program(&forged, &context.snapshot(), &Default::default()),
        Err(expected.clone())
    );
    let mut artifact =
        KbcArtifact::from_program(valid, Default::default(), &context, &Default::default())
            .unwrap();
    artifact.program = forged;
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| common::adopt(&context, metadata))
        .unwrap();
    assert!(
        matches!(decoded.validate_for_loader(&Default::default(), &context, &Default::default()), Err(ArtifactValidationError::Bytecode(error)) if error == expected)
    );
}

#[test]
fn rejects_function_fallthrough_before_loading() {
    let context = DefinitionContext::new().unwrap();
    for source in ["fn main() {}", "fn main() -> i32 { 42 }"] {
        let mut module = common::adopt_bytecode(&context, common::bytecode_ok(source));
        let function = module.modules[module.root.index()]
            .functions
            .iter_mut()
            .find(|function| function.name == "main")
            .unwrap();
        assert!(matches!(
            function.instructions.last(),
            Some(BytecodeInstruction::Return(_))
        ));
        function.instructions.pop();
        assert!(matches!(
            verify_program(&module, &context.snapshot(), &Default::default()),
            Err(BytecodeVerificationError::InvalidOperation {
                reason: "function falls through without a terminator",
                ..
            })
        ));
        assert!(
            KbcArtifact::from_program(
                module,
                ArtifactBuildOptions::default(),
                &context,
                &Default::default()
            )
            .is_err()
        );
    }
}

#[test]
fn verifier_rejects_vec_get_scalar_result_and_wrong_arity() {
    let context = DefinitionContext::new().unwrap();
    let module = common::adopt_bytecode(
        &context,
        common::bytecode_ok(
            "fn main() -> bool { val a = Vec::from([7]); match a.get(a.len()) { None => true, Some(_) => false } }",
        ),
    );
    let get = NativeImportId::new(
        module.modules[module.root.index()]
            .native_imports
            .iter()
            .position(|import| {
                context
                    .snapshot()
                    .resolve(import.binding)
                    .unwrap()
                    .last()
                    .is_some_and(|part| part.name == "get")
            })
            .unwrap(),
    );
    let mut scalar_result = module.clone();
    let function = &mut scalar_result.modules[scalar_result.root.index()].functions[0];
    let dst = function
        .instructions
        .iter()
        .find_map(|instruction| {
            if let BytecodeInstruction::Call {
                dst,
                callee: CallTarget::Native(import),
                ..
            } = instruction
            {
                if *import != get {
                    return None;
                }
                *dst
            } else {
                None
            }
        })
        .unwrap();
    function.metadata.slots[dst.index()] = ValueType::I32;
    function
        .metadata
        .roots
        .slots
        .retain(|register| *register != dst);
    assert!(matches!(
        verify_program(&scalar_result, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::TypeMismatch {
            expected: ValueType::HeapObject,
            found: ValueType::I32,
            ..
        })
    ));

    let mut wrong_arity = module;
    for instruction in &mut wrong_arity.modules[wrong_arity.root.index()].functions[0].instructions
    {
        if let BytecodeInstruction::Call {
            callee: CallTarget::Native(import),
            args,
            ..
        } = instruction
            && *import == get
        {
            args.pop();
        }
    }
    assert!(matches!(
        verify_program(&wrong_arity, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidOperation {
            reason: "native call arity mismatch",
            ..
        })
    ));
}

#[test]
fn encoded_root_layout_must_cover_exact_heap_slots() {
    let context = DefinitionContext::new().unwrap();
    let module = common::adopt_bytecode(
        &context,
        common::bytecode_ok("fn main() -> i32 { val values = [7]; values[0] }"),
    );
    let function = &module.modules[module.root.index()].functions[0];
    assert!(!function.metadata.roots.slots.is_empty() || !function.metadata.roots.slots.is_empty());
    verify_program(&module, &context.snapshot(), &Default::default()).unwrap();
    let artifact = KbcArtifact::from_program(
        module,
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    for corruption in ["missing", "extra"] {
        let mut forged = artifact.clone();
        let roots = &mut forged.program.modules[forged.program.root.index()].functions[0]
            .metadata
            .roots;
        if corruption == "missing" {
            roots.slots.pop().expect("the fixture has traced slots");
        } else {
            roots.slots.push(Slot::new(0));
        }
        assert!(matches!(
            verify_program(&forged.program, &context.snapshot(), &Default::default()),
            Err(BytecodeVerificationError::InvalidRootLayout { .. })
        ));
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
                BytecodeVerificationError::InvalidRootLayout { .. }
            ))
        ));
    }
}

#[test]
fn verifier_rejects_malformed_register_local_and_control_flow_bytecode() {
    let context = DefinitionContext::new().unwrap();
    let artifact = KbcArtifact::from_program(
        common::adopt_bytecode(&context, common::bytecode_ok("fn main() -> i32 { 1 }")),
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    for duplicate in [false, true] {
        let mut forged = artifact.clone();
        let module = &mut forged.program.modules[forged.program.root.index()];
        if duplicate {
            module.constants.push(module.constants[0].clone());
        } else {
            let BytecodeInstruction::LoadConst { constant, .. } =
                &mut module.functions[0].instructions[0]
            else {
                panic!("expected constant")
            };
            *constant = ConstantId::new(module.constants.len());
        }
        let expected = if duplicate {
            BytecodeVerificationError::DuplicateConstant
        } else {
            BytecodeVerificationError::MissingConstant {
                function: FunctionRef::new(0),
            }
        };
        assert_eq!(
            verify_program(&forged.program, &context.snapshot(), &Default::default()).unwrap_err(),
            expected
        );
        let decoded = KbcArtifact::from_bytes(&forged.to_bytes(&context.snapshot()).unwrap())
            .map(|metadata| common::adopt(&context, metadata))
            .unwrap();
        assert!(
            matches!(decoded.validate_for_loader(&ArtifactCompatibility::default(), &context, &Default::default()), Err(ArtifactValidationError::Bytecode(error)) if error == expected)
        );
    }
    let mut invalid_register =
        common::adopt_bytecode(&context, common::bytecode_ok("fn main() -> i32 { 1 }"));
    invalid_register.modules[invalid_register.root.index()].functions[0].instructions[0] =
        BytecodeInstruction::LoadConst {
            dst: Slot::new(999),
            constant: ConstantId::new(0),
        };
    assert!(matches!(
        verify_program(&invalid_register, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidRegister { .. })
    ));
    assert_eq!(
        verify_program(&invalid_register, &context.snapshot(), &Default::default())
            .unwrap_err()
            .code(),
        "KG_BYTECODE_INVALID_REGISTER"
    );

    let mut invalid_local = common::adopt_bytecode(
        &context,
        common::bytecode_ok("fn main() -> i32 { val value = 1; value }"),
    );
    invalid_local.modules[invalid_local.root.index()].functions[0].instructions[1] =
        BytecodeInstruction::Move {
            dst: Slot::new(999),
            src: Slot::new(0),
        };
    assert!(matches!(
        verify_program(&invalid_local, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidRegister { .. })
    ));
    assert_eq!(
        verify_program(&invalid_local, &context.snapshot(), &Default::default())
            .unwrap_err()
            .code(),
        "KG_BYTECODE_INVALID_REGISTER"
    );

    let mut invalid_jump = common::adopt_bytecode(
        &context,
        common::bytecode_ok("fn main() -> i32 { if true { 1 } else { 2 } }"),
    );
    invalid_jump.modules[invalid_jump.root.index()].functions[0]
        .metadata
        .control_flow_targets
        .push(JumpTarget::new(usize::MAX));
    assert!(matches!(
        verify_program(&invalid_jump, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidJumpTarget { .. })
    ));
    assert_eq!(
        verify_program(&invalid_jump, &context.snapshot(), &Default::default())
            .unwrap_err()
            .code(),
        "KG_BYTECODE_INVALID_JUMP_TARGET"
    );
}

#[test]
fn verifier_rejects_type_inconsistent_bytecode() {
    let context = DefinitionContext::new().unwrap();
    let mut bytecode =
        common::adopt_bytecode(&context, common::bytecode_ok("fn main() -> i32 { 1 }"));
    bytecode.modules[bytecode.root.index()].functions[0]
        .metadata
        .return_type = ValueType::Bool;
    bytecode.modules[bytecode.root.index()].function_table[0].return_type = ValueType::Bool;
    bytecode.modules[bytecode.root.index()]
        .types
        .push(ValueType::Bool);

    assert!(matches!(
        verify_program(&bytecode, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::TypeMismatch {
            context: "return value",
            expected: ValueType::Bool,
            found: ValueType::I32,
            ..
        })
    ));
}

#[test]
fn verifier_rejects_invalid_language_primitive_signatures() {
    let context = DefinitionContext::new().unwrap();
    let mut bytecode = common::adopt_bytecode(
        &context,
        common::bytecode_ok(
            r#"
fn main(value: Vec<i32>) -> u64 {
    value.len()
}
"#,
        ),
    );
    let call = bytecode.modules[bytecode.root.index()].functions[0]
        .instructions
        .iter_mut()
        .find_map(|instruction| {
            let BytecodeInstruction::Call { callee, .. } = instruction else {
                return None;
            };
            Some(callee)
        })
        .expect("expected standard intrinsic call");
    *call = CallTarget::RuntimePrimitive(RuntimePrimitive::ValueCmp);

    assert!(matches!(
        verify_program(&bytecode, &context.snapshot(), &Default::default()),
        Err(
            BytecodeVerificationError::RuntimePrimitiveSignatureMismatch {
                intrinsic: RuntimePrimitive::ValueCmp,
                reason: "invalid language primitive operands",
                ..
            }
        )
    ));

    let artifact = KbcArtifact::from_program(
        bytecode,
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    );
    assert!(matches!(
        artifact,
        Err(ArtifactValidationError::Bytecode(
            BytecodeVerificationError::RuntimePrimitiveSignatureMismatch {
                intrinsic: RuntimePrimitive::ValueCmp,
                reason: "invalid language primitive operands",
                ..
            }
        ))
    ));
}

#[test]
fn verifier_rejects_invalid_aggregate_writes() {
    let context = DefinitionContext::new().unwrap();
    let module = BytecodeModule {
        types: vec![
            ValueType::Unit,
            ValueType::Bool,
            ValueType::I32,
            ValueType::HeapObject,
        ],
        structures: {
            let program = common::adopt_bytecode(
                &context,
                common::bytecode_ok("struct Point { var x: i32 }"),
            );
            program.modules[program.root.index()].structures.clone()
        },
        function_table: vec![FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: "write_bad_field".to_owned(),
            params: Vec::new(),
            return_type: ValueType::Unit,
            effects: EffectSet::aggregate_write(),
        }],
        functions: vec![{
            let fixture_instructions = vec![
                BytecodeInstruction::WriteAggregateField {
                    base: Slot::new(0),
                    field: FieldRef {
                        structure: StructId::new(0),
                        arguments: vec![],
                        slot: 0,
                        reference: None,
                    },
                    value: Slot::new(1),
                },
                BytecodeInstruction::Return(None),
            ];
            let mut fixture_metadata: kagari_bytecode::module::FunctionMetadata =
                FunctionMetadata {
                    return_type: ValueType::Unit,
                    roots: RootSlotLayout {
                        slots: vec![Slot::new(0)],
                    },
                    effects: EffectSet::aggregate_write(),
                    slots: [vec![ValueType::HeapObject, ValueType::Bool], vec![].clone()].concat(),
                    ..Default::default()
                };

            if fixture_metadata.origins.is_empty() {
                fixture_metadata.origins = (0..fixture_instructions.len())
                    .map(|instruction| kagari_bytecode::module::InstructionOrigin {
                        span: Default::default(),
                        line: None,
                        column: None,
                        instruction,
                    })
                    .collect();
            }
            BytecodeFunction {
                id: FunctionRef::new(0),
                identity: None,
                name: "write_bad_field".to_owned(),
                parameter_count: 0,
                slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
                metadata: fixture_metadata,
                instructions: fixture_instructions,
            }
        }],
        ..Default::default()
    };

    context.intern_root(&module.identity).unwrap();
    assert!(matches!(
        verify_module(&module, &context.snapshot(), &Default::default()),
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
    let context = DefinitionContext::new().unwrap();
    let unresolved_path = BytecodeModule {
        types: vec![ValueType::HostHandle, ValueType::I32],
        function_table: vec![FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: "read_missing_path".to_owned(),
            params: vec![ValueType::HostHandle],
            return_type: ValueType::I32,
            effects: EffectSet::path_read(),
        }],
        functions: vec![{
            let parameter_base: usize = 2;
            let fixture_instructions = vec![
                BytecodeInstruction::ReadPath {
                    dst: Slot::new(1),
                    root_or_view: Slot::new(0),
                    path: PathId::new(99),
                    dynamic_args: Vec::new(),
                },
                BytecodeInstruction::Return(Some(Slot::new(1))),
            ];
            let mut fixture_metadata: kagari_bytecode::module::FunctionMetadata =
                FunctionMetadata {
                    params: vec![ValueType::HostHandle],
                    return_type: ValueType::I32,
                    effects: EffectSet::path_read(),
                    slots: [
                        vec![ValueType::HostHandle, ValueType::I32],
                        vec![ValueType::HostHandle],
                    ]
                    .concat(),
                    ..Default::default()
                };
            if fixture_metadata.parameter_slots.is_empty() {
                fixture_metadata.parameter_slots =
                    (0..1).map(|i| Slot::new(parameter_base + i)).collect();
            }
            if fixture_metadata.origins.is_empty() {
                fixture_metadata.origins = (0..fixture_instructions.len())
                    .map(|instruction| kagari_bytecode::module::InstructionOrigin {
                        span: Default::default(),
                        line: None,
                        column: None,
                        instruction,
                    })
                    .collect();
            }
            BytecodeFunction {
                id: FunctionRef::new(0),
                identity: None,
                name: "read_missing_path".to_owned(),
                parameter_count: 1,
                slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
                metadata: fixture_metadata,
                instructions: fixture_instructions,
            }
        }],
        ..actor_path_module(&context, "health", PathAccess::ReadWrite)
    };
    assert!(matches!(
        verify_module(&unresolved_path, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidPathId { .. })
    ));

    let read_only_path = BytecodeModule {
        types: vec![ValueType::Unit, ValueType::HostHandle, ValueType::I32],
        function_table: vec![FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: "write_readonly_path".to_owned(),
            params: vec![ValueType::HostHandle, ValueType::I32],
            return_type: ValueType::Unit,
            effects: EffectSet::path_write(),
        }],
        functions: vec![{
            let parameter_base: usize = 2;
            let fixture_instructions = vec![
                BytecodeInstruction::SetPath {
                    root_or_view: Slot::new(0),
                    path: PathId::new(0),
                    dynamic_args: Vec::new(),
                    value: Slot::new(1),
                },
                BytecodeInstruction::Return(None),
            ];
            let mut fixture_metadata: kagari_bytecode::module::FunctionMetadata =
                FunctionMetadata {
                    params: vec![ValueType::HostHandle, ValueType::I32],
                    return_type: ValueType::Unit,
                    effects: EffectSet::path_write(),
                    slots: [
                        vec![ValueType::HostHandle, ValueType::I32],
                        vec![ValueType::HostHandle, ValueType::I32],
                    ]
                    .concat(),
                    ..Default::default()
                };
            if fixture_metadata.parameter_slots.is_empty() {
                fixture_metadata.parameter_slots =
                    (0..2).map(|i| Slot::new(parameter_base + i)).collect();
            }
            if fixture_metadata.origins.is_empty() {
                fixture_metadata.origins = (0..fixture_instructions.len())
                    .map(|instruction| kagari_bytecode::module::InstructionOrigin {
                        span: Default::default(),
                        line: None,
                        column: None,
                        instruction,
                    })
                    .collect();
            }
            BytecodeFunction {
                id: FunctionRef::new(0),
                identity: None,
                name: "write_readonly_path".to_owned(),
                parameter_count: 2,
                slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
                metadata: fixture_metadata,
                instructions: fixture_instructions,
            }
        }],
        ..actor_path_module(&context, "id", PathAccess::ReadOnly)
    };
    assert!(matches!(
        verify_module(&read_only_path, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::ReadOnlyPath { .. })
    ));
}

#[test]
fn source_free_origin_tables_reject_missing_out_of_range_and_reordered_pcs() {
    let context = DefinitionContext::new().unwrap();
    let valid = common::adopt_bytecode(
        &context,
        common::bytecode_ok("fn main() -> i32 { var x = 0; while x < 3 { x = x + 1; } x }"),
    );
    for mutation in 0..5 {
        let mut forged = valid.clone();
        let function = &mut forged.modules[forged.root.index()].functions[0];
        let origins = &mut function.metadata.origins;
        match mutation {
            0 => origins.clear(),
            1 => origins[0].instruction = function.instructions.len(),
            2 => origins.reverse(),
            3 => origins.retain(|origin| origin.instruction != 1),
            _ => origins.last_mut().unwrap().instruction = usize::MAX,
        }
        // Isolate PC shape validation from the separately tested wire-level choices.
        let mut first = 0;
        function.metadata.instruction_origins = origins
            .chunk_by(|a, b| a.instruction == b.instruction)
            .filter_map(|group| {
                let id = first;
                first += group.len();
                (group.len() > 1).then_some(id)
            })
            .collect();
        assert!(function.metadata.origin_selection_valid());
        let expected = BytecodeVerificationError::InvalidOperation {
            function: function.id,
            reason: "invalid executable origin mapping",
        };
        assert_eq!(
            verify_program(&forged, &context.snapshot(), &Default::default()),
            Err(expected.clone())
        );
        let mut artifact = KbcArtifact::from_program(
            valid.clone(),
            Default::default(),
            &context,
            &Default::default(),
        )
        .unwrap();
        artifact.program = forged;
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
            .map(|metadata| common::adopt(&context, metadata))
            .unwrap();
        assert!(
            matches!(decoded.validate_for_loader(&Default::default(), &context, &Default::default()), Err(ArtifactValidationError::Bytecode(error)) if error == expected)
        );
    }
}

#[test]
fn entry_backedges_meet_parameter_access_facts_without_a_declared_home_type() {
    let context = DefinitionContext::new().unwrap();
    let valid = common::adopt_bytecode(
        &context,
        common::bytecode_ok(
            "fn probe(a: (i32, bool), b: (i32, i32)) -> bool { a[1] } fn main() -> bool { probe((1, true), (1, 2)) }",
        ),
    );
    for incompatible in [false, true] {
        let mut forged = valid.clone();
        let function = forged.modules[forged.root.index()]
            .functions
            .iter_mut()
            .find(|function| function.name == "probe")
            .unwrap();
        let a = function.metadata.parameter_slots[0];
        let b = function.metadata.parameter_slots[1];
        let (dst, index) = function
            .instructions
            .iter()
            .find_map(|instruction| match instruction {
                BytecodeInstruction::ReadAggregateIndex { dst, index, .. } => Some((*dst, *index)),
                _ => None,
            })
            .unwrap();
        let constant = function.instructions.iter().find(|instruction| {
            matches!(instruction, BytecodeInstruction::LoadConst { dst, .. } if *dst == index)
        }).unwrap().clone();
        function.metadata.semantic.slots.remove(&a.index());
        replace_fixture_code(
            function,
            vec![
                constant,
                BytecodeInstruction::ReadAggregateIndex {
                    dst,
                    base: a,
                    index,
                },
                BytecodeInstruction::Move {
                    dst: a,
                    src: if incompatible { b } else { a },
                },
                BytecodeInstruction::Jump {
                    target: JumpTarget::new(0),
                },
            ],
        );
        let expected = if incompatible {
            Err(BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "invalid collection access flow",
            })
        } else {
            Ok(())
        };
        assert_eq!(
            verify_program(&forged, &context.snapshot(), &Default::default()),
            expected
        );
        let artifact_base = if incompatible {
            valid.clone()
        } else {
            forged.clone()
        };
        let mut artifact = KbcArtifact::from_program(
            artifact_base,
            Default::default(),
            &context,
            &Default::default(),
        )
        .unwrap();
        artifact.program = forged;
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
            .map(|metadata| common::adopt(&context, metadata))
            .unwrap();
        match decoded.validate_for_loader(&Default::default(), &context, &Default::default()) {
            Ok(()) => assert_eq!(expected, Ok(())),
            Err(ArtifactValidationError::Bytecode(error)) => assert_eq!(Err(error), expected),
            Err(error) => panic!("unexpected loader failure: {error}"),
        }
    }
}

#[test]
fn verifier_rejects_malformed_debug_metadata() {
    let context = DefinitionContext::new().unwrap();
    let mut bytecode =
        common::adopt_bytecode(&context, common::bytecode_ok("fn main() -> i32 { 1 }"));
    let mut invalid = bytecode.clone();
    let function = &mut invalid.modules[invalid.root.index()].functions[0];
    let mut point = function.metadata.debug.safe_debug_points[0].clone();
    point.instruction_offset = function.instructions.len();
    function.metadata.debug.safe_debug_points.push(point);

    assert!(matches!(
        verify_program(&invalid, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidJumpTarget { .. })
    ));

    let function = &mut bytecode.modules[bytecode.root.index()].functions[0];
    let mut instructions = function.instructions.clone();
    let unreachable_pc = instructions.len();
    instructions.push(BytecodeInstruction::Unreachable);
    replace_fixture_code(function, instructions);
    function
        .metadata
        .debug
        .local_live_ranges
        .push(LocalLiveRange {
            slot: Slot::new(0),
            binding: 0,
            name: String::new(),
            span: Default::default(),
            start: unreachable_pc,
            end: unreachable_pc,
            ty: function.metadata.slots[0],
            is_parameter: false,
        });
    verify_program(&bytecode, &context.snapshot(), &Default::default()).unwrap();

    for mutation in 0..3 {
        let mut invalid = bytecode.clone();
        let function = &mut invalid.modules[invalid.root.index()].functions[0];
        let range = &mut function.metadata.debug.local_live_ranges[0];
        let expected = match mutation {
            0 => {
                range.start = range.end + 1;
                BytecodeVerificationError::InvalidJumpTarget {
                    function: function.id,
                    target: JumpTarget::new(range.end),
                }
            }
            1 => {
                range.end = function.instructions.len() + 1;
                BytecodeVerificationError::InvalidJumpTarget {
                    function: function.id,
                    target: JumpTarget::new(range.end),
                }
            }
            2 => {
                range.slot = Slot::new(function.slot_count as usize);
                BytecodeVerificationError::InvalidRegister {
                    function: function.id,
                    register: range.slot,
                }
            }
            _ => unreachable!(),
        };
        assert_eq!(
            verify_program(&invalid, &context.snapshot(), &Default::default()),
            Err(expected),
            "debug range mutation {mutation}"
        );
    }
}

#[test]
fn ranges_reject_forged_shapes_endpoints_and_bounds() {
    let context = DefinitionContext::new().unwrap();

    let module = common::adopt_bytecode(
        &context,
        common::bytecode_ok("fn main() { val range = 0u64..2u64; range.start_bound(); }"),
    );
    verify_program(&module, &context.snapshot(), &Default::default()).unwrap();
    for mutation in 0..6 {
        let mut invalid = module.clone();
        let instruction = invalid.modules[invalid.root.index()]
            .functions
            .iter_mut()
            .flat_map(|f| &mut f.instructions)
            .find(|i| matches!(i, BytecodeInstruction::MakeRange { .. }))
            .unwrap();
        let BytecodeInstruction::MakeRange { ty, start, end, .. } = instruction else {
            unreachable!()
        };
        match mutation {
            0 => *start = None,
            1 => *end = None,
            2 => {
                *ty = Ty::Range(
                    Box::new(Ty::Builtin(BuiltinType::Bool)),
                    RangeKind::Exclusive,
                )
            }
            3 => *ty = Ty::Range(Box::new(Ty::Builtin(BuiltinType::U64)), RangeKind::Full),
            4 => *start = Some(Slot::new(u32::MAX as usize)),
            _ => {
                *ty = Ty::Range(
                    Box::new(Ty::Builtin(BuiltinType::I64)),
                    RangeKind::Exclusive,
                )
            }
        }
        assert!(
            verify_program(&invalid, &context.snapshot(), &Default::default()).is_err(),
            "range mutation {mutation}"
        );
    }
    let root = module.root.index();
    let import = module.modules[root]
        .native_imports
        .iter()
        .position(|import| {
            context
                .snapshot()
                .resolve(import.binding)
                .unwrap()
                .last()
                .is_some_and(|part| part.name == "start_bound")
        })
        .unwrap();
    for mutation in 0..4 {
        let mut invalid = module.clone();
        let contract = &mut invalid.modules[root].native_imports[import];
        match mutation {
            0 => contract.signature.result = Ty::Builtin(BuiltinType::Bool),
            1 => {
                let Ty::Enum(nominal) = &mut contract.signature.result else {
                    unreachable!()
                };
                nominal.arguments.clear();
            }
            2 => {
                contract.signature.params[0] = Ty::Range(
                    Box::new(Ty::Builtin(BuiltinType::I32)),
                    RangeKind::Exclusive,
                )
            }
            _ => {
                let instruction = invalid.modules[root]
                    .functions
                    .iter_mut()
                    .flat_map(|function| &mut function.instructions)
                    .find(|instruction| {
                        matches!(instruction, BytecodeInstruction::Call {
                        callee: CallTarget::Native(id), ..
                    } if id.index() == import)
                    })
                    .unwrap();
                let BytecodeInstruction::Call { args, .. } = instruction else {
                    unreachable!()
                };
                args[0] = Slot::new(u32::MAX as usize);
            }
        }
        assert!(
            verify_program(&invalid, &context.snapshot(), &Default::default()).is_err(),
            "bound mutation {mutation}"
        );
    }
}

#[test]
fn forged_repetition_cannot_copy_shared_mutable_identities() {
    let context = DefinitionContext::new().unwrap();
    for value in [
        "Cell { value: 1 }",
        "(Cell { value: 1 }, 1)",
        "Some(Cell { value: 1 })",
    ] {
        let source = format!(
            "struct Cell {{ var value: i32 }} fn main() {{ val count = 2u64; val value = {value}; val array = [value, value]; }}"
        );
        let mut module = common::adopt_bytecode(&context, common::bytecode_ok(&source));
        verify_program(&module, &context.snapshot(), &Default::default()).unwrap();
        let constant = module.modules[module.root.index()]
            .constants
            .iter()
            .position(|constant| constant == &ConstantOperand::U64(2))
            .unwrap();
        let function = module.modules[module.root.index()]
            .functions
            .iter_mut()
            .find(|f| {
                f.instructions
                    .iter()
                    .any(|i| matches!(i, BytecodeInstruction::MakeArray { .. }))
            })
            .unwrap();
        let count = function
            .instructions
            .iter()
            .find_map(|i| match i {
                BytecodeInstruction::LoadConst { dst, constant: id } if id.index() == constant => {
                    Some(*dst)
                }
                _ => None,
            })
            .unwrap();
        let instruction = function
            .instructions
            .iter_mut()
            .rev()
            .find(|i| matches!(i, BytecodeInstruction::MakeArray { .. }))
            .unwrap();
        let BytecodeInstruction::MakeArray {
            dst,
            elements,
            element,
        } = instruction
        else {
            unreachable!()
        };
        *instruction = BytecodeInstruction::RepeatArray {
            dst: *dst,
            value: elements[0],
            element: element.clone(),
            count,
        };
        assert!(
            verify_program(&module, &context.snapshot(), &Default::default()).is_err(),
            "{value}"
        );
    }
}
