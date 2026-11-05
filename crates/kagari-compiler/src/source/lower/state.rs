use crate::source::elaborate::model::CheckedBody;
use crate::source::lower::{
    MirLoweringError,
    debug::capture_origin,
    instances::{Instance, InstancePlanner},
};
use kagari_abi::representation::ValueType;
use kagari_common::span::Span;
use kagari_contract::effects::EffectSet;
use kagari_hir::{
    compile_input::{
        body::bindings::BindingId, environment::CheckedEnvironment, program::CheckedModule,
    },
    hir::ids::{LocalId as HirLocalId, ParamId},
    semantic::signatures::{TypedFunction, TypedParameter},
    typeck::GenericBounds,
    types::TypeId,
};
use kagari_mir::{
    debug::{MirFunctionDebugMetadata, MirLexicalScope, MirLocalDebugInfo},
    function::{BasicBlock, MirFunction, MirLocal, MirParameter, MirTemp, ParameterBuffer},
    ids::{BlockId, LocalId, TempId},
    instruction::{Instruction, MirValue, Terminator},
};
use kagari_source::source::SourceFile;
use kagari_types::ty::Ty;
use std::{
    collections::{HashMap, HashSet},
    slice,
};

/// Executable signature independent of whether the authored body is a function or static.
pub(super) struct LoweringSignature {
    pub name: String,
    pub params: Vec<TypedParameter>,
    pub return_type: TypeId,
    pub bounds: GenericBounds,
}

impl From<&TypedFunction> for LoweringSignature {
    fn from(function: &TypedFunction) -> Self {
        Self {
            name: function.name.clone(),
            params: function.params.to_vec(),
            return_type: function.return_type.clone(),
            bounds: function.bounds.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopScope {
    pub(crate) break_block: BlockId,
    pub(crate) continue_block: BlockId,
    pub(crate) break_value: Option<MirValue>,
}

pub(crate) struct FunctionLowerer<'a, 'p> {
    pub(crate) environment: CheckedEnvironment<'a>,
    pub(crate) source: &'a SourceFile,
    /// Generated adapters have no authored operation tree.
    pub(crate) body: Option<CheckedBody>,
    pub(crate) instance: Instance,
    pub(crate) implementation_bounds: GenericBounds,
    pub(crate) planner: &'p mut InstancePlanner<'a>,
    pub(crate) function: MirFunction,
    pub(crate) current_block: BlockId,

    pub(crate) params: HashMap<ParamId, LocalId>,
    pub(crate) locals: HashMap<HirLocalId, LocalId>,
    pub(crate) cell_locals: HashSet<HirLocalId>,
    pub(crate) loops: Vec<LoopScope>,
    pub(crate) effects: EffectSet,
    pub(crate) current_scope: usize,
    current_debug_span: Option<Span>,
}

impl<'a, 'p> FunctionLowerer<'a, 'p> {
    pub(crate) fn new(
        module: &'a CheckedModule<'a>,
        signature: &LoweringSignature,
        body: Option<CheckedBody>,
        instance: Instance,
        planner: &'p mut InstancePlanner<'a>,
    ) -> Result<Self, MirLoweringError> {
        if body.as_ref().is_some_and(|body| {
            body.key.module != module.handle() || body.key.owner != instance.body.owner()
        }) {
            return Err(MirLoweringError::MissingBinding(
                "expanded body belongs to another instance owner",
            ));
        }
        let entry = BlockId::new(0);
        let span = instance.body.span(module);
        let value_type = |ty| planner.value_type(ty, &instance.substitution, span);
        let mut function = MirFunction {
            semantic: Default::default(),
            id: instance.id,
            instance: instance.lower(
                planner.catalog,
                planner.options,
                &planner.definitions(),
                span,
            )?,
            name: if instance.key.arguments.is_empty() {
                signature.name.clone()
            } else {
                format!("{}#{}", signature.name, instance.id.index())
            },
            params: ParameterBuffer::new(),
            return_type: value_type(&signature.return_type)?,
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![BasicBlock {
                instructions: Vec::new(),
                instruction_spans: Vec::new(),
                instruction_scopes: Vec::new(),
                terminator: None,
                terminator_span: None,
                terminator_scope: None,
            }],
            entry,
            effects: EffectSet::default(),
            debug: MirFunctionDebugMetadata {
                source: None,
                source_module: Some(module.source().module_identity().clone()),
                source_span: span,
                locals: Vec::new(),
                captured_bindings: Vec::new(),
                lexical_scopes: vec![MirLexicalScope {
                    parent: None,
                    local: None,
                }],
            },
        };

        let input = module.body_for(instance.body.owner());
        let cell_locals = body
            .as_ref()
            .map(|body| body.cell_locals.clone())
            .unwrap_or_default();
        let mut params = HashMap::new();
        for param in &signature.params {
            let local = LocalId::new(function.locals.len());
            function.locals.push(MirLocal {
                name: param.name.clone(),
                ty: value_type(&param.ty)?,
            });
            function.debug.locals.push(MirLocalDebugInfo {
                local,
                name: param.name.clone(),
                span: input
                    .and_then(|input| input.binding(BindingId::Parameter(param.id)))
                    .ok_or(MirLoweringError::MissingBinding("parameter source binding"))?
                    .span,
                ty: value_type(&param.ty)?,
                is_parameter: true,
            });
            function.params.push(MirParameter {
                name: param.name.clone(),
                ty: value_type(&param.ty)?,
                local,
            });
            params.insert(param.id, local);
        }

        let concrete = |ty: &TypeId| {
            let ty = planner
                .arguments(slice::from_ref(ty), &instance.substitution, span)?
                .remove(0);
            Ok::<_, MirLoweringError>(planner.encode_type(&ty))
        };
        function.semantic.result = Some(concrete(&signature.return_type)?);
        let scope_bounds = if instance.closure.is_some() {
            instance
                .body
                .function()
                .and_then(|id| {
                    module
                        .environment()
                        .signatures()
                        .functions()
                        .iter()
                        .find(|function| function.id == id)
                })
                .map_or(&signature.bounds, |function| &function.bounds)
        } else {
            &signature.bounds
        };
        function.semantic.generic =
            instance.shared_body(scope_bounds, planner.catalog, &planner.definitions());
        for (index, param) in signature.params.iter().enumerate() {
            function.semantic.params.insert(index, concrete(&param.ty)?);
            function.semantic.locals.insert(index, concrete(&param.ty)?);
        }
        let implementation_bounds =
            planner.implementation_bounds(scope_bounds, &instance.substitution)?;
        Ok(Self {
            environment: module.environment(),
            source: module.source(),
            body,
            instance,
            implementation_bounds,
            planner,
            function,
            current_block: entry,

            params,
            locals: HashMap::new(),
            cell_locals,
            loops: Vec::new(),
            effects: EffectSet::default(),
            current_scope: 0,
            current_debug_span: None,
        })
    }

    /// Authored operations are absent only for planner-generated adapter recipes.
    pub(crate) fn source_body(&self) -> &CheckedBody {
        self.body
            .as_ref()
            .expect("authored operation lowering requires a source body")
    }

    pub(crate) fn semantic_type(&self, ty: &TypeId) -> Result<Ty, MirLoweringError> {
        let concrete = self
            .planner
            .arguments(
                slice::from_ref(ty),
                &self.instance.substitution,
                self.function.debug.source_span,
            )?
            .remove(0);
        Ok(self.planner.encode_type(&concrete))
    }

    pub(crate) fn finish(mut self) -> Result<MirFunction, MirLoweringError> {
        self.function.effects = self.effects;
        self.function.debug.source = Some(capture_origin(
            &self.function,
            self.source,
            &self.planner.options.cancel,
        )?);
        Ok(self.function)
    }

    pub(crate) fn new_block(&mut self) -> BlockId {
        let id = BlockId::new(self.function.blocks.len());
        self.function.blocks.push(BasicBlock {
            instructions: Vec::new(),
            instruction_spans: Vec::new(),
            instruction_scopes: Vec::new(),
            terminator: None,
            terminator_span: None,
            terminator_scope: None,
        });
        id
    }

    pub(crate) fn switch_to_block(&mut self, block: BlockId) {
        self.current_block = block;
    }

    /// All incoming edges to a structured join have been emitted at this point.
    pub(crate) fn switch_to_join(&mut self, block: BlockId) {
        let has_predecessor =
            self.function
                .blocks
                .iter()
                .any(|candidate| match candidate.terminator.as_ref() {
                    Some(Terminator::Jump(target)) => *target == block,
                    Some(Terminator::Branch {
                        then_block,
                        else_block,
                        ..
                    }) => *then_block == block || *else_block == block,
                    _ => false,
                });
        self.switch_to_block(block);
        if !has_predecessor {
            self.set_terminator(Terminator::Unreachable);
        }
    }

    pub(crate) fn current_block_terminated(&self) -> bool {
        self.function.blocks[self.current_block.index()]
            .terminator
            .is_some()
    }

    pub(crate) fn emit(&mut self, instruction: Instruction) {
        self.planner.charge_instruction(
            self.current_debug_span
                .unwrap_or(self.function.debug.source_span),
        );
        if self.planner.check().is_err() {
            return;
        }
        self.effects = self.effects.union(instruction.effects());
        let block = &mut self.function.blocks[self.current_block.index()];
        block
            .instruction_spans
            .push(self.current_debug_span.unwrap_or_default());
        block.instruction_scopes.push(self.current_scope);
        block.instructions.push(instruction);
    }

    pub(crate) fn set_terminator(&mut self, terminator: Terminator) {
        self.planner.charge_instruction(
            self.current_debug_span
                .unwrap_or(self.function.debug.source_span),
        );
        if self.planner.check().is_err() {
            return;
        }
        self.effects = self.effects.union(terminator.effects());
        let block = &mut self.function.blocks[self.current_block.index()];
        block.terminator = Some(terminator);
        block.terminator_span = self.current_debug_span;
        block.terminator_scope = Some(self.current_scope);
    }

    pub(crate) fn introduce_debug_local(&mut self, local: LocalId) {
        let scope = self.function.debug.lexical_scopes.len();
        self.function.debug.lexical_scopes.push(MirLexicalScope {
            parent: Some(self.current_scope),
            local: Some(local),
        });
        self.current_scope = scope;
    }

    pub(crate) fn ensure_jump(&mut self, target: BlockId) {
        if !self.current_block_terminated() {
            self.set_terminator(Terminator::Jump(target));
        }
    }

    pub(crate) fn alloc_temp(&mut self, ty: ValueType) -> MirValue {
        let id = TempId::new(self.function.temps.len());
        self.function.temps.push(MirTemp { ty });
        MirValue { temp: id, ty }
    }

    pub(crate) fn alloc_local(&mut self, name: String, ty: ValueType, span: Span) -> LocalId {
        let id = LocalId::new(self.function.locals.len());
        self.function.locals.push(MirLocal {
            name: name.clone(),
            ty,
        });
        self.function.debug.locals.push(MirLocalDebugInfo {
            local: id,
            name,
            span,
            ty,
            is_parameter: false,
        });
        id
    }

    pub(crate) fn with_debug_span<R>(&mut self, span: Span, f: impl FnOnce(&mut Self) -> R) -> R {
        let previous = self.current_debug_span.replace(span);
        let result = f(self);
        self.current_debug_span = previous;
        result
    }

    pub(crate) fn value_type(&self, ty: &TypeId) -> Result<ValueType, MirLoweringError> {
        self.planner.value_type(
            ty,
            &self.instance.substitution,
            self.current_debug_span
                .unwrap_or(self.function.debug.source_span),
        )
    }
}
