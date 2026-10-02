use crate::source::{lower::instances::MirLoweringOptions, types::raise_type};
use instances::InstancePlanner;
use kagari_abi::{
    host as module_host,
    types::{AbiType, ConcreteFunctionIdentity},
};
use kagari_common::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_hir::{
    CheckedAnalysis,
    aggregates::AggregateCatalog,
    hir::{
        ids::{ExprId, FunctionId, LocalId, PlaceId},
        item::function::FunctionKind,
    },
    imports::ImportTarget,
    typeck::FunctionImplementation,
};
use kagari_mir::{
    function::MirModule,
    passes::optimize,
    verify::{MirVerificationError, MirVerificationErrorKind, VerifiedMirModule, verify_mir},
};
use std::{
    collections::{BTreeSet, HashSet},
    slice,
};
mod abi;
mod debug;
mod expr;
mod function;
mod host;
mod host_interfaces;
pub mod instances;
mod layouts;
mod place;
mod state;
mod stmt;
mod support;

#[derive(Debug)]
pub enum MirLoweringError {
    Verification(MirVerificationError),
    Diagnostic(Box<Diagnostic>),
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

impl MirLoweringError {
    pub(crate) fn diagnostic(diagnostic: Diagnostic) -> Self {
        Self::Diagnostic(Box::new(diagnostic))
    }
}

pub fn lower_to_mir(
    module: &CheckedAnalysis,
    options: &MirLoweringOptions,
) -> Result<VerifiedMirModule, MirLoweringError> {
    lower_to_mir_with_requests(
        module,
        options,
        &[],
        slice::from_ref(module),
        &module.aggregates,
    )
}

pub(crate) fn lower_to_mir_with_requests<'a>(
    module: &'a CheckedAnalysis,
    options: &'a MirLoweringOptions,
    requests: &[ConcreteFunctionIdentity],
    modules: &'a [CheckedAnalysis],
    catalog: &'a AggregateCatalog,
) -> Result<VerifiedMirModule, MirLoweringError> {
    let mut planner = InstancePlanner::new(module, options, modules, catalog);
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
        .collect::<HashSet<_>>();
    for function in &module.lowered.module.functions {
        if (matches!(function.kind, FunctionKind::User) || callable_methods.contains(&function.id))
            && function.generic_params.is_empty()
        {
            let signature = module
                .typed
                .functions
                .iter()
                .find(|signature| signature.id == function.id)
                .ok_or(MirLoweringError::MissingTypedFunction(function.id))?;
            if matches!(signature.implementation, FunctionImplementation::Native(_)) {
                continue;
            }
            planner.enqueue(
                function.id,
                Vec::new(),
                module.lowered.source_map.function_span(function.id),
            )?;
        }
    }
    for implementation in module.aggregates.implementations().filter(|item| {
        item.id.module == *module.lowered.source.module_identity() && item.generic_params.is_empty()
    }) {
        planner.record_interface(&implementation.id, &[], Default::default())?;
    }
    for request in requests {
        planner.check()?;
        if request.declaration.module != *module.lowered.source.module_identity() {
            return Err(MirLoweringError::MissingBinding("requested instance owner"));
        }
        if request.arguments.iter().all(AbiType::is_concrete)
            && planner.prepare_native_target(
                &request.declaration,
                &request.arguments.iter().map(raise_type).collect::<Vec<_>>(),
                Default::default(),
            )?
        {
            continue;
        }
        planner.enqueue_interface_method(
            &request.declaration,
            &request.arguments.iter().map(raise_type).collect::<Vec<_>>(),
            Default::default(),
        )?;
    }
    let mut functions = Vec::new();
    while let Some(instance) = planner.instances.get(functions.len()).cloned() {
        planner.check()?;
        let origin = planner.origin(&instance);
        let function = origin
            .lowered
            .module
            .functions
            .iter()
            .find(|function| function.id == instance.function)
            .ok_or(MirLoweringError::MissingTypedFunction(instance.function))?;
        functions.push(if instance.callable.is_some() {
            function::lower_callable(origin, function, instance, &mut planner)?
        } else if instance.protocol.is_some() {
            function::lower_protocol(origin, function, instance, &mut planner)?
        } else if let Some(closure) = instance.closure {
            function::lower_closure(origin, function, closure, instance, &mut planner)?
        } else {
            function::lower_function(origin, function, instance, &mut planner)?
        });
    }

    let (structures, enumerations) = layouts::collect(module, &mut planner, &functions)?;
    let mut abi = abi::collect_module_abi(module);
    host_interfaces::collect(&mut planner, module, &mut abi, &mut functions)?;
    planner.host_types.extend(
        module_host::references(
            &abi.public_items,
            &structures,
            &enumerations,
            &options.cancel,
        )
        .map_err(|_| MirLoweringError::Cancelled)?,
    );
    let host_types = host::collect(
        &module.names.hosts,
        planner.host_types,
        &functions,
        &options.cancel,
    )?;

    verify_mir(
        MirModule {
            native_targets: planner.native_targets,
            interface_instances: planner.interface_instances,
            host_types,
            dependencies: module
                .names
                .imports
                .entries
                .iter()
                .filter_map(|import| {
                    if let Some(ImportTarget::Source(target)) = &import.target {
                        Some(target.module.clone())
                    } else {
                        None
                    }
                })
                .collect::<BTreeSet<_>>()
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
    .and_then(|module| match &options.optimization {
        Some(passes) => optimize(module, passes, &options.cancel).map(|result| result.module),
        None => Ok(module),
    })
    .map_err(|error| match error.kind {
        MirVerificationErrorKind::Cancelled => MirLoweringError::Cancelled,
        MirVerificationErrorKind::Limit { resource, limit } => MirLoweringError::diagnostic(
            Diagnostic::error(DiagnosticKind::CompileLimitExceeded { resource, limit })
                .with_span(error.span.unwrap_or_default()),
        ),
        _ => MirLoweringError::Verification(error),
    })
}
