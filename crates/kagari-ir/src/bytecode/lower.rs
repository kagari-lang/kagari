use std::collections::HashMap;

use kagari_hir::hir::FunctionId;

use crate::bytecode::instruction::{
    BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, FunctionRef, JumpTarget, LocalSlot,
    ModuleSlot, Register, RuntimeHelper, StructFieldInit, UnaryOp,
};
use crate::bytecode::module::{BytecodeFunction, BytecodeModule, BytecodeModuleSlot};
use crate::module::{
    function::{BasicBlock, IrFunction, IrModule},
    ids::{BlockId, LocalId, ModuleSlotId, TempId},
    instruction::{
        BinaryOp as IrBinaryOp, CallTarget as IrCallTarget, Constant, Instruction,
        RuntimeHelper as IrRuntimeHelper, Terminator, UnaryOp as IrUnaryOp,
    },
};

#[derive(Debug)]
pub enum BytecodeLoweringError {
    InvalidBranchTarget(BlockId),
}

pub fn lower_to_bytecode(ir: &IrModule) -> Result<BytecodeModule, BytecodeLoweringError> {
    let function_refs = ir
        .functions
        .iter()
        .enumerate()
        .map(|(index, function)| (function.hir_id, FunctionRef::new(index)))
        .collect::<HashMap<_, _>>();
    let functions = ir
        .functions
        .iter()
        .map(|function| lower_function(function, &function_refs))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BytecodeModule {
        module_init: ir
            .module_init
            .and_then(|id| function_refs.get(&id).copied()),
        module_slots: ir
            .module_slots
            .iter()
            .map(|slot| BytecodeModuleSlot {
                name: slot.name.clone(),
                mutable: slot.mutable,
            })
            .collect(),
        functions,
    })
}

fn lower_function(
    function: &IrFunction,
    function_refs: &HashMap<FunctionId, FunctionRef>,
) -> Result<BytecodeFunction, BytecodeLoweringError> {
    let block_offsets = compute_block_offsets(function);
    let mut instructions = Vec::with_capacity(
        function
            .blocks
            .iter()
            .map(|block| block.instructions.len() + usize::from(block.terminator.is_some()))
            .sum(),
    );

    for block in &function.blocks {
        lower_block(block, &block_offsets, function_refs, &mut instructions)?;
    }

    Ok(BytecodeFunction {
        id: *function_refs
            .get(&function.hir_id)
            .expect("bytecode lowering should have a function ref for every IR function"),
        name: function.name.clone(),
        parameter_count: function.params.len() as u16,
        register_count: function.temps.len() as u16,
        local_count: function.locals.len() as u16,
        instructions,
    })
}

fn compute_block_offsets(function: &IrFunction) -> HashMap<BlockId, JumpTarget> {
    let mut offsets = HashMap::new();
    let mut next_offset = 0usize;

    for (index, block) in function.blocks.iter().enumerate() {
        let block_id = BlockId::new(index);
        offsets.insert(block_id, JumpTarget::new(next_offset));
        next_offset += block.instructions.len();
        if block.terminator.is_some() {
            next_offset += 1;
        }
    }

    offsets
}

fn lower_block(
    block: &BasicBlock,
    block_offsets: &HashMap<BlockId, JumpTarget>,
    function_refs: &HashMap<FunctionId, FunctionRef>,
    out: &mut Vec<BytecodeInstruction>,
) -> Result<(), BytecodeLoweringError> {
    for instruction in &block.instructions {
        out.push(lower_instruction(instruction, function_refs));
    }

    if let Some(terminator) = &block.terminator {
        out.push(lower_terminator(terminator, block_offsets)?);
    }

    Ok(())
}

fn lower_instruction(
    instruction: &Instruction,
    function_refs: &HashMap<FunctionId, FunctionRef>,
) -> BytecodeInstruction {
    match instruction {
        Instruction::LoadConst { dst, constant } => BytecodeInstruction::LoadConst {
            dst: lower_temp(*dst),
            constant: lower_constant(constant),
        },
        Instruction::LoadLocal { dst, local } => BytecodeInstruction::LoadLocal {
            dst: lower_temp(*dst),
            local: lower_local(*local),
        },
        Instruction::LoadModule { dst, slot } => BytecodeInstruction::LoadModule {
            dst: lower_temp(*dst),
            slot: lower_module_slot(*slot),
        },
        Instruction::StoreLocal { local, src } => BytecodeInstruction::StoreLocal {
            local: lower_local(*local),
            src: lower_temp(*src),
        },
        Instruction::StoreModule { slot, src } => BytecodeInstruction::StoreModule {
            slot: lower_module_slot(*slot),
            src: lower_temp(*src),
        },
        Instruction::Move { dst, src } => BytecodeInstruction::Move {
            dst: lower_temp(*dst),
            src: lower_temp(*src),
        },
        Instruction::Unary { dst, op, operand } => BytecodeInstruction::Unary {
            dst: lower_temp(*dst),
            op: match op {
                IrUnaryOp::Neg => UnaryOp::Neg,
                IrUnaryOp::Not => UnaryOp::Not,
            },
            operand: lower_temp(*operand),
        },
        Instruction::Binary { dst, op, lhs, rhs } => BytecodeInstruction::Binary {
            dst: lower_temp(*dst),
            op: match op {
                IrBinaryOp::Add => BinaryOp::Add,
                IrBinaryOp::Sub => BinaryOp::Sub,
                IrBinaryOp::Mul => BinaryOp::Mul,
                IrBinaryOp::Div => BinaryOp::Div,
                IrBinaryOp::Eq => BinaryOp::Eq,
                IrBinaryOp::NotEq => BinaryOp::NotEq,
                IrBinaryOp::Lt => BinaryOp::Lt,
                IrBinaryOp::Gt => BinaryOp::Gt,
                IrBinaryOp::Le => BinaryOp::Le,
                IrBinaryOp::Ge => BinaryOp::Ge,
                IrBinaryOp::AndAnd | IrBinaryOp::OrOr => {
                    unreachable!(
                        "short-circuit ops should be lowered into branches before bytecode"
                    )
                }
            },
            lhs: lower_temp(*lhs),
            rhs: lower_temp(*rhs),
        },
        Instruction::Call { dst, callee, args } => BytecodeInstruction::Call {
            dst: dst.map(lower_temp),
            callee: match callee {
                IrCallTarget::Function(id) => CallTarget::Function(
                    *function_refs
                        .get(id)
                        .expect("bytecode lowering should resolve direct call targets"),
                ),
                IrCallTarget::Temp(temp) => CallTarget::Register(lower_temp(*temp)),
                IrCallTarget::RuntimeHelper(helper) => {
                    CallTarget::RuntimeHelper(lower_runtime_helper(helper))
                }
            },
            args: args.iter().map(|arg| lower_temp(*arg)).collect(),
        },
        Instruction::MakeTuple { dst, elements } => BytecodeInstruction::MakeTuple {
            dst: lower_temp(*dst),
            elements: elements
                .iter()
                .map(|element| lower_temp(*element))
                .collect(),
        },
        Instruction::MakeArray { dst, elements } => BytecodeInstruction::MakeArray {
            dst: lower_temp(*dst),
            elements: elements
                .iter()
                .map(|element| lower_temp(*element))
                .collect(),
        },
        Instruction::MakeStruct { dst, name, fields } => BytecodeInstruction::MakeStruct {
            dst: lower_temp(*dst),
            name: name.clone(),
            fields: fields
                .iter()
                .map(|field| StructFieldInit {
                    name: field.name.clone(),
                    value: lower_temp(field.value),
                })
                .collect(),
        },
        Instruction::ReadField { dst, base, name } => BytecodeInstruction::ReadField {
            dst: lower_temp(*dst),
            base: lower_temp(*base),
            name: name.clone(),
        },
        Instruction::ReadIndex { dst, base, index } => BytecodeInstruction::ReadIndex {
            dst: lower_temp(*dst),
            base: lower_temp(*base),
            index: lower_temp(*index),
        },
    }
}

fn lower_terminator(
    terminator: &Terminator,
    block_offsets: &HashMap<BlockId, JumpTarget>,
) -> Result<BytecodeInstruction, BytecodeLoweringError> {
    Ok(match terminator {
        Terminator::Return(value) => BytecodeInstruction::Return(value.map(lower_temp)),
        Terminator::Jump(target) => BytecodeInstruction::Jump {
            target: lower_jump(*target, block_offsets)?,
        },
        Terminator::Branch {
            cond,
            then_block,
            else_block,
        } => BytecodeInstruction::Branch {
            cond: lower_temp(*cond),
            then_target: lower_jump(*then_block, block_offsets)?,
            else_target: lower_jump(*else_block, block_offsets)?,
        },
        Terminator::Unreachable => BytecodeInstruction::Unreachable,
    })
}

fn lower_constant(constant: &Constant) -> ConstantOperand {
    match constant {
        Constant::Unit => ConstantOperand::Unit,
        Constant::Bool(value) => ConstantOperand::Bool(*value),
        Constant::I32(value) => ConstantOperand::I32(*value),
        Constant::F32(value) => ConstantOperand::F32(*value),
        Constant::Str(value) => ConstantOperand::Str(value.clone()),
    }
}

fn lower_runtime_helper(helper: &IrRuntimeHelper) -> RuntimeHelper {
    match helper {
        IrRuntimeHelper::HostFunction(symbol) => RuntimeHelper::HostFunction(symbol.clone()),
        IrRuntimeHelper::ReflectTypeOf => RuntimeHelper::ReflectTypeOf,
        IrRuntimeHelper::ReflectGetField(name) => RuntimeHelper::ReflectGetField(name.clone()),
        IrRuntimeHelper::ReflectSetField(name) => RuntimeHelper::ReflectSetField(name.clone()),
        IrRuntimeHelper::ReflectSetIndex => RuntimeHelper::ReflectSetIndex,
        IrRuntimeHelper::DynamicCall => RuntimeHelper::DynamicCall,
    }
}

fn lower_temp(temp: TempId) -> Register {
    Register::new(temp.index())
}

fn lower_local(local: LocalId) -> LocalSlot {
    LocalSlot::new(local.index())
}

fn lower_module_slot(slot: ModuleSlotId) -> ModuleSlot {
    ModuleSlot::new(slot.index())
}

fn lower_jump(
    block: BlockId,
    block_offsets: &HashMap<BlockId, JumpTarget>,
) -> Result<JumpTarget, BytecodeLoweringError> {
    block_offsets
        .get(&block)
        .copied()
        .ok_or(BytecodeLoweringError::InvalidBranchTarget(block))
}
