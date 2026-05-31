use std::collections::HashMap;

use kagari_hir::typeck::TypedFunction;
use kagari_hir::{AnalyzedModule, hir};

use crate::lower::EvaluatedConst;
use crate::module::{
    function::{BasicBlock, IrFunction, IrLocal, IrParameter, IrTemp, ParameterBuffer},
    ids::{BlockId, LocalId, TempId},
    instruction::{EffectSet, Instruction, IrValue, Terminator},
    types::ValueType,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopScope {
    pub(crate) break_block: BlockId,
    pub(crate) continue_block: BlockId,
}

pub(crate) struct FunctionLowerer<'a> {
    pub(crate) analyzed: &'a AnalyzedModule,
    pub(crate) function: IrFunction,
    pub(crate) current_block: BlockId,
    pub(crate) const_values: &'a HashMap<hir::ConstId, EvaluatedConst>,
    pub(crate) params: HashMap<hir::ParamId, LocalId>,
    pub(crate) locals: HashMap<hir::LocalId, LocalId>,
    pub(crate) loops: Vec<LoopScope>,
    pub(crate) effects: EffectSet,
}

impl<'a> FunctionLowerer<'a> {
    pub(crate) fn new(
        analyzed: &'a AnalyzedModule,
        hir_function: &'a hir::Function,
        typed_function: &'a TypedFunction,
        const_values: &'a HashMap<hir::ConstId, EvaluatedConst>,
    ) -> Self {
        let entry = BlockId::new(0);
        let mut function = IrFunction {
            hir_id: hir_function.id,
            name: hir_function.name.clone(),
            params: ParameterBuffer::new(),
            return_type: ValueType::from_type_id(&typed_function.return_type),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![BasicBlock {
                instructions: Vec::new(),
                terminator: None,
            }],
            entry,
            effects: EffectSet::default(),
        };

        let mut params = HashMap::new();
        for param in &typed_function.params {
            let local = LocalId::new(function.locals.len());
            function.locals.push(IrLocal {
                name: param.name.clone(),
                ty: ValueType::from_type_id(&param.ty),
            });
            function.params.push(IrParameter {
                name: param.name.clone(),
                ty: ValueType::from_type_id(&param.ty),
                local,
            });
            params.insert(param.id, local);
        }

        Self {
            analyzed,
            function,
            current_block: entry,
            const_values,
            params,
            locals: HashMap::new(),
            loops: Vec::new(),
            effects: EffectSet::default(),
        }
    }

    pub(crate) fn finish(mut self) -> IrFunction {
        self.function.effects = self.effects;
        self.function
    }

    pub(crate) fn new_block(&mut self) -> BlockId {
        let id = BlockId::new(self.function.blocks.len());
        self.function.blocks.push(BasicBlock {
            instructions: Vec::new(),
            terminator: None,
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
        self.effects = self.effects.union(instruction.effects());
        self.function.blocks[self.current_block.index()]
            .instructions
            .push(instruction);
    }

    pub(crate) fn set_terminator(&mut self, terminator: Terminator) {
        self.effects = self.effects.union(terminator.effects());
        self.function.blocks[self.current_block.index()].terminator = Some(terminator);
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

    pub(crate) fn alloc_local(&mut self, name: String, ty: ValueType) -> LocalId {
        let id = LocalId::new(self.function.locals.len());
        self.function.locals.push(IrLocal { name, ty });
        id
    }
}
