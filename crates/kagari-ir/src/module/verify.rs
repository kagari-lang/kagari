use super::contracts;
use super::instruction::CallTarget;
use kagari_common::identity::DefinitionKind;
use smallvec::SmallVec;
use std::iter;
use std::{collections::HashSet, ops::Deref};

use kagari_common::{Span, cancellation::CancellationToken};

use super::{
    BlockId, EffectSet, Instruction, IrFunction, IrModule, IrValue, LocalId, TempId, Terminator,
    ValueType, contracts::ContractError, ids::InstanceId,
};

mod flow;
mod layout;
mod operation;

/// Owns a checked module. Mutating a copy requires verifying it again.
///
/// ```compile_fail
/// fn bypass(raw: &kagari_ir::module::IrModule) {
///     kagari_ir::bytecode::lower_to_bytecode(raw);
/// }
/// ```
///
/// ```compile_fail
/// fn mutate(checked: &mut kagari_ir::module::VerifiedIrModule) {
///     checked.functions.clear();
/// }
/// ```
#[derive(Debug, Clone)]
pub struct VerifiedIrModule(IrModule);

impl Deref for VerifiedIrModule {
    type Target = IrModule;
    fn deref(&self) -> &IrModule {
        &self.0
    }
}

impl VerifiedIrModule {
    pub fn into_unverified(self) -> IrModule {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{} at {function:?}/{block:?}/{instruction:?}: {kind:?}", self.code())]
pub struct IrVerificationError {
    pub function: Option<InstanceId>,
    pub block: Option<BlockId>,
    pub instruction: Option<usize>,
    pub span: Option<Span>,
    pub kind: IrVerificationErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrVerificationErrorKind {
    Cancelled,
    Limit {
        resource: &'static str,
        limit: usize,
    },
    InvalidInstance,
    InvalidStructLayout,
    InvalidEnumLayout,
    InvalidPublicAbi,
    InvalidHostInterface,
    InvalidEnumInitializer,
    InvalidInterfaceTable,
    InvalidField,
    InvalidStructInitializer,
    ReadOnlyField,
    InvalidModuleSlot,
    InvalidInitializer,
    InvalidBlock(BlockId),
    MissingTerminator,
    InvalidDebugMetadata,
    InvalidParameterLayout,
    InvalidTemp(TempId),
    InvalidLocal(LocalId),
    UninitializedTemp(TempId),
    UninitializedLocal(LocalId),
    InvalidCall(InstanceId),
    CallArity {
        expected: usize,
        found: usize,
    },
    UnsupportedCall,
    ReadOnlyPath,
    MissingEffects,
    Contract(ContractError),
}

impl IrVerificationError {
    pub fn code(&self) -> &'static str {
        match self.kind {
            IrVerificationErrorKind::Cancelled => "KG_IR_CANCELLED",
            IrVerificationErrorKind::Limit { .. } => "KG_IR_LIMIT_EXCEEDED",
            IrVerificationErrorKind::InvalidInstance => "KG_IR_INVALID_INSTANCE",
            IrVerificationErrorKind::InvalidStructLayout => "KG_IR_INVALID_STRUCT_LAYOUT",
            IrVerificationErrorKind::InvalidEnumLayout => "KG_IR_INVALID_ENUM_LAYOUT",
            IrVerificationErrorKind::InvalidPublicAbi => "KG_IR_INVALID_PUBLIC_ABI",
            IrVerificationErrorKind::InvalidHostInterface => "KG_IR_INVALID_HOST_INTERFACE",
            IrVerificationErrorKind::InvalidEnumInitializer => "KG_IR_INVALID_ENUM_INITIALIZER",
            IrVerificationErrorKind::InvalidInterfaceTable => "KG_IR_INVALID_INTERFACE_TABLE",
            IrVerificationErrorKind::InvalidField => "KG_IR_INVALID_FIELD",
            IrVerificationErrorKind::InvalidStructInitializer => "KG_IR_INVALID_STRUCT_INITIALIZER",
            IrVerificationErrorKind::ReadOnlyField => "KG_IR_READ_ONLY_FIELD",
            IrVerificationErrorKind::InvalidModuleSlot => "KG_IR_INVALID_MODULE_SLOT",
            IrVerificationErrorKind::InvalidInitializer => "KG_IR_INVALID_INITIALIZER",
            IrVerificationErrorKind::InvalidBlock(_) => "KG_IR_INVALID_BLOCK",
            IrVerificationErrorKind::MissingTerminator => "KG_IR_MISSING_TERMINATOR",
            IrVerificationErrorKind::InvalidDebugMetadata => "KG_IR_INVALID_DEBUG_METADATA",
            IrVerificationErrorKind::InvalidParameterLayout => "KG_IR_INVALID_PARAMETER_LAYOUT",
            IrVerificationErrorKind::InvalidTemp(_) => "KG_IR_INVALID_TEMP",
            IrVerificationErrorKind::InvalidLocal(_) => "KG_IR_INVALID_LOCAL",
            IrVerificationErrorKind::UninitializedTemp(_) => "KG_IR_UNINITIALIZED_TEMP",
            IrVerificationErrorKind::UninitializedLocal(_) => "KG_IR_UNINITIALIZED_LOCAL",
            IrVerificationErrorKind::InvalidCall(_) => "KG_IR_INVALID_CALL",
            IrVerificationErrorKind::CallArity { .. } => "KG_IR_CALL_ARITY",
            IrVerificationErrorKind::UnsupportedCall => "KG_IR_UNSUPPORTED_CALL",
            IrVerificationErrorKind::ReadOnlyPath => "KG_IR_READ_ONLY_PATH",
            IrVerificationErrorKind::MissingEffects => "KG_IR_MISSING_EFFECTS",
            IrVerificationErrorKind::Contract(_) => "KG_IR_OPERATION_CONTRACT",
        }
    }
}

/// Validates representation contracts and definite initialization before any
/// narrowing of IDs or flattening of control flow into bytecode offsets.
pub fn verify_ir(
    module: IrModule,
    cancel: &CancellationToken,
) -> Result<VerifiedIrModule, IrVerificationError> {
    let context = Context {
        function: None,
        block: None,
        instruction: None,
        span: None,
        cancel,
    };
    context.check_cancel()?;
    layout::verify(&module, context)?;
    context.limit(
        module.interface_instances.len(),
        4096,
        "interface instances",
    )?;
    let mut interfaces = HashSet::new();
    for instance in &module.interface_instances {
        context.check_cancel()?;
        if !interfaces.insert(instance)
            || !instance.arguments.iter().all(|ty| ty.is_concrete())
            || !instance.declaration.within_path_limit()
            || instance
                .declaration
                .path
                .last()
                .is_none_or(|part| part.kind != DefinitionKind::Impl)
        {
            return Err(context.error(IrVerificationErrorKind::InvalidInterfaceTable));
        }
    }
    context.limit(
        module.functions.len(),
        u32::MAX as usize,
        "function instances",
    )?;
    context.limit(
        module.module_slots.len(),
        u16::MAX as usize + 1,
        "module slots",
    )?;
    let mut keys = HashSet::new();
    for (index, function) in module.functions.iter().enumerate() {
        let context = Context {
            function: Some(function.id),
            span: Some(function.debug.source_span),
            ..context
        };
        context.check_cancel()?;
        if function.id.index() != index
            || function.instance.declaration.module != module.identity
            || !function
                .instance
                .arguments
                .iter()
                .all(|ty| ty.is_concrete())
            || !keys.insert(&function.instance)
        {
            return Err(context.error(IrVerificationErrorKind::InvalidInstance));
        }
    }
    for (index, slot) in module.module_slots.iter().enumerate() {
        context.check_cancel()?;
        if slot.id.index() != index {
            return Err(context.error(IrVerificationErrorKind::InvalidModuleSlot));
        }
    }
    for function in &module.functions {
        verify_function(
            &module,
            function,
            Context {
                function: Some(function.id),
                span: Some(function.debug.source_span),
                ..context
            },
        )?;
    }
    Ok(VerifiedIrModule(module))
}

#[derive(Clone, Copy)]
struct Context<'a> {
    function: Option<InstanceId>,
    block: Option<BlockId>,
    instruction: Option<usize>,
    span: Option<Span>,
    cancel: &'a CancellationToken,
}

impl Context<'_> {
    fn error(self, kind: IrVerificationErrorKind) -> IrVerificationError {
        IrVerificationError {
            function: self.function,
            block: self.block,
            instruction: self.instruction,
            span: self.span,
            kind,
        }
    }
    fn check_cancel(self) -> Result<(), IrVerificationError> {
        self.cancel
            .check()
            .map_err(|_| self.error(IrVerificationErrorKind::Cancelled))
    }
    fn limit(
        self,
        count: usize,
        limit: usize,
        resource: &'static str,
    ) -> Result<(), IrVerificationError> {
        if count > limit {
            Err(self.error(IrVerificationErrorKind::Limit { resource, limit }))
        } else {
            Ok(())
        }
    }
    fn expect(
        self,
        found: ValueType,
        expected: ValueType,
        label: &'static str,
    ) -> Result<(), IrVerificationError> {
        contracts::expect_type(found, expected, label)
            .map_err(|e| self.error(IrVerificationErrorKind::Contract(e)))
    }
    fn value(self, function: &IrFunction, value: IrValue) -> Result<(), IrVerificationError> {
        let ty = function
            .temps
            .get(value.temp.index())
            .ok_or_else(|| self.error(IrVerificationErrorKind::InvalidTemp(value.temp)))?
            .ty;
        self.expect(value.ty, ty, "temporary annotation")
    }
    fn local(
        self,
        function: &IrFunction,
        local: LocalId,
    ) -> Result<ValueType, IrVerificationError> {
        function
            .locals
            .get(local.index())
            .map(|l| l.ty)
            .ok_or_else(|| self.error(IrVerificationErrorKind::InvalidLocal(local)))
    }
}

fn verify_function(
    module: &IrModule,
    function: &IrFunction,
    context: Context<'_>,
) -> Result<(), IrVerificationError> {
    for (count, name) in [
        (function.params.len(), "parameters"),
        (function.locals.len(), "locals"),
        (function.temps.len(), "temporaries"),
    ] {
        context.limit(count, u16::MAX as usize, name)?;
    }
    context.limit(function.blocks.len(), u32::MAX as usize, "blocks")?;
    if function.entry.index() >= function.blocks.len() {
        return Err(context.error(IrVerificationErrorKind::InvalidBlock(function.entry)));
    }
    for (index, param) in function.params.iter().enumerate() {
        context.check_cancel()?;
        if param.local.index() != index {
            return Err(context.error(IrVerificationErrorKind::InvalidParameterLayout));
        }
        context.expect(
            context.local(function, param.local)?,
            param.ty,
            "parameter local",
        )?;
    }
    let mut debug_local_ids = HashSet::new();
    for local in &function.debug.locals {
        context.check_cancel()?;
        if !debug_local_ids.insert(local.local) {
            return Err(context.error(IrVerificationErrorKind::InvalidDebugMetadata));
        }
        context.expect(
            context.local(function, local.local)?,
            local.ty,
            "debug local",
        )?;
        if local.is_parameter != (local.local.index() < function.params.len()) {
            return Err(context.error(IrVerificationErrorKind::InvalidDebugMetadata));
        }
    }
    let scopes = &function.debug.lexical_scopes;
    if scopes.len()
        != function
            .debug
            .locals
            .len()
            .checked_sub(function.params.len())
            .and_then(|count| count.checked_add(1))
            .unwrap_or(0)
        || scopes
            .first()
            .is_none_or(|root| root.parent.is_some() || root.local.is_some())
    {
        return Err(context.error(IrVerificationErrorKind::InvalidDebugMetadata));
    }
    let mut scoped_locals = HashSet::new();
    for (index, scope) in scopes.iter().enumerate().skip(1) {
        context.check_cancel()?;
        let Some(local) = scope.local else {
            return Err(context.error(IrVerificationErrorKind::InvalidDebugMetadata));
        };
        if scope.parent.is_none_or(|parent| parent >= index)
            || local.index() < function.params.len()
            || !scoped_locals.insert(local)
            || !debug_local_ids.contains(&local)
        {
            return Err(context.error(IrVerificationErrorKind::InvalidDebugMetadata));
        }
    }
    let mut effects = EffectSet::default();
    let mut count = 0usize;
    for (index, block) in function.blocks.iter().enumerate() {
        let context = Context {
            block: Some(BlockId::new(index)),
            ..context
        };
        context.check_cancel()?;
        count = count
            .saturating_add(block.instructions.len())
            .saturating_add(1);
        context.limit(count, u32::MAX as usize, "instructions")?;
        if block.instructions.len() != block.instruction_spans.len()
            || block.instructions.len() != block.instruction_scopes.len()
            || block.terminator_scope.is_none()
            || block
                .instruction_scopes
                .iter()
                .chain(block.terminator_scope.iter())
                .any(|scope| *scope >= scopes.len())
        {
            return Err(context.error(IrVerificationErrorKind::InvalidDebugMetadata));
        }
        let terminator = block
            .terminator
            .as_ref()
            .ok_or_else(|| context.error(IrVerificationErrorKind::MissingTerminator))?;
        for (index, instruction) in block.instructions.iter().enumerate() {
            let context = Context {
                instruction: Some(index),
                span: Some(block.instruction_spans[index]),
                ..context
            };
            context.check_cancel()?;
            for value in inputs(instruction) {
                context.value(function, value)?;
            }
            if let Some(dst) = output(instruction) {
                context.value(function, dst)?;
            }
            operation::verify(module, function, instruction, context)?;
            effects = effects.union(instruction.effects());
        }
        let context = Context {
            span: block.terminator_span,
            ..context
        };
        for target in successors(terminator) {
            if target.index() >= function.blocks.len() {
                return Err(context.error(IrVerificationErrorKind::InvalidBlock(target)));
            }
        }
        match terminator {
            Terminator::Return(value) => {
                if let Some(value) = value {
                    context.value(function, *value)?;
                }
                context.expect(
                    value.map_or(ValueType::Unit, |v| v.ty),
                    function.return_type,
                    "return value",
                )?;
            }
            Terminator::Branch { cond, .. } => {
                context.value(function, *cond)?;
                context.expect(cond.ty, ValueType::Bool, "branch condition")?;
            }
            _ => {}
        }
        effects = effects.union(terminator.effects());
    }
    if function.effects.union(effects) != function.effects {
        return Err(context.error(IrVerificationErrorKind::MissingEffects));
    }
    flow::verify(function, context)
}

fn successors(terminator: &Terminator) -> SmallVec<[BlockId; 2]> {
    match terminator {
        Terminator::Jump(target) => smallvec::smallvec![*target],
        Terminator::Branch {
            then_block,
            else_block,
            ..
        } => smallvec::smallvec![*then_block, *else_block],
        Terminator::Return(_) | Terminator::Unreachable => smallvec::smallvec![],
    }
}

fn inputs(instruction: &Instruction) -> SmallVec<[IrValue; 4]> {
    match instruction {
        Instruction::Convert { src, .. } => smallvec::smallvec![*src],
        Instruction::Numeric { lhs, rhs, .. } => {
            iter::once(*lhs).chain(rhs.iter().copied()).collect()
        }
        Instruction::MapResultError {
            original, error, ..
        } => smallvec::smallvec![*original, *error],
        Instruction::Iter { value, .. } | Instruction::StandardEnum { value, .. } => {
            value.iter().copied().collect()
        }
        Instruction::LoadConst { .. }
        | Instruction::LoadLocal { .. }
        | Instruction::LoadModule { .. } => smallvec::smallvec![],
        Instruction::StoreLocal { src, .. }
        | Instruction::StoreModule { src, .. }
        | Instruction::Move { src, .. } => {
            smallvec::smallvec![*src]
        }
        Instruction::Unary { operand, .. } => smallvec::smallvec![*operand],
        Instruction::BeginIteration { collection } => smallvec::smallvec![*collection],
        Instruction::EndIteration => smallvec::smallvec![],
        Instruction::RangeBound { value, .. } => smallvec::smallvec![*value],
        Instruction::MakeRange { start, end, .. } => start.iter().chain(end).copied().collect(),
        Instruction::RepeatArray { value, count, .. } => smallvec::smallvec![*value, *count],
        Instruction::Binary { lhs, rhs, .. } => smallvec::smallvec![*lhs, *rhs],
        Instruction::Call { callee, args, .. } => {
            let mut values = args.clone();
            if let CallTarget::Value(value) = callee {
                values.push(*value);
            }
            if let CallTarget::Closure { value, .. } = callee {
                values.push(*value);
            }
            values
        }
        Instruction::MakeTuple { elements, .. }
        | Instruction::MakeArray { elements, .. }
        | Instruction::MakeEnum {
            fields: elements, ..
        } => elements.clone(),
        Instruction::MakeClosure { captures, .. } => captures.clone(),
        Instruction::MakeCell { value, .. } => smallvec::smallvec![*value],
        Instruction::ReadCell { cell, .. } => smallvec::smallvec![*cell],
        Instruction::WriteCell { cell, value } => smallvec::smallvec![*cell, *value],
        Instruction::MakeStruct { fields, .. } => fields.iter().map(|f| f.value).collect(),
        Instruction::MakeInterface { value, .. } | Instruction::UpcastInterface { value, .. } => {
            smallvec::smallvec![*value]
        }
        Instruction::TestEnumVariant { value, .. } | Instruction::ReadEnumPayload { value, .. } => {
            smallvec::smallvec![*value]
        }
        Instruction::ReadAggregateField { base, .. } => smallvec::smallvec![*base],
        Instruction::WriteAggregateField { base, value, .. } => smallvec::smallvec![*base, *value],
        Instruction::ReadAggregateIndex { base, index, .. } => smallvec::smallvec![*base, *index],
        Instruction::WriteAggregateIndex {
            base, index, value, ..
        } => smallvec::smallvec![*base, *index, *value],
        Instruction::ReadPath {
            root_or_view,
            dynamic_args,
            ..
        }
        | Instruction::MakePathView {
            root_or_view,
            dynamic_args,
            ..
        } => {
            let mut values = dynamic_args.clone();
            values.push(*root_or_view);
            values
        }
        Instruction::SetPath {
            root_or_view,
            dynamic_args,
            value,
            ..
        }
        | Instruction::ModifyPath {
            root_or_view,
            dynamic_args,
            value,
            ..
        } => {
            let mut values = dynamic_args.clone();
            values.push(*root_or_view);
            values.push(*value);
            values
        }
    }
}

fn output(instruction: &Instruction) -> Option<IrValue> {
    match instruction {
        Instruction::LoadConst { dst, .. }
        | Instruction::LoadLocal { dst, .. }
        | Instruction::LoadModule { dst, .. }
        | Instruction::Move { dst, .. }
        | Instruction::Unary { dst, .. }
        | Instruction::Binary { dst, .. }
        | Instruction::MakeTuple { dst, .. }
        | Instruction::MakeArray { dst, .. }
        | Instruction::RepeatArray { dst, .. }
        | Instruction::MakeRange { dst, .. }
        | Instruction::RangeBound { dst, .. }
        | Instruction::MakeClosure { dst, .. }
        | Instruction::MakeCell { dst, .. }
        | Instruction::ReadCell { dst, .. }
        | Instruction::MakeStruct { dst, .. }
        | Instruction::Convert { dst, .. }
        | Instruction::Numeric { dst, .. }
        | Instruction::MapResultError { dst, .. }
        | Instruction::Iter { dst, .. }
        | Instruction::StandardEnum { dst, .. }
        | Instruction::MakeEnum { dst, .. }
        | Instruction::MakeInterface { dst, .. }
        | Instruction::UpcastInterface { dst, .. }
        | Instruction::TestEnumVariant { dst, .. }
        | Instruction::ReadEnumPayload { dst, .. }
        | Instruction::ReadAggregateField { dst, .. }
        | Instruction::ReadAggregateIndex { dst, .. }
        | Instruction::ReadPath { dst, .. }
        | Instruction::MakePathView { dst, .. } => Some(*dst),
        Instruction::Call { dst, .. } | Instruction::ModifyPath { dst, .. } => *dst,
        Instruction::StoreLocal { .. }
        | Instruction::StoreModule { .. }
        | Instruction::WriteAggregateField { .. }
        | Instruction::WriteAggregateIndex { .. }
        | Instruction::SetPath { .. } => None,
        Instruction::WriteCell { .. } => None,
        Instruction::BeginIteration { .. } | Instruction::EndIteration => None,
    }
}
