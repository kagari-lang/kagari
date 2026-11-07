//! Checked static contracts and ordinary expression lowering for initializer bodies.

use crate::source::elaborate::elaborate;
use crate::source::lower::{
    MirLoweringError,
    instances::{Instance, InstancePlanner},
    state::{FunctionLowerer, LoweringSignature},
};
use kagari_contract::statics::StaticContract;
use kagari_hir::{
    compile_input::program::CheckedModule,
    hir::{ids::StaticId, writeability::Writeability},
    resolver::resolved::ResolvedName,
    types::semantic::lower_type,
};
use kagari_mir::{function::MirFunction, instruction::Terminator};

pub(super) fn static_contract(module: &CheckedModule<'_>, id: StaticId) -> StaticContract {
    let item = &module.headers().statics[id.index()];
    let environment = module.environment();
    let ty = environment
        .signatures()
        .static_type(id)
        .expect("admitted static type");
    let declaration = environment
        .definition(ResolvedName::Static(id))
        .expect("admitted static identity");
    StaticContract {
        declaration,
        supplier: module.source().module_identity().clone(),
        name: item.name.clone(),
        ty: lower_type(
            &environment
                .aggregates()
                .canonical_value_type(environment.definitions(), ty),
        ),
        mutable: item.writeability == Writeability::Var,
        visibility: item.visibility,
    }
}

pub(super) fn lower_initializer<'a>(
    module: &'a CheckedModule<'a>,
    id: StaticId,
    instance: Instance,
    planner: &mut InstancePlanner<'a>,
) -> Result<MirFunction, MirLoweringError> {
    let item = &module.headers().statics[id.index()];
    let signature = LoweringSignature {
        name: format!("$static_init_{}", item.name),
        params: Vec::new(),
        return_type: module.environment().signatures().statics()[&id].clone(),
        bounds: Default::default(),
    };
    let input = module
        .body_for(instance.body.owner())
        .ok_or(MirLoweringError::MissingBinding(
            "admitted initializer body",
        ))?;
    let body = elaborate(input, &planner.options.cancel)?;
    let mut lowerer = FunctionLowerer::new(module, &signature, Some(body), instance, planner)?;
    let value = lowerer.lower_expr(item.initializer)?;
    if !lowerer.current_block_terminated() {
        lowerer.set_terminator(Terminator::Return(Some(value)));
    }
    lowerer.planner.check()?;
    lowerer.finish()
}
