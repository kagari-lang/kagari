//! Compilation-owned module states, deterministic delivery and prefix accounting.

use crate::source::{
    lower::{
        LoweredModule, MirLoweringError, ModuleLowering, ModuleLoweringInputs, ModuleLoweringUsage,
        instances::{MirLoweringOptions, bridge_identities::BridgeIdentities},
    },
    program::{SourceProgramError, discovery::Discovery, update_requests::RequestQueue},
};
use kagari_common::identity::{
    ModuleIdentity,
    map::DefinitionContext,
    table::{DefinitionId, DefinitionTable},
};
use kagari_contract::{
    types::ConcreteFunctionIdentity,
    updates::{BodyClassification, DemandRequests},
};
use kagari_hir::{AnalyzedModule, aggregates::AggregateCatalog};
use kagari_mir::{
    function::MirModule,
    program::{ProgramError, ProgramErrorKind},
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};

#[cfg(test)]
mod tests;

pub(super) struct ScheduledModule {
    pub state: ModuleLowering,
    pub usage: ModuleLoweringUsage,
}

pub(super) struct ProgramSnapshot {
    pub modules: Vec<MirModule>,
    pub classifications: Vec<(ConcreteFunctionIdentity, BodyClassification)>,
    pub source_bodies: Vec<(DefinitionId, ConcreteFunctionIdentity)>,
}

pub(super) struct Scheduler<'a> {
    modules: &'a [AnalyzedModule],
    catalog: &'a AggregateCatalog,
    context: &'a DefinitionContext,
    root: &'a ModuleIdentity,
    options: &'a MirLoweringOptions,
    inputs: ModuleLoweringInputs<'a>,
    states: Vec<ScheduledModule>,
    requests: RequestQueue,
    bridges: BridgeIdentities,
    discovery: Discovery,
}

impl<'a> Scheduler<'a> {
    pub fn new(
        modules: &'a [AnalyzedModule],
        catalog: &'a AggregateCatalog,
        context: &'a DefinitionContext,
        root: &'a ModuleIdentity,
        options: &'a MirLoweringOptions,
        requests: RequestQueue,
    ) -> Self {
        Self {
            modules,
            catalog,
            context,
            root,
            options,
            inputs: ModuleLoweringInputs::new(modules, catalog, context),
            states: Vec::new(),
            requests,
            bridges: BridgeIdentities::default(),
            discovery: Discovery::default(),
        }
    }

    /// Reach a product fixed point without making candidates or rescanning old MIR.
    pub fn close(&mut self) -> Result<(), SourceProgramError> {
        loop {
            #[cfg(test)]
            crate::source::work_metrics::round();
            self.options.cancel.check().map_err(|_| ProgramError {
                module: Box::new(self.root.clone()),
                kind: ProgramErrorKind::Cancelled,
            })?;
            let processed = self.process_pending()?;
            let definitions = self.context.snapshot();
            self.discovery.scan(
                &self.states,
                self.modules,
                &definitions,
                &mut self.requests,
                self.root,
                self.options,
            )?;
            if self.requests.has_pending() {
                if !processed {
                    return Err(lowering_error(
                        self.root,
                        MirLoweringError::MissingBinding("requested output module"),
                        self.options,
                    ));
                }
                continue;
            }
            if !self.discovery.close_constructions(
                &self.states,
                self.modules,
                &definitions,
                &mut self.requests,
                self.root,
                self.options,
            )? {
                return Ok(());
            }
        }
    }

    fn process_pending(&mut self) -> Result<bool, SourceProgramError> {
        let mut available = self.options.clone();
        let mut deferred = 0usize;
        let mut processed = false;
        for (index, module) in self.modules.iter().enumerate() {
            let identity = module.lowered.source.module_identity();
            let handle = self
                .context
                .snapshot()
                .lookup_module(identity)
                .expect("checked source module");
            let delta = self.requests.take_pending(handle);
            if index == self.states.len() {
                let mut state =
                    ModuleLowering::initialize(module, &self.inputs, &available, &mut self.bridges)
                        .map_err(|error| lowering_error(identity, error, self.options))?;
                let usage = state
                    .process(&delta, &self.inputs, &available, &mut self.bridges)
                    .map_err(|error| lowering_error(identity, error, self.options))?;
                self.states.push(ScheduledModule { state, usage });
                processed = true;
            } else {
                let retained = &mut self.states[index];
                retained
                    .state
                    .check_budget(&available)
                    .map_err(|error| lowering_error(identity, error, self.options))?;
                if !delta.is_empty() {
                    retained.usage = retained
                        .state
                        .process(&delta, &self.inputs, &available, &mut self.bridges)
                        .map_err(|error| lowering_error(identity, error, self.options))?;
                    processed = true;
                }
            }
            let usage = self.states[index].usage;
            debit(&mut available, usage, identity, self.options)?;
            deferred += usage.deferred_generic_products;
        }
        debit_deferred(&mut available, deferred, self.root, self.options)?;
        Ok(processed)
    }

    pub fn apply(
        &mut self,
        batch: DemandRequests,
        definitions: &DefinitionTable,
    ) -> Result<bool, SourceProgramError> {
        let owner = definitions
            .lookup_module(self.root)
            .expect("checked root module");
        self.requests
            .apply(
                batch,
                self.catalog,
                definitions,
                owner,
                &self.options.cancel,
            )
            .map_err(|error| lowering_error(self.root, error, self.options))
    }

    /// Clone retained products only at a candidate barrier, keeping fresh seals.
    pub fn snapshot(&self) -> Result<ProgramSnapshot, SourceProgramError> {
        let mut available = self.options.clone();
        let mut deferred = 0usize;
        let mut snapshot = ProgramSnapshot {
            modules: Vec::new(),
            classifications: Vec::new(),
            source_bodies: Vec::new(),
        };
        for (source, retained) in self.modules.iter().zip(&self.states) {
            let identity = source.lowered.source.module_identity();
            let LoweredModule {
                module,
                classifications,
                source_bodies,
            } = retained
                .state
                .snapshot(&self.inputs, &available)
                .map_err(|error| lowering_error(identity, error, self.options))?;
            snapshot
                .modules
                .push(module.into_unverified().into_records());
            snapshot.classifications.extend(classifications);
            snapshot.source_bodies.extend(source_bodies);
            debit(&mut available, retained.usage, identity, self.options)?;
            deferred += retained.usage.deferred_generic_products;
        }
        debit_deferred(&mut available, deferred, self.root, self.options)?;
        Ok(snapshot)
    }
}

fn debit(
    available: &mut MirLoweringOptions,
    usage: ModuleLoweringUsage,
    owner: &ModuleIdentity,
    options: &MirLoweringOptions,
) -> Result<(), SourceProgramError> {
    let ordinary = usage.generic_products - usage.deferred_generic_products;
    available.max_generic_instances = available
        .max_generic_instances
        .checked_sub(ordinary)
        .ok_or_else(|| limit_error(owner, "generic instances", options.max_generic_instances))?;
    available.max_instructions = available
        .max_instructions
        .checked_sub(usage.instructions)
        .ok_or_else(|| limit_error(owner, "generated instructions", options.max_instructions))?;
    Ok(())
}

fn debit_deferred(
    available: &mut MirLoweringOptions,
    count: usize,
    root: &ModuleIdentity,
    options: &MirLoweringOptions,
) -> Result<(), SourceProgramError> {
    // These concrete generated tables were added by program closure, after all
    // module-local checks. Do not move their debit into a later module's prefix.
    available.max_generic_instances = available
        .max_generic_instances
        .checked_sub(count)
        .ok_or_else(|| limit_error(root, "generic instances", options.max_generic_instances))?;
    Ok(())
}

fn limit_error(owner: &ModuleIdentity, resource: &'static str, limit: usize) -> SourceProgramError {
    SourceProgramError::Lowering {
        module: Box::new(owner.clone()),
        error: MirLoweringError::diagnostic(
            Diagnostic::error(DiagnosticKind::CompileLimitExceeded { resource, limit })
                .with_span(Default::default()),
        ),
    }
}

pub(super) fn lowering_error(
    owner: &ModuleIdentity,
    mut error: MirLoweringError,
    options: &MirLoweringOptions,
) -> SourceProgramError {
    if let MirLoweringError::Diagnostic(diagnostic) = &mut error
        && let DiagnosticKind::CompileLimitExceeded { resource, limit } = &mut diagnostic.kind
    {
        match *resource {
            "generic instances" => *limit = options.max_generic_instances,
            "generated instructions" => *limit = options.max_instructions,
            _ => {}
        }
    }
    SourceProgramError::Lowering {
        module: Box::new(owner.clone()),
        error,
    }
}
