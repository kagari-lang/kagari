use std::collections::HashMap;

use kagari_common::Span;
use kagari_hir::typeck::TypedFunction;
use kagari_hir::{AnalyzedModule, hir};

use crate::module::{
    function::{
        BasicBlock, IrFunction, IrFunctionDebugMetadata, IrLocal, IrLocalDebugInfo, IrParameter,
        IrTemp, ParameterBuffer,
    },
    ids::{BlockId, LocalId, TempId},
    instruction::{EffectSet, Instruction, IrValue, Terminator},
    types::ValueType,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopScope {
    pub(crate) break_block: BlockId,
    pub(crate) continue_block: BlockId,
}

pub(crate) struct FunctionLowerer<'a, 'p> {
    pub(crate) analyzed: &'a AnalyzedModule,
    pub(crate) instance: super::instances::Instance,
    pub(crate) planner: &'p mut super::instances::InstancePlanner<'a>,
    pub(crate) function: IrFunction,
    pub(crate) current_block: BlockId,

    pub(crate) params: HashMap<hir::ParamId, LocalId>,
    pub(crate) locals: HashMap<hir::LocalId, LocalId>,
    pub(crate) loops: Vec<LoopScope>,
    pub(crate) effects: EffectSet,
    current_debug_span: Option<Span>,
}

impl<'a, 'p> FunctionLowerer<'a, 'p> {
    pub(crate) fn new(
        analyzed: &'a AnalyzedModule,
        hir_function: &hir::Function,
        typed_function: &TypedFunction,
        instance: super::instances::Instance,
        planner: &'p mut super::instances::InstancePlanner<'a>,
    ) -> Result<Self, super::IrLoweringError> {
        let entry = BlockId::new(0);
        let span = analyzed.lowered.source_map.function_span(hir_function.id);
        let value_type = |ty| planner.value_type(ty, &instance.substitution, span);
        let mut function = IrFunction {
            id: instance.id,
            instance: instance.key.clone(),
            name: if instance.key.arguments.is_empty() {
                hir_function.name.clone()
            } else {
                format!("{}#{}", hir_function.name, instance.id.index())
            },
            params: ParameterBuffer::new(),
            return_type: value_type(&typed_function.return_type)?,
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![BasicBlock {
                instructions: Vec::new(),
                instruction_spans: Vec::new(),
                terminator: None,
                terminator_span: None,
            }],
            entry,
            effects: EffectSet::default(),
            debug: IrFunctionDebugMetadata {
                source_span: analyzed.lowered.source_map.function_span(hir_function.id),
                locals: Vec::new(),
                captured_bindings: Vec::new(),
            },
        };

        let mut params = HashMap::new();
        for param in &typed_function.params {
            let local = LocalId::new(function.locals.len());
            function.locals.push(IrLocal {
                name: param.name.clone(),
                ty: value_type(&param.ty)?,
            });
            function.debug.locals.push(IrLocalDebugInfo {
                local,
                name: param.name.clone(),
                span: analyzed.lowered.source_map.param_span(param.id),
                ty: value_type(&param.ty)?,
                is_parameter: true,
            });
            function.params.push(IrParameter {
                name: param.name.clone(),
                ty: value_type(&param.ty)?,
                local,
            });
            params.insert(param.id, local);
        }

        Ok(Self {
            analyzed,
            instance,
            planner,
            function,
            current_block: entry,

            params,
            locals: HashMap::new(),
            loops: Vec::new(),
            effects: EffectSet::default(),
            current_debug_span: None,
        })
    }

    pub(crate) fn finish(mut self) -> IrFunction {
        self.function.effects = self.effects;
        self.function
    }

    pub(crate) fn new_block(&mut self) -> BlockId {
        let id = BlockId::new(self.function.blocks.len());
        self.function.blocks.push(BasicBlock {
            instructions: Vec::new(),
            instruction_spans: Vec::new(),
            terminator: None,
            terminator_span: None,
        });
        id
    }

    pub(crate) fn switch_to_block(&mut self, block: BlockId) {
        self.current_block = block;
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
    }

    pub(crate) fn ensure_jump(&mut self, target: BlockId) {
        if !self.current_block_terminated() {
            self.set_terminator(Terminator::Jump(target));
        }
    }

    pub(crate) fn alloc_temp(&mut self, ty: ValueType) -> IrValue {
        let id = TempId::new(self.function.temps.len());
        self.function.temps.push(IrTemp { ty });
        IrValue { temp: id, ty }
    }

    pub(crate) fn alloc_local(&mut self, name: String, ty: ValueType, span: Span) -> LocalId {
        let id = LocalId::new(self.function.locals.len());
        self.function.locals.push(IrLocal {
            name: name.clone(),
            ty,
        });
        self.function.debug.locals.push(IrLocalDebugInfo {
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

    pub(crate) fn value_type(
        &self,
        ty: &kagari_hir::types::TypeId,
    ) -> Result<ValueType, super::IrLoweringError> {
        self.planner.value_type(
            ty,
            &self.instance.substitution,
            self.current_debug_span
                .unwrap_or(self.function.debug.source_span),
        )
    }
}
