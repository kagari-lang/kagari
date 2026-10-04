use kagari_bytecode::{
    artifact::ArtifactFingerprint,
    instruction::{BytecodeInstruction, ConstantOperand},
    verifier::BytecodeVerificationError,
};

use {
    kagari_bytecode::{
        instruction::{BinaryOp as KagaribytecodeBinaryOp, EnumId},
        program::{ModuleRef, verify_program},
    },
    kagari_mir::instruction::PathRef,
};

use kagari_common::{cancellation::CancellationToken, collection::CollectionAccess};
use kagari_hir::types::abi::lower_type;

use crate::{source::lower::lower_to_mir, tests::common};
use {
    kagari_abi::representation::ValueType,
    kagari_contract::{contracts::ContractError, effects::EffectSet, operations::BinaryOp},
};

use kagari_mir::{
    function::{MirModule, MirTemp},
    ids::{BlockId, InstanceId, LocalId, TempId},
    instruction::{CallTarget, Constant, Instruction, MirValue, Terminator},
    verify::{MirVerificationErrorKind as Error, verify_mir},
};

fn raw(source: &str) -> MirModule {
    lower_to_mir(&common::analyze_ok(source), &Default::default())
        .unwrap()
        .into_unverified()
}

#[test]
fn integer_constants_must_match_semantic_range_and_representation() {
    for (source, invalid) in [
        ("fn main() -> i8 { 1i8 }", ConstantOperand::I32(128)),
        ("fn main() -> u8 { 1u8 }", ConstantOperand::I64(256)),
        ("fn main() -> u8 { 1u8 }", ConstantOperand::I64(-1)),
        ("fn main() -> usize { 1usize }", ConstantOperand::I64(1)),
        ("fn main() -> u64 { 1u64 }", ConstantOperand::I64(1)),
    ] {
        let mut bytecode = common::bytecode_ok(source);
        let mut replaced = false;
        for function in &mut bytecode.modules[bytecode.root.index()].functions {
            for instruction in &mut function.instructions {
                if let BytecodeInstruction::LoadConst { constant, .. } = instruction {
                    *constant = invalid.clone();
                    replaced = true;
                }
            }
        }
        assert!(replaced, "{source}");
        assert!(verify_program(&bytecode).is_err(), "{source}");
    }
}

#[test]
fn unused_public_enum_templates_validate_parameter_ownership_and_position() {
    let original = common::bytecode_ok("pub enum Packet<T> { Data(T) } fn main() {}");
    assert!(
        original.modules[original.root.index()]
            .enumerations
            .iter()
            .all(|layout| layout.declaration.module
                != original.modules[original.root.index()].identity)
    );
    for foreign_owner in [false, true] {
        let mut bytecode = original.clone();
        let kagari_contract::types::PublicItem::Type(template) =
            &mut bytecode.modules[bytecode.root.index()].public_items[0]
        else {
            unreachable!()
        };
        let kagari_contract::types::Ty::Parameter { owner, position } =
            &mut template.variants[0].payload[0]
        else {
            unreachable!()
        };
        if foreign_owner {
            owner.module.path.push("foreign".into());
        } else {
            *position = 1;
        }
        assert!(verify_program(&bytecode).is_err());
    }
}

#[test]
fn layout_templates_require_scoped_instruction_arguments() {
    for source in [
        "struct Cell<T> { val value: T } fn main() { val c = Cell { value: 1 }; }",
        "enum Packet<T> { Data(T) } fn main() { val p = Packet::Data(1); }",
    ] {
        let mut bytecode = common::bytecode_ok(source);
        if let Some(layout) = bytecode.modules[bytecode.root.index()]
            .structures
            .first_mut()
        {
            layout.arguments[0] = kagari_contract::types::Ty::Parameter {
                owner: layout.declaration.clone(),
                position: 0,
            };
        } else {
            let member = &mut bytecode.modules[bytecode.root.index()];
            let layout = member
                .enumerations
                .iter_mut()
                .find(|layout| layout.declaration.module == member.identity)
                .unwrap();
            layout.arguments[0] = kagari_contract::types::Ty::Parameter {
                owner: layout.declaration.clone(),
                position: 0,
            };
        }
        let member = &mut bytecode.modules[bytecode.root.index()];
        let argument = member
            .structures
            .first()
            .map(|layout| layout.arguments[0].clone())
            .or_else(|| {
                member
                    .enumerations
                    .iter()
                    .find(|layout| layout.declaration.module == member.identity)
                    .map(|layout| layout.arguments[0].clone())
            })
            .unwrap();
        let mut changed = false;
        for instruction in member
            .functions
            .iter_mut()
            .flat_map(|function| &mut function.instructions)
        {
            match instruction {
                BytecodeInstruction::MakeStruct { arguments, .. }
                | BytecodeInstruction::MakeEnum { arguments, .. }
                    if !arguments.is_empty() =>
                {
                    arguments[0] = argument.clone();
                    changed = true;
                }
                _ => {}
            }
        }
        assert!(changed);
        assert!(verify_program(&bytecode).is_err());
    }
}

#[test]
fn applied_nominal_abi_preserves_arguments_and_cannot_bind_to_a_bare_layout() {
    use kagari_contract::{
        scalar::BuiltinType,
        types::{NominalTy, Ty},
    };
    use kagari_hir::types::{NominalType, TypeId};
    let source =
        "struct Point {} enum Event { Data(Point) } fn main() -> Event { Event::Data(Point {}) }";
    let mut module = raw(source);
    let enumeration = module
        .enumerations
        .iter()
        .position(|layout| layout.declaration.module == module.identity)
        .unwrap();
    let declaration = module.structures[0].declaration.clone();
    let nominal = NominalType {
        associated_types: Default::default(),
        declaration: declaration.clone(),
        arguments: vec![TypeId::Array(
            Box::new(TypeId::Builtin(BuiltinType::I32)),
            CollectionAccess::Mutable,
        )],
    };
    let encoded = lower_type(&TypeId::Struct(nominal));
    let expected = Ty::Struct(NominalTy {
        associated_types: Default::default(),
        declaration,
        arguments: vec![Ty::Array(
            Box::new(Ty::Builtin(BuiltinType::I32)),
            CollectionAccess::Mutable,
        )],
    });
    assert_eq!(encoded, expected);
    let bytes = bincode::serialize(&encoded).unwrap();
    assert_eq!(bincode::deserialize::<Ty>(&bytes).unwrap(), encoded);
    let bare = &module.enumerations[enumeration].variants[0].payload[0];
    assert_ne!(
        ArtifactFingerprint::of_serialized(&encoded),
        ArtifactFingerprint::of_serialized(bare)
    );
    module.enumerations[enumeration].variants[0].payload[0] = encoded.clone();
    assert_eq!(reject(module), Error::InvalidEnumLayout);
    let mut bytecode = crate::tests::common::bytecode_ok(source);
    let member = &mut bytecode.modules[bytecode.root.index()];
    member
        .enumerations
        .iter_mut()
        .find(|layout| layout.declaration.module == member.identity)
        .unwrap()
        .variants[0]
        .payload[0] = encoded;
    assert_eq!(
        verify_program(&bytecode).unwrap_err(),
        BytecodeVerificationError::InvalidEnumLayout
    );
}

#[test]
fn enum_layouts_and_constructor_operands_are_validated_before_execution() {
    let source = "enum Event { Data(i32) } fn main() -> Event { Event::Data(7) }";
    let mut public = crate::tests::common::bytecode_ok(&format!("pub {source}"));
    let kagari_contract::types::PublicItem::Type(ty) =
        &mut public.modules[public.root.index()].public_items[0]
    else {
        panic!("enum ABI")
    };
    ty.variants[0].payload.clear();
    assert_eq!(
        verify_program(&public).unwrap_err(),
        BytecodeVerificationError::InvalidEnumLayout
    );
    let mut module = raw(source);
    let enumeration = module
        .enumerations
        .iter()
        .position(|layout| layout.declaration.module == module.identity)
        .unwrap();
    module.enumerations[enumeration].variants[0]
        .declaration
        .path[0]
        .name = "Other".into();
    assert_eq!(reject(module), Error::InvalidEnumLayout);
    let mut module = raw(source);
    let mut absent = module.enumerations[enumeration].declaration.clone();
    absent.path[0].name = "Absent".into();
    module.enumerations[enumeration].variants[0].payload[0] =
        kagari_contract::types::Ty::Enum(kagari_contract::types::NominalTy {
            associated_types: Default::default(),
            declaration: absent,
            arguments: Vec::new(),
        });
    assert_eq!(reject(module), Error::InvalidEnumLayout);
    let mut module = raw(source);
    for instruction in module
        .functions
        .iter_mut()
        .flat_map(|f| &mut f.blocks)
        .flat_map(|b| &mut b.instructions)
    {
        if let Instruction::MakeEnum { variant, .. } = instruction {
            *variant = 99;
        }
    }
    assert_eq!(reject(module), Error::InvalidEnumInitializer);
    let good = crate::tests::common::bytecode_ok(source);
    for mode in 0..3 {
        let mut bad = good.clone();
        for instruction in &mut bad.modules[bad.root.index()].functions[0].instructions {
            if let BytecodeInstruction::MakeEnum {
                enumeration,
                variant,
                fields,
                ..
            } = instruction
            {
                match mode {
                    0 => *enumeration = EnumId::new(99),
                    1 => *variant = 99,
                    _ => fields.clear(),
                }
            }
        }
        assert!(verify_program(&bad).is_err());
    }
    let mut second = good.modules[good.root.index()].clone();
    second.identity = kagari_common::identity::ModuleIdentity::single_file("second.kgr");
    for function in &mut second.functions {
        function.identity.as_mut().unwrap().declaration.module = second.identity.clone();
    }
    for record in &mut second.function_table {
        record.identity.as_mut().unwrap().declaration.module = second.identity.clone();
    }
    second.dependencies.push(good.root);
    let mut program = good;
    program.root = ModuleRef::new(program.modules.len());
    program.modules.push(second);
    verify_program(&program).unwrap();
    program.modules[program.root.index()].enumerations[enumeration].variants[0]
        .payload
        .clear();
    assert_eq!(
        verify_program(&program).unwrap_err(),
        BytecodeVerificationError::InvalidEnumLayout
    );
}

#[test]
fn conflicting_host_contracts_cannot_be_hidden_by_import_interning() {
    let mut module = raw(r#"fn main() { print("one"); print("two"); }"#);
    let mut calls = module
        .functions
        .iter_mut()
        .flat_map(|f| &mut f.blocks)
        .flat_map(|b| &mut b.instructions)
        .filter_map(|i| {
            if let Instruction::Call {
                callee: CallTarget::Native(import),
                ..
            } = i
            {
                import.host.as_mut()
            } else {
                None
            }
        });
    calls.next().unwrap();
    calls.next().unwrap().effects.may_mutate_host_state = false;
    assert!(matches!(reject(module), Error::InvalidHostInterface));
}

fn reject(module: MirModule) -> Error {
    verify_mir(module, &Default::default()).unwrap_err().kind
}

#[test]
fn layouts_reject_duplicate_owners_and_foreign_declarations() {
    let source = "struct P { var value: i32 } fn main() -> i32 { P { value: 42 }.value }";
    let mut module = raw(source);
    module.structures.push(module.structures[0].clone());
    assert_eq!(reject(module), Error::InvalidStructLayout);
    let mut module = raw(source);
    module.structures[0].fields[0]
        .declaration
        .module
        .path
        .push("other".into());
    assert_eq!(reject(module), Error::InvalidStructLayout);
    let mut module = raw(source);
    let duplicate = module.structures[0].fields[0].clone();
    module.structures[0].fields.push(duplicate);
    assert_eq!(reject(module), Error::InvalidStructLayout);
}

#[test]
fn struct_initializers_require_every_slot_once_and_the_declared_representation() {
    let source = "struct P { var number: i32, val flag: bool } fn main() -> P { P { flag: true, number: 42 } }";
    for invalid in [0, 1, 2] {
        let mut module = raw(source);
        for instruction in module
            .functions
            .iter_mut()
            .flat_map(|f| &mut f.blocks)
            .flat_map(|b| &mut b.instructions)
        {
            if let Instruction::MakeStruct { fields, .. } = instruction {
                match invalid {
                    0 => {
                        fields.pop();
                    }
                    1 => fields[1].slot = fields[0].slot,
                    _ => fields[1].slot = usize::MAX,
                }
            }
        }
        assert_eq!(reject(module), Error::InvalidStructInitializer);
    }
    let mut module = raw(source);
    for instruction in module
        .functions
        .iter_mut()
        .flat_map(|f| &mut f.blocks)
        .flat_map(|b| &mut b.instructions)
    {
        if let Instruction::MakeStruct { fields, .. } = instruction {
            for field in fields {
                field.slot = 1 - field.slot;
            }
        }
    }
    assert!(matches!(
        reject(module),
        Error::Contract(ContractError::TypeMismatch {
            context: "struct field initializer",
            ..
        })
    ));
}

#[test]
fn field_operands_require_an_existing_owner_slot_type_and_write_permission() {
    let source = "struct P { var number: i32, val flag: bool } fn main() -> i32 { val p = P { number: 1, flag: true }; p.number = 42; p.number }";
    let mut module = raw(source);
    module.structures[0].fields[0].mutable = false;
    assert_eq!(reject(module), Error::ReadOnlyField);
    let mut module = raw(source);
    for instruction in module
        .functions
        .iter_mut()
        .flat_map(|f| &mut f.blocks)
        .flat_map(|b| &mut b.instructions)
    {
        if let Instruction::ReadAggregateField { field, .. } = instruction {
            field.slot = 999;
        }
    }
    assert_eq!(reject(module), Error::InvalidField);
    let mut module = raw(source);
    for instruction in module
        .functions
        .iter_mut()
        .flat_map(|f| &mut f.blocks)
        .flat_map(|b| &mut b.instructions)
    {
        if let Instruction::ReadAggregateField { field, .. } = instruction {
            field.owner.declaration.module.path.push("different".into());
        }
    }
    assert_eq!(reject(module), Error::InvalidField);
    let mut module = raw(source);
    for instruction in module
        .functions
        .iter_mut()
        .flat_map(|f| &mut f.blocks)
        .flat_map(|b| &mut b.instructions)
    {
        if let Instruction::ReadAggregateField { field, .. } = instruction {
            field.slot = 1;
        }
    }
    assert!(matches!(
        reject(module),
        Error::Contract(ContractError::TypeMismatch {
            context: "field read",
            ..
        })
    ));
}

#[test]
fn bytecode_initializers_use_layout_order_after_source_order_evaluation() {
    let input = common::program_ok(
        "struct P { val first: i32, val second: bool } fn main() -> P { P { second: true, first: 42 } }",
    );
    let module = lower_to_mir(input.root(), &Default::default()).unwrap();
    let bytecode = common::bytecode_with_edited_root(&input, &module);
    let (structure, fields) = bytecode.modules[bytecode.root.index()]
        .functions
        .iter()
        .flat_map(|f| &f.instructions)
        .find_map(|instruction| {
            if let BytecodeInstruction::MakeStruct {
                structure, fields, ..
            } = instruction
            {
                Some((structure, fields))
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        bytecode.modules[bytecode.root.index()].structures[structure.index()]
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    let registers = &bytecode.modules[bytecode.root.index()].functions[0]
        .metadata
        .registers;
    assert_eq!(
        fields
            .iter()
            .map(|register| registers[register.index()])
            .collect::<Vec<_>>(),
        [ValueType::I32, ValueType::Bool]
    );
}

#[test]
fn rejects_missing_call_and_duplicate_instance_identity_before_bytecode() {
    let mut module = raw("fn f(x: i32) -> i32 { x } fn main() -> i32 { f(7) }");
    for instruction in &mut module.functions[1].blocks[0].instructions {
        if let Instruction::Call { callee, .. } = instruction {
            *callee = CallTarget::Function(InstanceId::new(999));
        }
    }
    assert_eq!(reject(module), Error::InvalidCall(InstanceId::new(999)));
    let mut module = raw("fn a() {} fn b() {}");
    module.functions[1].instance = module.functions[0].instance.clone();
    assert_eq!(reject(module), Error::InvalidInstance);
    let mut module = raw("fn a() {} fn b() {}");
    module.functions[1].id = module.functions[0].id;
    assert_eq!(reject(module), Error::InvalidInstance);
}

#[test]
fn checks_call_arity_and_return_contracts() {
    let mut module = raw("fn f(x: i32) -> i32 { x } fn main() -> i32 { f(7) }");
    for instruction in &mut module.functions[1].blocks[0].instructions {
        if let Instruction::Call { args, .. } = instruction {
            args.clear();
        }
    }
    assert_eq!(
        reject(module),
        Error::CallArity {
            expected: 1,
            found: 0
        }
    );
    let mut module = raw("fn main() -> i32 { 7 }");
    module.functions[0].blocks[0].terminator = Some(Terminator::Return(None));
    assert!(matches!(
        reject(module),
        Error::Contract(ContractError::TypeMismatch {
            context: "return value",
            ..
        })
    ));
}

#[test]
fn checks_block_termination_targets_and_debug_alignment() {
    let mut module = raw("fn main() {}");
    module.functions[0].blocks[0].terminator = None;
    assert_eq!(reject(module), Error::MissingTerminator);
    let mut module = raw("fn main() {}");
    module.functions[0].blocks[0].terminator = Some(Terminator::Jump(BlockId::new(9)));
    assert_eq!(reject(module), Error::InvalidBlock(BlockId::new(9)));
    let mut module = raw("fn main() {}");
    module.functions[0].blocks[0].instruction_spans.clear();
    assert_eq!(reject(module), Error::InvalidDebugMetadata);
    let mut module = raw("fn main() -> i32 { val value = 1; value }");
    module.functions[0].blocks[0].instruction_scopes.clear();
    assert_eq!(reject(module), Error::InvalidDebugMetadata);
    let mut module = raw("fn main() -> i32 { val value = 1; value }");
    module.functions[0].debug.lexical_scopes[1].parent = Some(99);
    assert_eq!(reject(module), Error::InvalidDebugMetadata);
}

#[test]
fn temporary_annotations_cannot_forge_their_storage_type() {
    let mut module = raw("fn main() -> i32 { 7 }");
    if let Instruction::LoadConst { dst, .. } = &mut module.functions[0].blocks[0].instructions[0] {
        dst.ty = ValueType::Bool;
    }
    let error = verify_mir(module, &Default::default()).unwrap_err();
    assert_eq!(error.instruction, Some(0));
    assert!(error.span.is_some());
    assert!(matches!(
        error.kind,
        Error::Contract(ContractError::TypeMismatch {
            context: "temporary annotation",
            ..
        })
    ));
}

#[test]
fn requires_initialization_on_every_predecessor() {
    let source = "fn choose(flag: bool) -> i32 { if flag { 1 } else { 2 } }";
    let valid = raw(source);
    verify_mir(valid.clone(), &Default::default()).unwrap();
    let mut module = valid;
    let branch = &mut module.functions[0].blocks[1];
    let index = branch
        .instructions
        .iter()
        .position(|i| matches!(i, Instruction::Move { .. }))
        .unwrap();
    branch.instructions.remove(index);
    branch.instruction_spans.remove(index);
    branch.instruction_scopes.remove(index);
    assert!(matches!(reject(module), Error::UninitializedTemp(_)));

    let mut module = raw("fn main() -> i32 { val x = 7; x }");
    let block = &mut module.functions[0].blocks[0];
    let index = block
        .instructions
        .iter()
        .position(|i| matches!(i, Instruction::StoreLocal { .. }))
        .unwrap();
    block.instructions.remove(index);
    block.instruction_spans.remove(index);
    block.instruction_scopes.remove(index);
    assert_eq!(reject(module), Error::UninitializedLocal(LocalId::new(0)));
}

#[test]
fn a_loop_backedge_does_not_initialize_the_first_iteration() {
    let mut module = raw("fn main() -> i32 { var n = 0; while n < 3 { n += 1; } n }");
    let entry = &mut module.functions[0].blocks[0];
    let index = entry
        .instructions
        .iter()
        .position(|i| matches!(i, Instruction::StoreLocal { .. }))
        .unwrap();
    entry.instructions.remove(index);
    entry.instruction_spans.remove(index);
    entry.instruction_scopes.remove(index);
    assert_eq!(reject(module), Error::UninitializedLocal(LocalId::new(0)));
}

#[test]
fn rejects_use_before_definition_within_a_block() {
    let mut module = raw("fn main() -> i32 { 7 }");
    module.functions[0].blocks[0].instructions[0] = Instruction::Move {
        dst: MirValue {
            temp: TempId::new(0),
            ty: ValueType::I32,
        },
        src: MirValue {
            temp: TempId::new(0),
            ty: ValueType::I32,
        },
    };
    assert_eq!(reject(module), Error::UninitializedTemp(TempId::new(0)));
}

#[test]
fn validates_effects_parameter_layout_and_encoding_limits() {
    let mut module = raw("fn main() -> i32 { 1 + 2 }");
    module.functions[0].effects = EffectSet::default();
    assert_eq!(reject(module), Error::MissingEffects);
    let mut module = raw("fn f(x: i32, y: i32) -> i32 { x }");
    module.functions[0].params[0].local = LocalId::new(1);
    assert_eq!(reject(module), Error::InvalidParameterLayout);
    let mut module = raw("fn main() {}");
    module.functions[0].temps.resize(
        usize::from(u16::MAX) + 1,
        MirTemp {
            ty: ValueType::Unit,
        },
    );
    assert_eq!(
        reject(module),
        Error::Limit {
            resource: "temporaries",
            limit: u16::MAX as usize
        }
    );
}

#[test]
fn emits_the_declared_entry_block_first() {
    let input = common::program_ok("fn main() -> i32 { 7 }");
    let mut module = lower_to_mir(input.root(), &Default::default())
        .unwrap()
        .into_unverified();
    let mut entry = module.functions[0].blocks[0].clone();
    if let Instruction::LoadConst { constant, .. } = &mut entry.instructions[0] {
        *constant = Constant::I32(42);
    }
    module.functions[0].blocks.push(entry);
    module.functions[0].entry = BlockId::new(1);
    let checked = verify_mir(module, &Default::default()).unwrap();
    let bytecode = common::bytecode_with_edited_root(&input, &checked);
    assert!(matches!(
        bytecode.modules[bytecode.root.index()].functions[0].instructions[0],
        BytecodeInstruction::LoadConst {
            constant: ConstantOperand::I32(42),
            ..
        }
    ));
}

#[test]
fn ir_and_bytecode_share_numeric_operation_contracts() {
    let mut module = raw("fn main() -> bool { true == false }");
    for instruction in &mut module.functions[0].blocks[0].instructions {
        if let Instruction::Binary { op, .. } = instruction {
            *op = BinaryOp::Add;
        }
    }
    assert!(matches!(
        reject(module),
        Error::Contract(ContractError::InvalidOperation { .. })
    ));
    let mut bytecode = common::bytecode_ok("fn main() -> bool { true == false }");
    for instruction in &mut bytecode.modules[bytecode.root.index()].functions[0].instructions {
        if let BytecodeInstruction::Binary { op, .. } = instruction {
            *op = KagaribytecodeBinaryOp::Add;
        }
    }
    assert!(matches!(
        verify_program(&bytecode),
        Err(BytecodeVerificationError::InvalidOperation { .. })
    ));
}

#[test]
fn native_contracts_apply_before_bytecode_emission() {
    let mut module = raw("fn main(value: Vec<i32>) { value.push(1); }");
    for instruction in &mut module.functions[0].blocks[0].instructions {
        if let Instruction::Call { args, .. } = instruction {
            args.clear();
        }
    }
    assert!(matches!(
        reject(module),
        Error::Contract(ContractError::InvalidOperation {
            reason: "native call arity mismatch",
        })
    ));
}

#[test]
fn readonly_path_modification_is_rejected_before_effects_or_flow() {
    let mut module = raw("fn main() -> i32 { 7 }");
    let value = MirValue {
        temp: TempId::new(0),
        ty: ValueType::I32,
    };
    let block = &mut module.functions[0].blocks[0];
    block.instructions.push(Instruction::ModifyPath {
        dst: Some(value),
        root_or_view: value,
        path: PathRef {
            declaration: None,
            contract_fingerprint: 0,
            root_ty: ValueType::I32,
            result_ty: ValueType::I32,
            read_only: true,
            debug_name: "readonly".into(),
        },
        dynamic_args: Default::default(),
        op: BinaryOp::Add,
        value,
    });
    block.instruction_spans.push(Default::default());
    block
        .instruction_scopes
        .push(block.terminator_scope.unwrap());
    assert_eq!(reject(module), Error::ReadOnlyPath);
}

#[test]
fn verification_bounds_the_flow_matrix_before_allocating_it() {
    let mut module = raw("fn main() {}");
    let function = &mut module.functions[0];
    function.temps.resize(
        u16::MAX as usize,
        MirTemp {
            ty: ValueType::Unit,
        },
    );
    let block = function.blocks[0].clone();
    function.blocks.resize(8193, block);
    assert_eq!(
        reject(module),
        Error::Limit {
            resource: "definite-initialization state bytes",
            limit: 64 * 1024 * 1024
        }
    );
}

#[test]
fn verification_observes_cancellation_even_for_empty_modules() {
    let module = raw("");
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        verify_mir(module, &cancel).unwrap_err().kind,
        Error::Cancelled
    );
}

#[test]
fn unused_public_aggregate_templates_reject_malformed_member_shapes() {
    use kagari_contract::{
        scalar::BuiltinType,
        types::{FieldDef, PublicItem, Ty, TypeDefKind, VariantDef},
    };
    for source in [
        "pub struct Box<T> { val value: T } fn main() {}",
        "pub enum Box<T> { Value(T) } fn main() {}",
    ] {
        let original = raw(source);
        assert!(
            original
                .structures
                .iter()
                .all(|layout| layout.declaration.module != original.identity)
                && original
                    .enumerations
                    .iter()
                    .all(|layout| layout.declaration.module != original.identity)
        );
        let bytecode = common::bytecode_ok(source);
        for mutation in 0..5 {
            let mut invalid = original.clone();
            let PublicItem::Type(template) = &mut invalid.abi.public_items[0] else {
                unreachable!()
            };
            match mutation {
                0 => template.name.clear(),
                1 if template.kind == TypeDefKind::Struct => template.fields[0].name.clear(),
                1 => template.variants[0].name.clear(),
                2 if template.kind == TypeDefKind::Struct => {
                    template.fields.push(template.fields[0].clone())
                }
                2 => template.variants.push(template.variants[0].clone()),
                3 if template.kind == TypeDefKind::Struct => template.variants.push(VariantDef {
                    name: "Unexpected".into(),
                    payload: vec![],
                }),
                3 => template.fields.push(FieldDef {
                    name: "unexpected".into(),
                    ty: Ty::Builtin(BuiltinType::I32),
                    mutable: false,
                }),
                _ => {
                    let duplicate = invalid.abi.public_items[0].clone();
                    invalid.abi.public_items.push(duplicate);
                }
            }
            let mut invalid_bytecode = bytecode.clone();
            invalid_bytecode.modules[invalid_bytecode.root.index()].public_items =
                invalid.abi.public_items.clone();
            assert_eq!(
                verify_mir(invalid, &Default::default()).unwrap_err().kind,
                Error::InvalidPublicAbi
            );
            assert_eq!(
                verify_program(&invalid_bytecode),
                Err(BytecodeVerificationError::InvalidPublicAbi)
            );
        }
    }
}

#[test]
fn public_layout_matching_observes_cancellation_including_empty_inputs() {
    use kagari_contract::layout::{enum_abi_matches, struct_abi_matches};
    let mut source = String::new();
    for index in 0..128 {
        source.push_str(&format!(
            "pub struct S{index}<T> {{ val value: T }} pub enum E{index}<T> {{ Data(T) }} "
        ));
    }
    source.push_str("fn main() { val s = S127 { value: 42 }; val e = E127::Data(true); }");
    let module = raw(&source);
    let cancel = CancellationToken::default();
    assert_eq!(
        struct_abi_matches(
            &module.structures,
            &module.identity,
            &module.abi.public_items,
            &cancel
        ),
        Ok(true)
    );
    assert_eq!(
        enum_abi_matches(
            &module.enumerations,
            &module.identity,
            &module.abi.public_items,
            &cancel
        ),
        Ok(true)
    );
    cancel.cancel();
    for empty in [false, true] {
        let items = if empty {
            &[][..]
        } else {
            &module.abi.public_items[..]
        };
        assert_eq!(
            struct_abi_matches(
                if empty { &[] } else { &module.structures },
                &module.identity,
                items,
                &cancel
            ),
            Err(kagari_common::cancellation::Cancelled)
        );
        assert_eq!(
            enum_abi_matches(
                if empty { &[] } else { &module.enumerations },
                &module.identity,
                items,
                &cancel
            ),
            Err(kagari_common::cancellation::Cancelled)
        );
    }
    assert_eq!(
        verify_mir(module, &cancel).unwrap_err().kind,
        Error::Cancelled
    );
}
