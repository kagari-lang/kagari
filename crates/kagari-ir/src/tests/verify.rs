use kagari_common::cancellation::CancellationToken;

use crate::{
    bytecode::{BytecodeInstruction, ConstantOperand, lower_to_bytecode},
    lower_to_ir,
    module::{
        BinaryOp, BlockId, CallTarget, Constant, EffectSet, Instruction, IrModule, IrTemp, IrValue,
        IrVerificationErrorKind as Error, LocalId, TempId, Terminator, ValueType,
        contracts::ContractError, ids::InstanceId, verify_ir,
    },
    tests::common,
};

fn raw(source: &str) -> IrModule {
    lower_to_ir(&common::analyze_ok(source), &Default::default())
        .unwrap()
        .into_unverified()
}

fn reject(module: IrModule) -> Error {
    verify_ir(module, &Default::default()).unwrap_err().kind
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
}

#[test]
fn temporary_annotations_cannot_forge_their_storage_type() {
    let mut module = raw("fn main() -> i32 { 7 }");
    if let Instruction::LoadConst { dst, .. } = &mut module.functions[0].blocks[0].instructions[0] {
        dst.ty = ValueType::Bool;
    }
    let error = verify_ir(module, &Default::default()).unwrap_err();
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
    verify_ir(valid.clone(), &Default::default()).unwrap();
    let mut module = valid;
    let branch = &mut module.functions[0].blocks[1];
    let index = branch
        .instructions
        .iter()
        .position(|i| matches!(i, Instruction::Move { .. }))
        .unwrap();
    branch.instructions.remove(index);
    branch.instruction_spans.remove(index);
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
    assert_eq!(reject(module), Error::UninitializedLocal(LocalId::new(0)));
}

#[test]
fn rejects_use_before_definition_within_a_block() {
    let mut module = raw("fn main() -> i32 { 7 }");
    module.functions[0].blocks[0].instructions[0] = Instruction::Move {
        dst: IrValue {
            temp: TempId::new(0),
            ty: ValueType::I32,
        },
        src: IrValue {
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
        IrTemp {
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
    let mut module = raw("fn main() -> i32 { 7 }");
    let mut entry = module.functions[0].blocks[0].clone();
    if let Instruction::LoadConst { constant, .. } = &mut entry.instructions[0] {
        *constant = Constant::I32(42);
    }
    module.functions[0].blocks.push(entry);
    module.functions[0].entry = BlockId::new(1);
    let checked = verify_ir(module, &Default::default()).unwrap();
    let bytecode = lower_to_bytecode(&checked).unwrap();
    assert!(matches!(
        bytecode.functions[0].instructions[0],
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
    for instruction in &mut bytecode.functions[0].instructions {
        if let BytecodeInstruction::Binary { op, .. } = instruction {
            *op = crate::bytecode::BinaryOp::Add;
        }
    }
    assert!(matches!(
        crate::bytecode::verify_module(&bytecode),
        Err(crate::bytecode::BytecodeVerificationError::InvalidOperation { .. })
    ));
}

#[test]
fn standard_intrinsic_contracts_apply_before_bytecode_emission() {
    let mut module = raw("fn main() -> usize { \"text\".len_bytes() }");
    for instruction in &mut module.functions[0].blocks[0].instructions {
        if let Instruction::Call { args, .. } = instruction {
            args.clear();
        }
    }
    assert!(matches!(
        reject(module),
        Error::Contract(ContractError::Intrinsic {
            reason: "arity mismatch",
            ..
        })
    ));
}

#[test]
fn readonly_path_modification_is_rejected_before_effects_or_flow() {
    let mut module = raw("fn main() -> i32 { 7 }");
    let value = IrValue {
        temp: TempId::new(0),
        ty: ValueType::I32,
    };
    let block = &mut module.functions[0].blocks[0];
    block.instructions.push(Instruction::ModifyPath {
        dst: Some(value),
        root_or_view: value,
        path: crate::module::PathRef {
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
    assert_eq!(reject(module), Error::ReadOnlyPath);
}

#[test]
fn verification_bounds_the_flow_matrix_before_allocating_it() {
    let mut module = raw("fn main() {}");
    let function = &mut module.functions[0];
    function.temps.resize(
        u16::MAX as usize,
        IrTemp {
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
        verify_ir(module, &cancel).unwrap_err().kind,
        Error::Cancelled
    );
}
