//! Checked static contracts and ordinary expression lowering for initializer bodies.
use crate::source::lower::{
    MirLoweringError,
    instances::{Instance, InstancePlanner},
    state::{FunctionLowerer, LoweringSignature},
};
use kagari_contract::statics::StaticContract;
use kagari_hir::{
    AnalyzedModule,
    declarations::DeclarationId,
    hir::{ids::StaticId, writeability::Writeability},
    resolver::resolved::ResolvedName,
    types::semantic::lower_type,
};
use kagari_mir::{function::MirFunction, instruction::Terminator};

pub(super) fn static_contract(module: &AnalyzedModule, id: StaticId) -> StaticContract {
    let item = module.lowered.module.static_item(id);
    let ty = &module.typed.statics[&id];
    let DeclarationId::Definition(declaration) = module
        .declarations
        .target(ResolvedName::Static(id))
        .expect("checked static declaration identity")
        .id
    else {
        unreachable!("static definition identity");
    };
    StaticContract {
        declaration,
        supplier: module.lowered.source.module_identity().clone(),
        name: item.name.clone(),
        ty: lower_type(
            &module
                .aggregates
                .canonical_value_type(module.declarations.definitions(), ty),
        ),
        mutable: item.writeability == Writeability::Var,
        visibility: item.visibility,
    }
}

pub(super) fn lower_initializer<'a>(
    module: &'a AnalyzedModule,
    id: StaticId,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let item = module.lowered.module.static_item(id);
    let signature = LoweringSignature {
        name: format!("$static_init_{}", item.name),
        params: Vec::new(),
        return_type: module.typed.statics[&id].clone(),
        bounds: Default::default(),
    };
    let mut lowerer = FunctionLowerer::new(module, &signature, instance, planner)?;
    let value = lowerer.lower_expr(item.initializer)?;
    if !lowerer.current_block_terminated() {
        lowerer.set_terminator(Terminator::Return(Some(value)));
    }
    lowerer.planner.check()?;
    lowerer.finish()
}
