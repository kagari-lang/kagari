use std::collections::HashMap;

use kagari_hir::builtin::BuiltinMethod;
use kagari_hir::hir::FunctionId;

use crate::bytecode::instruction::{
    BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, FieldId, FunctionRef, JumpTarget,
    LocalSlot, ModuleSlot, PathId, Register, RuntimeHelper, StructFieldInit, UnaryOp,
};
use crate::bytecode::module::{
    BytecodeFunction, BytecodeModule, BytecodeModuleSlot, FieldRecord, FunctionMetadata,
    FunctionRecord, PathRecord,
};
use crate::bytecode::verify_module;
use crate::module::{
    ValueType,
    function::{BasicBlock, IrFunction, IrModule},
    ids::{BlockId, LocalId, ModuleSlotId, TempId},
    instruction::{
        AggregateFieldRef, BinaryOp as IrBinaryOp, CallTarget as IrCallTarget, Constant,
        Instruction, IrValue, PathRef, RuntimeHelper as IrRuntimeHelper, Terminator,
        UnaryOp as IrUnaryOp,
    },
};

#[derive(Debug)]
pub enum BytecodeLoweringError {
    InvalidBranchTarget(BlockId),
    Verification(crate::bytecode::BytecodeVerificationError),
}

pub fn lower_to_bytecode(ir: &IrModule) -> Result<BytecodeModule, BytecodeLoweringError> {
    let mut context = BytecodeLoweringContext::default();
    let function_refs = ir
        .functions
        .iter()
        .enumerate()
        .map(|(index, function)| (function.hir_id, FunctionRef::new(index)))
        .collect::<HashMap<_, _>>();
    let functions = ir
        .functions
        .iter()
        .map(|function| lower_function(function, &function_refs, &mut context))
        .collect::<Result<Vec<_>, _>>()?;
    let mut module = BytecodeModule {
        module_init: ir
            .module_init
            .and_then(|id| function_refs.get(&id).copied()),
        module_slots: ir
            .module_slots
            .iter()
            .map(|slot| BytecodeModuleSlot {
                name: slot.name.clone(),
                ty: slot.ty,
                mutable: slot.mutable,
            })
            .collect(),
        constants: Vec::new(),
        types: Vec::new(),
        fields: context.fields,
        paths: context.paths,
        function_table: Vec::new(),
        public_items: Vec::new(),
        functions,
    };
    module.constants = collect_constant_pool(&module.functions);
    module.types = collect_type_table(&module);
    module.function_table = collect_function_table(&module.functions);
    verify_module(&module).map_err(BytecodeLoweringError::Verification)?;
    Ok(module)
}

#[derive(Debug, Default)]
struct BytecodeLoweringContext {
    fields: Vec<FieldRecord>,
    paths: Vec<PathRecord>,
}

impl BytecodeLoweringContext {
    fn field_id(&mut self, field: &AggregateFieldRef, ty: ValueType) -> FieldId {
        if let Some(record) = self.fields.iter().find(|record| {
            record.owner == field.owner && record.name == field.name && record.ty == ty
        }) {
            return record.id;
        }
        let id = FieldId::new(self.fields.len());
        self.fields.push(FieldRecord {
            id,
            owner: field.owner.clone(),
            name: field.name.clone(),
            ty,
        });
        id
    }

    fn path_id(&mut self, path: &PathRef) -> PathId {
        if let Some(record) = self.paths.iter().find(|record| {
            record.root_ty == path.root_ty
                && record.result_ty == path.result_ty
                && record.read_only == path.read_only
                && record.debug_name == path.debug_name
        }) {
            return record.id;
        }
        let id = PathId::new(self.paths.len());
        self.paths.push(PathRecord {
            id,
            root_ty: path.root_ty,
            result_ty: path.result_ty,
            read_only: path.read_only,
            debug_name: path.debug_name.clone(),
        });
        id
    }
}

fn lower_function(
    function: &IrFunction,
    function_refs: &HashMap<FunctionId, FunctionRef>,
    context: &mut BytecodeLoweringContext,
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
        lower_block(
            block,
            &block_offsets,
            function_refs,
            context,
            &mut instructions,
        )?;
    }

    let metadata = FunctionMetadata {
        params: function.params.iter().map(|param| param.ty).collect(),
        return_type: function.return_type,
        locals: function.locals.iter().map(|local| local.ty).collect(),
        registers: function.temps.iter().map(|temp| temp.ty).collect(),
        control_flow_targets: collect_control_flow_targets(&instructions),
        effects: function.effects,
    };

    Ok(BytecodeFunction {
        id: *function_refs
            .get(&function.hir_id)
            .expect("bytecode lowering should have a function ref for every IR function"),
        name: function.name.clone(),
        parameter_count: function.params.len() as u16,
        register_count: function.temps.len() as u16,
        local_count: function.locals.len() as u16,
        metadata,
        instructions,
    })
}

fn collect_function_table(functions: &[BytecodeFunction]) -> Vec<FunctionRecord> {
    functions
        .iter()
        .map(|function| FunctionRecord {
            id: function.id,
            name: function.name.clone(),
            params: function.metadata.params.clone(),
            return_type: function.metadata.return_type,
            effects: function.metadata.effects,
        })
        .collect()
}

fn collect_constant_pool(functions: &[BytecodeFunction]) -> Vec<ConstantOperand> {
    let mut constants = Vec::new();
    for function in functions {
        for instruction in &function.instructions {
            if let BytecodeInstruction::LoadConst { constant, .. } = instruction
                && !constants.contains(constant)
            {
                constants.push(constant.clone());
            }
        }
    }
    constants
}

fn collect_type_table(module: &BytecodeModule) -> Vec<ValueType> {
    let mut types = Vec::new();
    for slot in &module.module_slots {
        push_type(&mut types, slot.ty);
    }
    for field in &module.fields {
        push_type(&mut types, field.ty);
    }
    for path in &module.paths {
        push_type(&mut types, path.root_ty);
        push_type(&mut types, path.result_ty);
    }
    for function in &module.functions {
        push_type(&mut types, function.metadata.return_type);
        for ty in function
            .metadata
            .params
            .iter()
            .chain(&function.metadata.locals)
            .chain(&function.metadata.registers)
        {
            push_type(&mut types, *ty);
        }
    }
    types
}

fn push_type(types: &mut Vec<ValueType>, ty: ValueType) {
    if !types.contains(&ty) {
        types.push(ty);
    }
}

fn collect_control_flow_targets(instructions: &[BytecodeInstruction]) -> Vec<JumpTarget> {
    let mut targets = Vec::new();
    for instruction in instructions {
        match instruction {
            BytecodeInstruction::Jump { target } => push_target(&mut targets, *target),
            BytecodeInstruction::Branch {
                then_target,
                else_target,
                ..
            } => {
                push_target(&mut targets, *then_target);
                push_target(&mut targets, *else_target);
            }
            _ => {}
        }
    }
    targets
}

fn push_target(targets: &mut Vec<JumpTarget>, target: JumpTarget) {
    if !targets.contains(&target) {
        targets.push(target);
    }
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
    context: &mut BytecodeLoweringContext,
    out: &mut Vec<BytecodeInstruction>,
) -> Result<(), BytecodeLoweringError> {
    for instruction in &block.instructions {
        out.push(lower_instruction(instruction, function_refs, context));
    }

    if let Some(terminator) = &block.terminator {
        out.push(lower_terminator(terminator, block_offsets)?);
    }

    Ok(())
}

fn lower_instruction(
    instruction: &Instruction,
    function_refs: &HashMap<FunctionId, FunctionRef>,
    context: &mut BytecodeLoweringContext,
) -> BytecodeInstruction {
    match instruction {
        Instruction::LoadConst { dst, constant } => BytecodeInstruction::LoadConst {
            dst: lower_value(*dst),
            constant: lower_constant(constant),
        },
        Instruction::LoadLocal { dst, local } => BytecodeInstruction::LoadLocal {
            dst: lower_value(*dst),
            local: lower_local(*local),
        },
        Instruction::LoadModule { dst, slot } => BytecodeInstruction::LoadModule {
            dst: lower_value(*dst),
            slot: lower_module_slot(*slot),
        },
        Instruction::StoreLocal { local, src } => BytecodeInstruction::StoreLocal {
            local: lower_local(*local),
            src: lower_value(*src),
        },
        Instruction::StoreModule { slot, src } => BytecodeInstruction::StoreModule {
            slot: lower_module_slot(*slot),
            src: lower_value(*src),
        },
        Instruction::Move { dst, src } => BytecodeInstruction::Move {
            dst: lower_value(*dst),
            src: lower_value(*src),
        },
        Instruction::Unary { dst, op, operand } => BytecodeInstruction::Unary {
            dst: lower_value(*dst),
            op: match op {
                IrUnaryOp::Neg => UnaryOp::Neg,
                IrUnaryOp::Not => UnaryOp::Not,
            },
            operand: lower_value(*operand),
        },
        Instruction::Binary { dst, op, lhs, rhs } => BytecodeInstruction::Binary {
            dst: lower_value(*dst),
            op: lower_binary_op(*op),
            lhs: lower_value(*lhs),
            rhs: lower_value(*rhs),
        },
        Instruction::Call { dst, callee, args } => BytecodeInstruction::Call {
            dst: dst.map(lower_value),
            callee: match callee {
                IrCallTarget::Function(id) => CallTarget::Function(
                    *function_refs
                        .get(id)
                        .expect("bytecode lowering should resolve direct call targets"),
                ),
                IrCallTarget::Value(value) => CallTarget::Register(lower_value(*value)),
                IrCallTarget::BuiltinMethod(method) => {
                    CallTarget::BuiltinMethod(lower_builtin_method(*method))
                }
                IrCallTarget::RuntimeHelper(helper) => {
                    CallTarget::RuntimeHelper(lower_runtime_helper(helper))
                }
            },
            args: args.iter().map(|arg| lower_value(*arg)).collect(),
        },
        Instruction::MakeTuple { dst, elements } => BytecodeInstruction::MakeTuple {
            dst: lower_value(*dst),
            elements: elements
                .iter()
                .map(|element| lower_value(*element))
                .collect(),
        },
        Instruction::MakeArray { dst, elements } => BytecodeInstruction::MakeArray {
            dst: lower_value(*dst),
            elements: elements
                .iter()
                .map(|element| lower_value(*element))
                .collect(),
        },
        Instruction::MakeStruct { dst, name, fields } => BytecodeInstruction::MakeStruct {
            dst: lower_value(*dst),
            name: name.clone(),
            fields: fields
                .iter()
                .map(|field| StructFieldInit {
                    name: field.name.clone(),
                    value: lower_value(field.value),
                })
                .collect(),
        },
        Instruction::ReadAggregateField { dst, base, field } => {
            BytecodeInstruction::ReadAggregateField {
                dst: lower_value(*dst),
                base: lower_value(*base),
                field: context.field_id(field, dst.ty),
            }
        }
        Instruction::WriteAggregateField { base, field, value } => {
            BytecodeInstruction::WriteAggregateField {
                base: lower_value(*base),
                field: context.field_id(field, value.ty),
                value: lower_value(*value),
            }
        }
        Instruction::ReadAggregateIndex { dst, base, index } => {
            BytecodeInstruction::ReadAggregateIndex {
                dst: lower_value(*dst),
                base: lower_value(*base),
                index: lower_value(*index),
            }
        }
        Instruction::WriteAggregateIndex { base, index, value } => {
            BytecodeInstruction::WriteAggregateIndex {
                base: lower_value(*base),
                index: lower_value(*index),
                value: lower_value(*value),
            }
        }
        Instruction::ReadPath {
            dst,
            root_or_view,
            path,
            dynamic_args,
        } => BytecodeInstruction::ReadPath {
            dst: lower_value(*dst),
            root_or_view: lower_value(*root_or_view),
            path: context.path_id(path),
            dynamic_args: dynamic_args.iter().map(|arg| lower_value(*arg)).collect(),
        },
        Instruction::SetPath {
            root_or_view,
            path,
            dynamic_args,
            value,
        } => BytecodeInstruction::SetPath {
            root_or_view: lower_value(*root_or_view),
            path: context.path_id(path),
            dynamic_args: dynamic_args.iter().map(|arg| lower_value(*arg)).collect(),
            value: lower_value(*value),
        },
        Instruction::ModifyPath {
            dst,
            root_or_view,
            path,
            dynamic_args,
            op,
            value,
        } => BytecodeInstruction::ModifyPath {
            dst: dst.map(lower_value),
            root_or_view: lower_value(*root_or_view),
            path: context.path_id(path),
            dynamic_args: dynamic_args.iter().map(|arg| lower_value(*arg)).collect(),
            op: lower_binary_op(*op),
            value: lower_value(*value),
        },
        Instruction::MakePathView {
            dst,
            root_or_view,
            path,
            dynamic_args,
        } => BytecodeInstruction::MakePathView {
            dst: lower_value(*dst),
            root_or_view: lower_value(*root_or_view),
            path: context.path_id(path),
            dynamic_args: dynamic_args.iter().map(|arg| lower_value(*arg)).collect(),
        },
    }
}

fn lower_terminator(
    terminator: &Terminator,
    block_offsets: &HashMap<BlockId, JumpTarget>,
) -> Result<BytecodeInstruction, BytecodeLoweringError> {
    Ok(match terminator {
        Terminator::Return(value) => BytecodeInstruction::Return(value.map(lower_value)),
        Terminator::Jump(target) => BytecodeInstruction::Jump {
            target: lower_jump(*target, block_offsets)?,
        },
        Terminator::Branch {
            cond,
            then_block,
            else_block,
        } => BytecodeInstruction::Branch {
            cond: lower_value(*cond),
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

fn lower_builtin_method(method: BuiltinMethod) -> BuiltinMethod {
    method
}

fn lower_binary_op(op: IrBinaryOp) -> BinaryOp {
    match op {
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
            unreachable!("short-circuit ops should be lowered into branches before bytecode")
        }
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

fn lower_value(value: IrValue) -> Register {
    lower_temp(value.temp)
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
