mod abi;
mod expr;
mod function;
mod host;
mod instances;
mod layouts;
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
    lower_to_ir_with_requests(module, options, &[])
}

pub(crate) fn lower_to_ir_with_requests(
    module: &kagari_hir::CheckedAnalysis,
    options: &IrLoweringOptions,
    requests: &[crate::module::function::FunctionInstance],
) -> Result<VerifiedIrModule, IrLoweringError> {
    let mut planner = instances::InstancePlanner::new(module, options);
    planner.check()?;
    let callable_methods = module
        .lowered
        .module
        .impls
        .iter()
        .filter(|implementation| {
            implementation.trait_ref.is_none() || implementation.generic_params.is_empty()
        })
        .flat_map(|implementation| implementation.methods.iter().map(|method| method.function))
        .collect::<std::collections::HashSet<_>>();
    for function in &module.lowered.module.functions {
        if (matches!(function.kind, FunctionKind::User) || callable_methods.contains(&function.id))
            && function.generic_params.is_empty()
        {
            planner.enqueue(
                function.id,
                Vec::new(),
                module.lowered.source_map.function_span(function.id),
            )?;
        }
    }
    for request in requests {
        planner.check()?;
        if request.declaration.module != *module.lowered.source.module_identity() {
            return Err(IrLoweringError::MissingBinding("requested instance owner"));
        }
        let Some(kagari_hir::resolver::ResolvedName::Function(function)) =
            module.declarations.definition_target(&request.declaration)
        else {
            return Err(IrLoweringError::MissingBinding(
                "requested function instance",
            ));
        };
        planner.enqueue(
            function,
            request.arguments.clone(),
            module.lowered.source_map.function_span(function),
        )?;
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
        functions.push(if let Some(closure) = instance.closure {
            function::lower_closure(module, function, closure, instance, &mut planner)?
        } else {
            function::lower_function(module, function, instance, &mut planner)?
        });
    }

    let (structures, enumerations) = layouts::collect(module, &mut planner)?;
    let abi = abi::collect_module_abi(module);
    planner.host_types.extend(
        crate::module::host::references(
            &abi.public_items,
            &structures,
            &enumerations,
            &options.cancel,
        )
        .map_err(|_| IrLoweringError::Cancelled)?,
    );
    let host_types = host::collect(
        &module.names.hosts,
        planner.host_types,
        &functions,
        &options.cancel,
    )?;

    verify_ir(
        IrModule {
            host_types,
            dependencies: module
                .names
                .imports
                .entries
                .iter()
                .filter_map(|import| {
                    if let Some(kagari_hir::imports::ImportTarget::Source(target)) = &import.target
                    {
                        Some(target.module.clone())
                    } else {
                        None
                    }
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
            structures,
            enumerations,
            identity: module.lowered.source.module_identity().clone(),
            source_name: module.lowered.source.name().to_owned(),
            module_slots: Vec::new(),
            abi,
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
