mod abi;
mod expr;
mod function;
mod instances;
mod place;
mod state;
mod stmt;
mod support;

use crate::module::{IrModule, IrVerificationErrorKind, VerifiedIrModule, verify_ir};
pub use instances::IrLoweringOptions;
use kagari_hir::hir::{ExprId, FunctionId, FunctionKind, LocalId, PlaceId};

#[derive(Debug)]
pub enum IrLoweringError {
    Verification(crate::module::IrVerificationError),
    Diagnostic(Box<kagari_common::Diagnostic>),
    Cancelled,
    MissingTypedFunction(FunctionId),
    MissingExprType(ExprId),
    MissingLocalType(LocalId),
    UnresolvedExpr(ExprId),
    UnresolvedPlace(PlaceId),
    MissingBinding(&'static str),
    UnsupportedExpr(&'static str),
    UnsupportedStatement(&'static str),
    InvalidLoopControl,
}

impl IrLoweringError {
    pub(crate) fn diagnostic(diagnostic: kagari_common::Diagnostic) -> Self {
        Self::Diagnostic(Box::new(diagnostic))
    }
}

pub fn lower_to_ir(
    module: &kagari_hir::CheckedAnalysis,
    options: &IrLoweringOptions,
) -> Result<VerifiedIrModule, IrLoweringError> {
    let mut planner = instances::InstancePlanner::new(module, options);
    planner.check()?;
    let mut module_init = None;
    for function in &module.lowered.module.functions {
        if matches!(function.kind, FunctionKind::User | FunctionKind::ModuleInit)
            && function.generic_params.is_empty()
        {
            let id = planner.enqueue(
                function.id,
                Vec::new(),
                module.lowered.source_map.function_span(function.id),
            )?;
            if Some(function.id) == module.lowered.module.module_init {
                module_init = Some(id);
            }
        }
    }
    let mut functions = Vec::new();
    while let Some(instance) = planner.instances.get(functions.len()).cloned() {
        planner.check()?;
        let function = module
            .lowered
            .module
            .functions
            .iter()
            .find(|function| function.id == instance.function)
            .ok_or(IrLoweringError::MissingTypedFunction(instance.function))?;
        functions.push(function::lower_function(
            module,
            function,
            instance,
            &mut planner,
        )?);
    }

    verify_ir(
        IrModule {
            identity: module.lowered.source.module_identity().clone(),
            source_name: module.lowered.source.name().to_owned(),
            module_init,
            module_slots: Vec::new(),
            abi: abi::collect_module_abi(module),
            functions,
        },
        &options.cancel,
    )
    .map_err(|error| match error.kind {
        IrVerificationErrorKind::Cancelled => IrLoweringError::Cancelled,
        IrVerificationErrorKind::Limit { resource, limit } => IrLoweringError::diagnostic(
            kagari_common::Diagnostic::error(kagari_common::DiagnosticKind::CompileLimitExceeded {
                resource,
                limit,
            })
            .with_span(error.span.unwrap_or_default()),
        ),
        _ => IrLoweringError::Verification(error),
    })
}
