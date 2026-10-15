use crate::source::lower::instances::MirLoweringOptions;
use instances::InstancePlanner;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionPath, map::DefinitionContext, mapping::DefinitionMappingError,
        metadata::DefinitionMetadata,
    },
};
use kagari_contract::{
    host as module_host,
    types::{ConcreteFunctionIdentity, ModuleContract, identity},
};
use kagari_hir::{
    AnalyzedModule, CheckedAnalysis,
    aggregates::AggregateCatalog,
    hir::{
        ids::{ExprId, FunctionId, LocalId, PlaceId},
        item::function::FunctionKind,
    },
    native::NativeBinding,
    typeck::FunctionImplementation,
    types::semantic::raise_type,
};
use kagari_mir::{
    function::MirModule,
    passes::optimize,
    verify::{MirVerificationError, MirVerificationErrorKind, VerifiedMirModule, verify_mir},
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::ty::Ty;
use std::{collections::HashSet, iter, slice};

mod abi;
mod debug;
mod expr;
mod function;
mod future;
mod host;
pub mod instances;
mod interface_bridges;
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
    Identity(DefinitionMappingError),
    MissingTypedFunction(FunctionId),
    MissingExprType(ExprId),
    MissingLocalType(LocalId),
    UnresolvedExpr(ExprId),
    UnresolvedPlace(PlaceId),
    MissingBinding(&'static str),
    MissingHostDeclaration(DefinitionPath),
    UnsupportedExpr(&'static str),
    UnsupportedStatement(&'static str),
    InvalidLoopControl,
}

impl MirLoweringError {
    pub(crate) fn diagnostic(diagnostic: Diagnostic) -> Self {
        Self::Diagnostic(Box::new(diagnostic))
    }
}

impl From<DefinitionMappingError> for MirLoweringError {
    fn from(error: DefinitionMappingError) -> Self {
        match error {
            DefinitionMappingError::Cancelled => Self::Cancelled,
            error => Self::Identity(error),
        }
    }
}

/// Project a checked source module's declarations for source-independent products.
pub fn module_contract(
    module: &CheckedAnalysis,
    cancel: &CancellationToken,
) -> Result<DefinitionMetadata<ModuleContract>, DefinitionMappingError> {
    cancel
        .check()
        .map_err(|_| DefinitionMappingError::Cancelled)?;
    DefinitionMetadata::checked(
        module.definitions().clone(),
        abi::collect_module_abi(module),
        cancel,
    )
}

pub fn lower_to_mir(
    module: &CheckedAnalysis,
    options: &MirLoweringOptions,
) -> Result<VerifiedMirModule, MirLoweringError> {
    let context = module.definition_context();
    let module = module
        .to_unverified(&options.cancel)
        .map_err(MirLoweringError::from)?
        .into_records();
    lower_to_mir_with_requests(
        &module,
        options,
        &[],
        slice::from_ref(&module),
        &module.aggregates,
        &context,
    )
}

pub(crate) fn lower_to_mir_with_requests<'a>(
    module: &'a AnalyzedModule,
    options: &'a MirLoweringOptions,
    requests: &[ConcreteFunctionIdentity],
    modules: &'a [AnalyzedModule],
    catalog: &'a AggregateCatalog,
    context: &'a DefinitionContext,
) -> Result<VerifiedMirModule, MirLoweringError> {
    identity::collect_language(context, &options.cancel)?;
    let mut planner = InstancePlanner::new(module, options, modules, catalog, context);
    planner.check()?;
    for function in &module.typed.functions {
        if matches!(
            function.implementation,
            FunctionImplementation::Native(NativeBinding::Default(_))
        ) && module
            .typed
            .type_table
            .native_default_call(function.id)
            .is_none()
        {
            return Err(MirLoweringError::MissingBinding(
                "checked native default body",
            ));
        }
    }
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
    let input_definitions = context.snapshot();
    let mut implementations: Vec<_> = module
        .aggregates
        .implementations()
        .filter(|item| {
            input_definitions
                .resolve(item.id)
                .expect("checked implementation")
                .module()
                == module.lowered.source.module_identity()
                && item.generic_params.is_empty()
        })
        .collect();
    implementations.sort_by(|left, right| {
        input_definitions
            .compare(left.id, right.id)
            .expect("checked implementation identities")
    });
    for implementation in implementations {
        planner.record_interface(&implementation.id, &[], Default::default())?;
    }
    for request in requests {
        planner.check()?;
        if planner.module_identity(request.declaration) != *module.lowered.source.module_identity()
        {
            return Err(MirLoweringError::MissingBinding("requested instance owner"));
        }
        if planner
            .catalog
            .implementation_signature(&request.declaration)
            .is_some()
        {
            planner.record_interface(
                &request.declaration,
                &request.arguments.iter().map(raise_type).collect::<Vec<_>>(),
                Default::default(),
            )?;
            continue;
        }
        if request.arguments.iter().all(Ty::is_concrete)
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

    let layouts::Layouts {
        structures,
        enumerations,
    } = layouts::collect(module, &mut planner, &functions)?;
    let mut abi = abi::collect_module_abi(module);
    interface_bridges::collect(&mut planner, module, &mut abi, &mut functions)?;
    planner.host_types.extend(
        module_host::references(
            &abi.public_items,
            &structures,
            &enumerations,
            &options.cancel,
        )
        .map_err(|_| MirLoweringError::Cancelled)?,
    );
    planner.host_types.extend(
        module_host::type_references(
            abi.native_declarations
                .iter()
                .flat_map(|declaration| {
                    declaration
                        .function
                        .params
                        .iter()
                        .map(|param| &param.ty)
                        .chain(iter::once(&declaration.function.return_type))
                })
                .chain(planner.native_targets.iter().flat_map(|import| {
                    import
                        .signature
                        .params
                        .iter()
                        .chain(iter::once(&import.signature.result))
                }))
                .chain(functions.iter().flat_map(|function| {
                    function
                        .semantic
                        .params
                        .values()
                        .chain(function.semantic.result.iter())
                        .chain(function.semantic.locals.values())
                        .chain(function.semantic.registers.values())
                })),
            &options.cancel,
        )
        .map_err(|_| MirLoweringError::Cancelled)?,
    );
    let external_types = host::collect(
        module,
        modules,
        planner.host_types,
        &functions,
        &options.cancel,
        &context.snapshot(),
    )?;

    let mir = MirModule {
        native_targets: planner.native_targets,
        interface_instances: planner.interface_instances,
        external_types,
        dependencies: module.names.imports.dependencies.clone(),
        structures,
        enumerations,
        identity: module.lowered.source.module_identity().clone(),
        source_name: module.lowered.source.name().to_owned(),
        module_slots: Vec::new(),
        abi,
        functions,
    };
    identity::collect(
        &mir.identity,
        &mir.abi.public_items,
        &mir.abi.trait_contracts,
        context,
        &options.cancel,
    )?;
    let metadata = DefinitionMetadata::checked(context.snapshot(), mir, &options.cancel)?;
    verify_mir(metadata, &options.cancel)
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
