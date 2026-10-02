use kagari_abi::{
    contracts::{self, ContractError},
    effects::EffectSet,
    representation::ValueType,
    types::AbiType,
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind, span::Span};
use std::{collections::HashSet, ops::Deref};

use crate::{
    analysis::FunctionAnalysis,
    function::{MirFunction, MirModule},
    ids::{BlockId, InstanceId, LocalId, TempId},
    instruction::{MirValue, Terminator},
    verify::analysis::Budget,
};

mod analysis;
mod debug;
mod flow;
mod layout;
mod operation;

/// Owns a checked module. Mutating a copy requires verifying it again.
///
/// ```compile_fail
/// fn bypass(raw: kagari_mir::MirModule) -> kagari_mir::VerifiedMirModule {
///     raw
/// }
/// ```
///
/// ```compile_fail
/// fn mutate(checked: &mut kagari_mir::VerifiedMirModule) {
///     checked.functions.clear();
/// }
/// ```
#[derive(Debug, Clone)]
pub struct VerifiedMirModule {
    module: MirModule,
    analyses: Vec<FunctionAnalysis>,
}

impl Deref for VerifiedMirModule {
    type Target = MirModule;
    fn deref(&self) -> &MirModule {
        &self.module
    }
}

impl VerifiedMirModule {
    /// Facts belong to this immutable module revision and cannot survive mutation.
    pub fn analysis(&self, function: InstanceId) -> Option<&FunctionAnalysis> {
        self.analyses.get(function.index())
    }

    pub fn into_unverified(self) -> MirModule {
        self.module
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{} at {function:?}/{block:?}/{instruction:?}: {kind:?}", self.code())]
pub struct MirVerificationError {
    pub function: Option<InstanceId>,
    pub block: Option<BlockId>,
    pub instruction: Option<usize>,
    pub span: Option<Span>,
    pub kind: MirVerificationErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirVerificationErrorKind {
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
    InvalidNativeTarget,
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

impl MirVerificationError {
    pub fn code(&self) -> &'static str {
        match self.kind {
            MirVerificationErrorKind::Cancelled => "KG_IR_CANCELLED",
            MirVerificationErrorKind::Limit { .. } => "KG_IR_LIMIT_EXCEEDED",
            MirVerificationErrorKind::InvalidInstance => "KG_IR_INVALID_INSTANCE",
            MirVerificationErrorKind::InvalidStructLayout => "KG_IR_INVALID_STRUCT_LAYOUT",
            MirVerificationErrorKind::InvalidEnumLayout => "KG_IR_INVALID_ENUM_LAYOUT",
            MirVerificationErrorKind::InvalidPublicAbi => "KG_IR_INVALID_PUBLIC_ABI",
            MirVerificationErrorKind::InvalidHostInterface => "KG_IR_INVALID_HOST_INTERFACE",
            MirVerificationErrorKind::InvalidEnumInitializer => "KG_IR_INVALID_ENUM_INITIALIZER",
            MirVerificationErrorKind::InvalidInterfaceTable => "KG_IR_INVALID_INTERFACE_TABLE",
            MirVerificationErrorKind::InvalidNativeTarget => "KG_IR_INVALID_NATIVE_TARGET",
            MirVerificationErrorKind::InvalidField => "KG_IR_INVALID_FIELD",
            MirVerificationErrorKind::InvalidStructInitializer => {
                "KG_IR_INVALID_STRUCT_INITIALIZER"
            }
            MirVerificationErrorKind::ReadOnlyField => "KG_IR_READ_ONLY_FIELD",
            MirVerificationErrorKind::InvalidModuleSlot => "KG_IR_INVALID_MODULE_SLOT",
            MirVerificationErrorKind::InvalidInitializer => "KG_IR_INVALID_INITIALIZER",
            MirVerificationErrorKind::InvalidBlock(_) => "KG_IR_INVALID_BLOCK",
            MirVerificationErrorKind::MissingTerminator => "KG_IR_MISSING_TERMINATOR",
            MirVerificationErrorKind::InvalidDebugMetadata => "KG_IR_INVALID_DEBUG_METADATA",
            MirVerificationErrorKind::InvalidParameterLayout => "KG_IR_INVALID_PARAMETER_LAYOUT",
            MirVerificationErrorKind::InvalidTemp(_) => "KG_IR_INVALID_TEMP",
            MirVerificationErrorKind::InvalidLocal(_) => "KG_IR_INVALID_LOCAL",
            MirVerificationErrorKind::UninitializedTemp(_) => "KG_IR_UNINITIALIZED_TEMP",
            MirVerificationErrorKind::UninitializedLocal(_) => "KG_IR_UNINITIALIZED_LOCAL",
            MirVerificationErrorKind::InvalidCall(_) => "KG_IR_INVALID_CALL",
            MirVerificationErrorKind::CallArity { .. } => "KG_IR_CALL_ARITY",
            MirVerificationErrorKind::UnsupportedCall => "KG_IR_UNSUPPORTED_CALL",
            MirVerificationErrorKind::ReadOnlyPath => "KG_IR_READ_ONLY_PATH",
            MirVerificationErrorKind::MissingEffects => "KG_IR_MISSING_EFFECTS",
            MirVerificationErrorKind::Contract(_) => "KG_IR_OPERATION_CONTRACT",
        }
    }
}

/// Validates representation contracts and definite initialization before any
/// narrowing of IDs or flattening of control flow into bytecode offsets.
pub fn verify_mir(
    module: MirModule,
    cancel: &CancellationToken,
) -> Result<VerifiedMirModule, MirVerificationError> {
    verify_with_budget(module, cancel, &mut VerificationBudget::default())
}

/// Program verification shares this budget across every retained module seal.
#[derive(Default)]
pub(crate) struct VerificationBudget(Budget);

pub(crate) fn verify_with_budget(
    module: MirModule,
    cancel: &CancellationToken,
    budget: &mut VerificationBudget,
) -> Result<VerifiedMirModule, MirVerificationError> {
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
            || !instance.arguments.iter().enumerate().all(|(position, ty)| {
                ty.is_concrete()
                    || matches!(ty, AbiType::Parameter { owner, position: index }
                    if *owner == instance.declaration && *index == position)
            })
            || !instance.declaration.within_path_limit()
            || instance
                .declaration
                .path
                .last()
                .is_none_or(|part| part.kind != DefinitionKind::Impl)
        {
            return Err(context.error(MirVerificationErrorKind::InvalidInterfaceTable));
        }
    }
    context.limit(module.native_targets.len(), 4096, "native targets")?;
    let mut native = HashSet::new();
    for target in &module.native_targets {
        context.check_cancel()?;
        // A table may retain a default from another module. Linked validation
        // authenticates the target against that owner's declaration and closure.
        if target.host.is_some() || !target.structurally_valid() || !native.insert(&target.instance)
        {
            return Err(context.error(MirVerificationErrorKind::InvalidNativeTarget));
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
            return Err(context.error(MirVerificationErrorKind::InvalidInstance));
        }
    }
    for (index, slot) in module.module_slots.iter().enumerate() {
        context.check_cancel()?;
        if slot.id.index() != index {
            return Err(context.error(MirVerificationErrorKind::InvalidModuleSlot));
        }
    }
    let mut analyses = Vec::new();
    for function in &module.functions {
        analyses.push(verify_function(
            &module,
            function,
            Context {
                function: Some(function.id),
                span: Some(function.debug.source_span),
                ..context
            },
            &mut budget.0,
        )?);
    }
    Ok(VerifiedMirModule { module, analyses })
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
    fn error(self, kind: MirVerificationErrorKind) -> MirVerificationError {
        MirVerificationError {
            function: self.function,
            block: self.block,
            instruction: self.instruction,
            span: self.span,
            kind,
        }
    }
    fn check_cancel(self) -> Result<(), MirVerificationError> {
        self.cancel
            .check()
            .map_err(|_| self.error(MirVerificationErrorKind::Cancelled))
    }
    fn limit(
        self,
        count: usize,
        limit: usize,
        resource: &'static str,
    ) -> Result<(), MirVerificationError> {
        if count > limit {
            Err(self.error(MirVerificationErrorKind::Limit { resource, limit }))
        } else {
            Ok(())
        }
    }
    fn expect(
        self,
        found: ValueType,
        expected: ValueType,
        label: &'static str,
    ) -> Result<(), MirVerificationError> {
        contracts::expect_type(found, expected, label)
            .map_err(|e| self.error(MirVerificationErrorKind::Contract(e)))
    }
    fn value(self, function: &MirFunction, value: MirValue) -> Result<(), MirVerificationError> {
        let ty = function
            .temps
            .get(value.temp.index())
            .ok_or_else(|| self.error(MirVerificationErrorKind::InvalidTemp(value.temp)))?
            .ty;
        self.expect(value.ty, ty, "temporary annotation")
    }
    fn local(
        self,
        function: &MirFunction,
        local: LocalId,
    ) -> Result<ValueType, MirVerificationError> {
        function
            .locals
            .get(local.index())
            .map(|l| l.ty)
            .ok_or_else(|| self.error(MirVerificationErrorKind::InvalidLocal(local)))
    }
}

fn verify_function(
    module: &MirModule,
    function: &MirFunction,
    context: Context<'_>,
    budget: &mut Budget,
) -> Result<FunctionAnalysis, MirVerificationError> {
    if !function
        .semantic
        .types_valid(Some(&function.instance), &Default::default())
        || !function.semantic.generic_layout_valid(
            function.params.iter().map(|param| param.ty),
            function.locals.iter().map(|local| local.ty),
            function.temps.iter().map(|temp| temp.ty),
            function.return_type,
        )
    {
        return Err(context.error(MirVerificationErrorKind::InvalidParameterLayout));
    }
    for (count, name) in [
        (function.params.len(), "parameters"),
        (function.locals.len(), "locals"),
        (function.temps.len(), "temporaries"),
    ] {
        context.limit(count, u16::MAX as usize, name)?;
    }
    context.limit(function.blocks.len(), u32::MAX as usize, "blocks")?;
    if function.entry.index() >= function.blocks.len() {
        return Err(context.error(MirVerificationErrorKind::InvalidBlock(function.entry)));
    }
    for (index, param) in function.params.iter().enumerate() {
        context.check_cancel()?;
        if param.local.index() != index {
            return Err(context.error(MirVerificationErrorKind::InvalidParameterLayout));
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
            return Err(context.error(MirVerificationErrorKind::InvalidDebugMetadata));
        }
        context.expect(
            context.local(function, local.local)?,
            local.ty,
            "debug local",
        )?;
        if local.is_parameter != (local.local.index() < function.params.len()) {
            return Err(context.error(MirVerificationErrorKind::InvalidDebugMetadata));
        }
    }
    if function
        .params
        .iter()
        .any(|param| !debug_local_ids.contains(&param.local))
    {
        return Err(context.error(MirVerificationErrorKind::InvalidDebugMetadata));
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
        return Err(context.error(MirVerificationErrorKind::InvalidDebugMetadata));
    }
    let mut scoped_locals = HashSet::new();
    for (index, scope) in scopes.iter().enumerate().skip(1) {
        context.check_cancel()?;
        let Some(local) = scope.local else {
            return Err(context.error(MirVerificationErrorKind::InvalidDebugMetadata));
        };
        if scope.parent.is_none_or(|parent| parent >= index)
            || local.index() < function.params.len()
            || !scoped_locals.insert(local)
            || !debug_local_ids.contains(&local)
        {
            return Err(context.error(MirVerificationErrorKind::InvalidDebugMetadata));
        }
    }
    debug::verify(function, context)?;
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
            return Err(context.error(MirVerificationErrorKind::InvalidDebugMetadata));
        }
        let terminator = block
            .terminator
            .as_ref()
            .ok_or_else(|| context.error(MirVerificationErrorKind::MissingTerminator))?;
        for (index, instruction) in block.instructions.iter().enumerate() {
            let context = Context {
                instruction: Some(index),
                span: Some(block.instruction_spans[index]),
                ..context
            };
            context.check_cancel()?;
            for value in instruction.inputs() {
                context.value(function, value)?;
            }
            if let Some(dst) = instruction.output() {
                context.value(function, dst)?;
            }
            operation::verify(module, function, instruction, context)?;
            effects = effects.union(instruction.effects());
        }
        let context = Context {
            span: block.terminator_span,
            ..context
        };
        for target in terminator.successors() {
            if target.index() >= function.blocks.len() {
                return Err(context.error(MirVerificationErrorKind::InvalidBlock(target)));
            }
        }
        match terminator {
            Terminator::Return(value) => {
                if function.return_type == ValueType::Never {
                    return Err(context.error(MirVerificationErrorKind::Contract(
                        ContractError::InvalidOperation {
                            reason: "Never function cannot return",
                        },
                    )));
                }
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
        return Err(context.error(MirVerificationErrorKind::MissingEffects));
    }
    flow::check_size(function, context)?;
    analysis::reserve(function, context, budget)?;
    let initialized = flow::verify(function, context, budget)?;
    analysis::build(function, initialized, context, budget)
}
