use std::{collections::HashSet, fmt, ops::Deref};

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

#[derive(Debug, Clone, PartialEq, Eq)]
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
        use IrVerificationErrorKind::*;
        match self.kind {
            Cancelled => "KG_IR_CANCELLED",
            Limit { .. } => "KG_IR_LIMIT_EXCEEDED",
            InvalidInstance => "KG_IR_INVALID_INSTANCE",
            InvalidStructLayout => "KG_IR_INVALID_STRUCT_LAYOUT",
            InvalidField => "KG_IR_INVALID_FIELD",
            InvalidStructInitializer => "KG_IR_INVALID_STRUCT_INITIALIZER",
            ReadOnlyField => "KG_IR_READ_ONLY_FIELD",
            InvalidModuleSlot => "KG_IR_INVALID_MODULE_SLOT",
            InvalidInitializer => "KG_IR_INVALID_INITIALIZER",
            InvalidBlock(_) => "KG_IR_INVALID_BLOCK",
            MissingTerminator => "KG_IR_MISSING_TERMINATOR",
            InvalidDebugMetadata => "KG_IR_INVALID_DEBUG_METADATA",
            InvalidParameterLayout => "KG_IR_INVALID_PARAMETER_LAYOUT",
            InvalidTemp(_) => "KG_IR_INVALID_TEMP",
            InvalidLocal(_) => "KG_IR_INVALID_LOCAL",
            UninitializedTemp(_) => "KG_IR_UNINITIALIZED_TEMP",
            UninitializedLocal(_) => "KG_IR_UNINITIALIZED_LOCAL",
            InvalidCall(_) => "KG_IR_INVALID_CALL",
            CallArity { .. } => "KG_IR_CALL_ARITY",
            UnsupportedCall => "KG_IR_UNSUPPORTED_CALL",
            ReadOnlyPath => "KG_IR_READ_ONLY_PATH",
            MissingEffects => "KG_IR_MISSING_EFFECTS",
            Contract(_) => "KG_IR_OPERATION_CONTRACT",
        }
    }
}

impl fmt::Display for IrVerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {:?}/{:?}/{:?}: {:?}",
            self.code(),
            self.function,
            self.block,
            self.instruction,
            self.kind
        )
    }
}
impl std::error::Error for IrVerificationError {}

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
    if let Some(init) = module.module_init {
        let function = module
            .functions
            .get(init.index())
            .ok_or_else(|| context.error(IrVerificationErrorKind::InvalidInitializer))?;
        if !function.params.is_empty() {
            return Err(context.error(IrVerificationErrorKind::InvalidInitializer));
        }
    }
    let mut host_declarations = std::collections::HashMap::new();
    let mut host_symbols = std::collections::HashMap::new();
    for function in &module.functions {
        for block in &function.blocks {
            for instruction in &block.instructions {
                context.check_cancel()?;
                let Instruction::Call {
                    callee: super::CallTarget::HostFunction(declaration),
                    ..
                } = instruction
                else {
                    continue;
                };
                let declaration = declaration.as_ref();
                if host_declarations
                    .insert(&declaration.id, declaration)
                    .is_some_and(
                        |previous: &kagari_common::host_interface::HostFunctionDeclaration| {
                            !previous.matches_binding(declaration)
                        },
                    )
                    || host_symbols
                        .insert(&declaration.symbol, &declaration.id)
                        .is_some_and(|previous| previous != &declaration.id)
                {
                    return Err(context.error(IrVerificationErrorKind::Contract(
                        ContractError::InvalidOperation {
                            reason: "conflicting host declarations",
                        },
                    )));
                }
            }
        }
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
        super::contracts::expect_type(found, expected, label)
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
    for local in &function.debug.locals {
        context.check_cancel()?;
        context.expect(
            context.local(function, local.local)?,
            local.ty,
            "debug local",
        )?;
        if local.is_parameter != (local.local.index() < function.params.len()) {
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
        if block.instructions.len() != block.instruction_spans.len() {
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

fn successors(terminator: &Terminator) -> smallvec::SmallVec<[BlockId; 2]> {
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

fn inputs(instruction: &Instruction) -> smallvec::SmallVec<[IrValue; 4]> {
    use Instruction::*;
    match instruction {
        LoadConst { .. } | LoadLocal { .. } | LoadModule { .. } => smallvec::smallvec![],
        StoreLocal { src, .. } | StoreModule { src, .. } | Move { src, .. } => {
            smallvec::smallvec![*src]
        }
        Unary { operand, .. } => smallvec::smallvec![*operand],
        Binary { lhs, rhs, .. } => smallvec::smallvec![*lhs, *rhs],
        Call { callee, args, .. } => {
            let mut values = args.clone();
            if let super::instruction::CallTarget::Value(value) = callee {
                values.push(*value);
            }
            values
        }
        MakeTuple { elements, .. } | MakeArray { elements, .. } => elements.clone(),
        MakeStruct { fields, .. } => fields.iter().map(|f| f.value).collect(),
        ReadAggregateField { base, .. } => smallvec::smallvec![*base],
        WriteAggregateField { base, value, .. } => smallvec::smallvec![*base, *value],
        ReadAggregateIndex { base, index, .. } => smallvec::smallvec![*base, *index],
        WriteAggregateIndex {
            base, index, value, ..
        } => smallvec::smallvec![*base, *index, *value],
        ReadPath {
            root_or_view,
            dynamic_args,
            ..
        }
        | MakePathView {
            root_or_view,
            dynamic_args,
            ..
        } => {
            let mut values = dynamic_args.clone();
            values.push(*root_or_view);
            values
        }
        SetPath {
            root_or_view,
            dynamic_args,
            value,
            ..
        }
        | ModifyPath {
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
    use Instruction::*;
    match instruction {
        LoadConst { dst, .. }
        | LoadLocal { dst, .. }
        | LoadModule { dst, .. }
        | Move { dst, .. }
        | Unary { dst, .. }
        | Binary { dst, .. }
        | MakeTuple { dst, .. }
        | MakeArray { dst, .. }
        | MakeStruct { dst, .. }
        | ReadAggregateField { dst, .. }
        | ReadAggregateIndex { dst, .. }
        | ReadPath { dst, .. }
        | MakePathView { dst, .. } => Some(*dst),
        Call { dst, .. } | ModifyPath { dst, .. } => *dst,
        StoreLocal { .. }
        | StoreModule { .. }
        | WriteAggregateField { .. }
        | WriteAggregateIndex { .. }
        | SetPath { .. } => None,
    }
}
