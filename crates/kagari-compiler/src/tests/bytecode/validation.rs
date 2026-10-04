use crate::tests::bytecode::*;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{ConstantOperand, NativeImportId},
    module::{FunctionRecord, RootSlotLayout},
    program::verify_program,
};

use kagari_contract::{effects::EffectSet, standard::RuntimePrimitive};

#[test]
fn rejects_function_fallthrough_before_loading() {
    for source in ["fn main() {}", "fn main() -> i32 { 42 }"] {
        let mut module = common::bytecode_ok(source);
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
            verify_program(&module),
            Err(BytecodeVerificationError::InvalidOperation {
                reason: "function falls through without a terminator",
                ..
            })
        ));
        assert!(KbcArtifact::from_program(module, ArtifactBuildOptions::default(),).is_err());
    }
}

#[test]
fn verifier_rejects_array_get_scalar_result_and_wrong_arity() {
    let module = common::bytecode_ok(
        "fn main() -> bool { val a = [7]; match a.get(a.len()) { None => true, Some(_) => false } }",
    );
    let get = NativeImportId::new(
        module.modules[module.root.index()]
            .native_imports
            .iter()
            .position(|import| {
                import
                    .binding
                    .path
                    .last()
                    .is_some_and(|part| part.name == "$foundation_list_get")
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
    function.metadata.registers[dst.index()] = ValueType::I32;
    function
        .metadata
        .roots
        .registers
        .retain(|register| *register != dst);
    assert!(matches!(
        verify_program(&scalar_result),
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
        verify_program(&wrong_arity),
        Err(BytecodeVerificationError::InvalidOperation {
            reason: "native call arity mismatch",
            ..
        })
    ));
}

#[test]
fn encoded_root_layout_must_cover_exact_heap_slots() {
    let module = common::bytecode_ok("fn main() -> i32 { val values = [7]; values[0] }");
    let function = &module.modules[module.root.index()].functions[0];
    assert!(
        !function.metadata.roots.locals.is_empty() || !function.metadata.roots.registers.is_empty()
    );
    verify_program(&module).unwrap();
    let artifact = KbcArtifact::from_program(module, ArtifactBuildOptions::default()).unwrap();
    for corruption in ["missing", "extra"] {
        let mut forged = artifact.clone();
        let roots = &mut forged.program.modules[forged.program.root.index()].functions[0]
            .metadata
            .roots;
        if corruption == "missing" {
            if !roots.registers.is_empty() {
                roots.registers.pop();
            } else {
                roots.locals.pop();
            }
        } else {
            roots.registers.push(Register::new(0));
        }
        assert!(matches!(
            verify_program(&forged.program),
            Err(BytecodeVerificationError::InvalidRootLayout { .. })
        ));
        let decoded = KbcArtifact::from_bytes(&forged.to_bytes().unwrap()).unwrap();
        assert!(matches!(
            decoded.validate_for_loader(&ArtifactCompatibility::default()),
            Err(ArtifactValidationError::Bytecode(
                BytecodeVerificationError::InvalidRootLayout { .. }
            ))
        ));
    }
}

#[test]
fn verifier_rejects_malformed_register_local_and_control_flow_bytecode() {
    let mut invalid_register = common::bytecode_ok("fn main() -> i32 { 1 }");
    invalid_register.modules[invalid_register.root.index()].functions[0].instructions[0] =
        BytecodeInstruction::LoadConst {
            dst: Register::new(999),
            constant: ConstantOperand::I32(1),
        };
    assert!(matches!(
        verify_program(&invalid_register),
        Err(BytecodeVerificationError::InvalidRegister { .. })
    ));
    assert_eq!(
        verify_program(&invalid_register).unwrap_err().code(),
        "KG_BYTECODE_INVALID_REGISTER"
    );

    let mut invalid_local = common::bytecode_ok("fn main() -> i32 { val value = 1; value }");
    invalid_local.modules[invalid_local.root.index()].functions[0].instructions[1] =
        BytecodeInstruction::StoreLocal {
            local: LocalSlot::new(999),
            src: Register::new(0),
        };
    assert!(matches!(
        verify_program(&invalid_local),
        Err(BytecodeVerificationError::InvalidLocal { .. })
    ));
    assert_eq!(
        verify_program(&invalid_local).unwrap_err().code(),
        "KG_BYTECODE_INVALID_LOCAL"
    );

    let mut invalid_jump = common::bytecode_ok("fn main() -> i32 { if true { 1 } else { 2 } }");
    invalid_jump.modules[invalid_jump.root.index()].functions[0]
        .metadata
        .control_flow_targets
        .push(JumpTarget::new(usize::MAX));
    assert!(matches!(
        verify_program(&invalid_jump),
        Err(BytecodeVerificationError::InvalidJumpTarget { .. })
    ));
    assert_eq!(
        verify_program(&invalid_jump).unwrap_err().code(),
        "KG_BYTECODE_INVALID_JUMP_TARGET"
    );
}

#[test]
fn verifier_rejects_type_inconsistent_bytecode() {
    let mut bytecode = common::bytecode_ok("fn main() -> i32 { 1 }");
    bytecode.modules[bytecode.root.index()].functions[0]
        .metadata
        .return_type = ValueType::Bool;
    bytecode.modules[bytecode.root.index()].function_table[0].return_type = ValueType::Bool;
    bytecode.modules[bytecode.root.index()]
        .types
        .push(ValueType::Bool);

    assert!(matches!(
        verify_program(&bytecode),
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
    let mut bytecode = common::bytecode_ok(
        r#"
fn main(value: Vec<i32>) -> usize {
    value.len()
}
"#,
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
        verify_program(&bytecode),
        Err(
            BytecodeVerificationError::RuntimePrimitiveSignatureMismatch {
                intrinsic: RuntimePrimitive::ValueCmp,
                reason: "invalid language primitive operands",
                ..
            }
        )
    ));

    let artifact = KbcArtifact::from_program(bytecode, ArtifactBuildOptions::default());
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
    let module = BytecodeModule {
        types: vec![
            ValueType::Unit,
            ValueType::Bool,
            ValueType::I32,
            ValueType::HeapObject,
        ],
        structures: {
            let program = common::bytecode_ok("struct Point { var x: i32 }");
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
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            identity: None,
            name: "write_bad_field".to_owned(),
            parameter_count: 0,
            local_count: 0,
            register_count: 2,
            metadata: FunctionMetadata {
                return_type: ValueType::Unit,
                registers: vec![ValueType::HeapObject, ValueType::Bool],
                roots: RootSlotLayout {
                    registers: vec![Register::new(0)],
                    ..Default::default()
                },
                effects: EffectSet::aggregate_write(),
                ..Default::default()
            },
            instructions: vec![
                BytecodeInstruction::WriteAggregateField {
                    base: Register::new(0),
                    field: FieldRef {
                        structure: StructId::new(0),
                        arguments: vec![],
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
            contract_fingerprint: 0,
            id: PathId::new(0),
            root_ty: ValueType::HostHandle,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "Actor.health".to_owned(),
        }],
        function_table: vec![FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: "read_missing_path".to_owned(),
            params: vec![ValueType::HostHandle],
            return_type: ValueType::I32,
            effects: EffectSet::path_read(),
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            identity: None,
            name: "read_missing_path".to_owned(),
            parameter_count: 1,
            local_count: 1,
            register_count: 2,
            metadata: FunctionMetadata {
                params: vec![ValueType::HostHandle],
                return_type: ValueType::I32,
                locals: vec![ValueType::HostHandle],
                registers: vec![ValueType::HostHandle, ValueType::I32],
                effects: EffectSet::path_read(),
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
            contract_fingerprint: 0,
            id: PathId::new(0),
            root_ty: ValueType::HostHandle,
            result_ty: ValueType::I32,
            read_only: true,
            debug_name: "Actor.id".to_owned(),
        }],
        function_table: vec![FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: "write_readonly_path".to_owned(),
            params: vec![ValueType::HostHandle, ValueType::I32],
            return_type: ValueType::Unit,
            effects: EffectSet::path_write(),
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            identity: None,
            name: "write_readonly_path".to_owned(),
            parameter_count: 2,
            local_count: 2,
            register_count: 2,
            metadata: FunctionMetadata {
                params: vec![ValueType::HostHandle, ValueType::I32],
                return_type: ValueType::Unit,
                locals: vec![ValueType::HostHandle, ValueType::I32],
                registers: vec![ValueType::HostHandle, ValueType::I32],
                effects: EffectSet::path_write(),
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
    let function = &mut bytecode.modules[bytecode.root.index()].functions[0];
    let mut point = function.metadata.debug.safe_debug_points[0].clone();
    point.instruction_offset = function.instructions.len();
    function.metadata.debug.safe_debug_points.push(point);

    assert!(matches!(
        verify_program(&bytecode),
        Err(BytecodeVerificationError::InvalidJumpTarget { .. })
    ));
}

#[test]
fn mapped_result_error_rejects_invalid_contracts_and_registers() {
    use kagari_contract::{
        scalar::BuiltinType, standard::surface::StandardEnum as StandardEnumKind, types::Ty,
    };
    let module = common::bytecode_ok(
        "fn main()->Result<i32,String>{val r:Result<i32,String> = Err(\"error\");Ok(r?)}",
    );
    verify_program(&module).unwrap();
    for mutation in 0..4 {
        let mut invalid = module.clone();
        let instruction = invalid.modules[invalid.root.index()]
            .functions
            .iter_mut()
            .flat_map(|f| &mut f.instructions)
            .find(|i| matches!(i, BytecodeInstruction::MapResultError { .. }))
            .unwrap();
        let BytecodeInstruction::MapResultError {
            original,
            error,
            ty,
            ..
        } = instruction
        else {
            unreachable!()
        };
        match mutation {
            0 => *ty = Ty::Builtin(BuiltinType::Bool),
            1 => {
                *ty = Ty::StandardEnum {
                    kind: StandardEnumKind::Result,
                    args: vec![],
                }
            }
            2 => *original = Register::new(usize::MAX),
            _ => *error = Register::new(usize::MAX),
        }
        assert!(verify_program(&invalid).is_err());
    }
}

#[test]
fn ranges_reject_forged_shapes_endpoints_and_bounds() {
    use kagari_common::range::RangeKind;
    use kagari_contract::{
        scalar::BuiltinType, standard::surface::StandardEnum as StandardEnumKind, types::Ty,
    };
    let module =
        common::bytecode_ok("fn main() { val range = 0usize..2usize; range.start_bound(); }");
    verify_program(&module).unwrap();
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
            3 => *ty = Ty::Range(Box::new(Ty::Builtin(BuiltinType::USize)), RangeKind::Full),
            4 => *start = Some(Register::new(usize::MAX)),
            _ => {
                *ty = Ty::Range(
                    Box::new(Ty::Builtin(BuiltinType::U64)),
                    RangeKind::Exclusive,
                )
            }
        }
        assert!(
            verify_program(&invalid).is_err(),
            "range mutation {mutation}"
        );
    }
    let root = module.root.index();
    let import = module.modules[root]
        .native_imports
        .iter()
        .position(|import| {
            import
                .binding
                .path
                .last()
                .is_some_and(|part| part.name == "$foundation_Range_start_bound")
        })
        .unwrap();
    for mutation in 0..4 {
        let mut invalid = module.clone();
        let contract = &mut invalid.modules[root].native_imports[import];
        match mutation {
            0 => contract.signature.result = Ty::Builtin(BuiltinType::Bool),
            1 => {
                contract.signature.result = Ty::StandardEnum {
                    kind: StandardEnumKind::Bound,
                    args: vec![],
                }
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
                args[0] = Register::new(usize::MAX);
            }
        }
        assert!(
            verify_program(&invalid).is_err(),
            "bound mutation {mutation}"
        );
    }
}

#[test]
fn forged_repetition_cannot_copy_shared_mutable_identities() {
    use kagari_bytecode::instruction::ConstantOperand;
    for value in [
        "Cell { value: 1 }",
        "(Cell { value: 1 }, 1)",
        "Some(Cell { value: 1 })",
    ] {
        let source = format!(
            "struct Cell {{ var value: i32 }} fn main() {{ val count = 2usize; val value = {value}; val array = [value, value]; }}"
        );
        let mut module = common::bytecode_ok(&source);
        verify_program(&module).unwrap();
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
                BytecodeInstruction::LoadConst {
                    dst,
                    constant: ConstantOperand::U64(2),
                } => Some(*dst),
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
        assert!(verify_program(&module).is_err(), "{value}");
    }
}
