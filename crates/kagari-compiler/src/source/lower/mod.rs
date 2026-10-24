use crate::source::lower::instances::MirLoweringOptions;
use instances::{InstanceBody, InstancePlanner};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionPath, map::DefinitionContext, mapping::DefinitionMappingError,
        metadata::DefinitionMetadata, table::DefinitionId,
    },
};
use kagari_contract::{
    host as module_host,
    types::{ConcreteFunctionIdentity, ModuleContract, identity},
    updates::{BodyClassification, DemandRequests, limits::UpdateError},
};
use kagari_hir::{
    AnalyzedModule, CheckedAnalysis,
    aggregates::{AggregateCatalog, implementations::ImplementationSearchError},
    declarations::DeclarationId,
    hir::{
        ids::{ExprId, FunctionId, LocalId, PlaceId},
        item::function::FunctionKind,
    },
    native::NativeBinding,
    resolver::resolved::ResolvedName,
    typeck::FunctionImplementation,
};
use kagari_mir::{
    function::MirModule,
    passes::optimize,
    verify::{MirVerificationError, MirVerificationErrorKind, VerifiedMirModule, verify_mir},
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};

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
mod statics;
mod stmt;
mod support;

#[derive(Debug)]
pub enum MirLoweringError {
    Verification(MirVerificationError),
    Diagnostic(Box<Diagnostic>),
    Cancelled,
    /// Candidate or recursive requirement limits exhausted implementation selection.
    ImplementationSearchLimitExceeded,
    Identity(DefinitionMappingError),
    Update(UpdateError),
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

impl From<ImplementationSearchError> for MirLoweringError {
    fn from(error: ImplementationSearchError) -> Self {
        match error {
            ImplementationSearchError::Cancelled => Self::Cancelled,
            ImplementationSearchError::LimitExceeded => Self::ImplementationSearchLimitExceeded,
        }
    }
}

impl From<MirVerificationError> for MirLoweringError {
    fn from(error: MirVerificationError) -> Self {
        verification_error(error)
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
) -> Result<DefinitionMetadata<ModuleContract>, MirLoweringError> {
    cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
    Ok(DefinitionMetadata::checked(
        module.definitions().clone(),
        abi::collect_module_abi(module),
        cancel,
    )?)
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
        &DemandRequests::default(),
        slice::from_ref(&module),
        &module.aggregates,
        &context,
    )
    .and_then(|lowered| optimize_module(lowered.module, options))
}

/// Verified lowering before optional simplification. Publication evidence must be
/// extracted from this module before `optimize_module` consumes it.
pub(crate) struct LoweredModule {
    pub module: VerifiedMirModule,
    pub classifications: Vec<(ConcreteFunctionIdentity, BodyClassification)>,
    pub source_bodies: Vec<(DefinitionId, ConcreteFunctionIdentity)>,
}

pub(crate) fn lower_to_mir_with_requests<'a>(
    module: &'a AnalyzedModule,
    options: &'a MirLoweringOptions,
    requests: &DemandRequests,
    modules: &'a [AnalyzedModule],
    catalog: &'a AggregateCatalog,
    context: &'a DefinitionContext,
) -> Result<LoweredModule, MirLoweringError> {
    identity::collect_language(context, &options.cancel)?;
    let mut planner = InstancePlanner::new(module, options, modules, catalog, context);
    planner.check()?;

    planner.enqueue_statics()?;
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
    planner.request_products(requests)?;
    let mut functions = Vec::new();
    let mut classifications = Vec::new();
    let mut source_bodies = Vec::new();
    while let Some(instance) = planner.instances.get(functions.len()).cloned() {
        planner.check()?;
        let origin = planner.origin(&instance);
        let identity = instance.lower(
            planner.catalog,
            planner.options,
            &planner.definitions(),
            instance.body.span(origin),
        )?;
        classifications.push((identity.clone(), instance.classification.lower(&identity)));
        let authored = match instance.body {
            InstanceBody::Function(id) => ResolvedName::Function(id),
            InstanceBody::Static(id) => ResolvedName::Static(id),
        };
        let DeclarationId::Definition(source) = origin
            .declarations
            .target(authored)
            .ok_or(MirLoweringError::MissingBinding(
                "emitted body source owner",
            ))?
            .id
        else {
            return Err(MirLoweringError::MissingBinding(
                "emitted body source identity",
            ));
        };
        source_bodies.push((source, identity));
        functions.push(if instance.callable.is_some() {
            function::lower_callable(origin, instance, &mut planner)?
        } else if instance.protocol.is_some() {
            function::lower_protocol(origin, instance, &mut planner)?
        } else if let Some(closure) = instance.closure {
            function::lower_closure(origin, closure, instance, &mut planner)?
        } else {
            match instance.body {
                InstanceBody::Function(id) => {
                    let function = origin
                        .lowered
                        .module
                        .functions
                        .iter()
                        .find(|function| function.id == id)
                        .ok_or(MirLoweringError::MissingTypedFunction(id))?;
                    function::lower_function(origin, function, instance, &mut planner)?
                }
                InstanceBody::Static(id) => {
                    statics::lower_initializer(origin, id, instance, &mut planner)?
                }
            }
        });
    }

    let layouts::Layouts {
        structures,
        enumerations,
    } = layouts::collect(module, &mut planner, &functions)?;
    let mut abi = abi::collect_module_abi(module);
    interface_bridges::collect(
        &mut planner,
        module,
        &mut abi,
        &mut functions,
        &mut classifications,
    )?;
    abi.function_origins = planner.function_origins();
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
        dependencies: module
            .names
            .imports
            .dependencies
            .iter()
            .map(|module_handle| {
                module
                    .names
                    .catalog
                    .definitions()
                    .module_identity(*module_handle)
                    .expect("checked dependency")
                    .clone()
            })
            .collect(),
        structures,
        enumerations,
        identity: module.lowered.source.module_identity().clone(),
        source_name: module.lowered.source.name().to_owned(),
        module_slots: planner.static_slots,
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
        .map_err(verification_error)
        .map(|module| LoweredModule {
            module,
            classifications,
            source_bodies,
        })
}

pub(crate) fn optimize_module(
    module: VerifiedMirModule,
    options: &MirLoweringOptions,
) -> Result<VerifiedMirModule, MirLoweringError> {
    match &options.optimization {
        Some(passes) => optimize(module, passes, &options.cancel)
            .map(|result| result.module)
            .map_err(verification_error),
        None => Ok(module),
    }
}

fn verification_error(error: MirVerificationError) -> MirLoweringError {
    match error.kind {
        MirVerificationErrorKind::Cancelled => MirLoweringError::Cancelled,
        MirVerificationErrorKind::Limit { resource, limit } => MirLoweringError::diagnostic(
            Diagnostic::error(DiagnosticKind::CompileLimitExceeded { resource, limit })
                .with_span(error.span.unwrap_or_default()),
        ),
        _ => MirLoweringError::Verification(error),
    }
}
