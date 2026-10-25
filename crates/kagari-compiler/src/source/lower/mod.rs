use crate::source::lower::{
    instances::{MirLoweringOptions, bridge_identities::BridgeIdentities, state::InstanceState},
    interface_bridges::InterfaceBridges,
    layouts::Layouts,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionPath,
        map::DefinitionContext,
        mapping::DefinitionMappingError,
        metadata::DefinitionMetadata,
        table::{DefinitionId, module::ModuleHandle},
    },
};
use kagari_contract::{
    layout::{EnumLayout, StructLayout},
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, ModuleContract},
    updates::{BodyClassification, DemandRequests, limits::UpdateError},
};
use kagari_hir::{
    AnalyzedModule, CheckedAnalysis,
    aggregates::{AggregateCatalog, implementations::ImplementationSearchError},
    hir::ids::{ExprId, FunctionId, LocalId, PlaceId},
};
use kagari_mir::{
    function::MirFunction,
    passes::optimize,
    verify::{MirVerificationError, MirVerificationErrorKind, VerifiedMirModule},
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use std::{collections::HashMap, slice};

mod abi;
mod debug;
mod expr;
mod function;
mod future;
mod host;
pub mod instances;
mod interface_bridges;
mod layouts;
mod lifecycle;
mod place;
mod snapshot;
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

/// Immutable checked inputs shared by every module in one lowering operation.
///
/// The catalog must cover the complete checked dependency closure, including
/// caller-private generic arguments. Construct once and reuse for every step;
/// states and their definition IDs must not be transferred between input contexts.
pub(crate) struct ModuleLoweringInputs<'a> {
    modules: &'a [AnalyzedModule],
    modules_by_handle: HashMap<ModuleHandle, &'a AnalyzedModule>,
    catalog: &'a AggregateCatalog,
    context: &'a DefinitionContext,
}

impl<'a> ModuleLoweringInputs<'a> {
    pub fn new(
        modules: &'a [AnalyzedModule],
        catalog: &'a AggregateCatalog,
        context: &'a DefinitionContext,
    ) -> Self {
        let definitions = context.snapshot();
        let modules_by_handle = modules
            .iter()
            .map(|module| {
                let handle = definitions
                    .lookup_module(module.lowered.source.module_identity())
                    .expect("checked source module");
                (handle, module)
            })
            .collect();
        Self {
            modules,
            modules_by_handle,
            catalog,
            context,
        }
    }
}

/// Retained, unoptimized lowering products for one output module.
///
/// Initialize once, process demand deltas with the current deterministic prefix
/// allowance, then snapshot at a candidate barrier. Ordinary function IDs remain
/// stable; bridge wrappers receive candidate-local tail IDs. Processing errors
/// invalidate this state; discard it rather than attempting partial publication.
pub(crate) struct ModuleLowering {
    owner: ModuleHandle,
    instances: InstanceState,
    functions: Vec<MirFunction>,
    layouts: Layouts,
    bridges: InterfaceBridges,
    abi: ModuleContract,
    classifications: Vec<(ConcreteFunctionIdentity, BodyClassification)>,
    source_bodies: Vec<(DefinitionId, ConcreteFunctionIdentity)>,
    failed: bool,
    ready: bool,
}

/// Independent local admission charges and emitted-product prefix debits.
///
/// Generic admissions deliberately differ from products with nonempty identity
/// arguments. The program driver subtracts output debits in module order and
/// rechecks local charges against each resulting prefix allowance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ModuleLoweringUsage {
    pub generic_admissions: usize,
    pub instruction_charges: usize,
    pub generic_products: usize,
    /// Closed generated tables were historically debited after all module checks.
    pub deferred_generic_products: usize,
    pub instructions: usize,
}

/// Borrowed append-only products for delta discovery, without a candidate clone.
///
/// Product slice indices remain stable across successful processing steps; ABI
/// function origins are derived in canonical order rather than append order. Bridge IDs
/// are tail-relative here, not module function IDs; their outgoing local targets
/// always index `functions`. Only a snapshot assigns their final module IDs.
pub(crate) struct ModuleLoweringProducts<'a> {
    pub functions: &'a [MirFunction],
    pub bridge_functions: &'a [MirFunction],
    pub native_targets: &'a [NativeImport],
    pub interface_instances: &'a [ConcreteFunctionIdentity],
    pub structures: &'a [StructLayout],
    pub enumerations: &'a [EnumLayout],
    pub abi: &'a ModuleContract,
    pub classifications: &'a [(ConcreteFunctionIdentity, BodyClassification)],
    pub source_bodies: &'a [(DefinitionId, ConcreteFunctionIdentity)],
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
    let inputs = ModuleLoweringInputs::new(slice::from_ref(&module), &module.aggregates, &context);
    let mut bridges = BridgeIdentities::default();
    let mut state = ModuleLowering::initialize(&module, &inputs, options, &mut bridges)?;
    state.process(&DemandRequests::default(), &inputs, options, &mut bridges)?;
    optimize_module(state.snapshot(&inputs, options)?.module, options)
}

/// Verified lowering before optional simplification. Publication evidence must be
/// extracted from this module before `optimize_module` consumes it.
pub(crate) struct LoweredModule {
    pub module: VerifiedMirModule,
    pub classifications: Vec<(ConcreteFunctionIdentity, BodyClassification)>,
    pub source_bodies: Vec<(DefinitionId, ConcreteFunctionIdentity)>,
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
