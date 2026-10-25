//! Initialization and resumable processing of one module's retained products.

use crate::source::lower::{
    MirLoweringError, ModuleLowering, ModuleLoweringInputs, ModuleLoweringProducts,
    ModuleLoweringUsage, abi, function,
    instances::{
        InstanceBody, InstanceClassification, InstancePlanner, MirLoweringOptions,
        bridge_identities::BridgeIdentities,
    },
    statics,
};
use kagari_contract::{types::identity, updates::DemandRequests};
use kagari_hir::{
    AnalyzedModule, declarations::DeclarationId, hir::item::function::FunctionKind,
    native::NativeBinding, resolver::resolved::ResolvedName, typeck::FunctionImplementation,
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use std::{collections::HashSet, mem};

impl ModuleLowering {
    /// Register roots and declaration obligations once, without generating bodies.
    ///
    /// `available` is the original limit less preceding modules' output debits.
    /// Only its instruction/generic allowance may change between processing steps.
    pub fn initialize(
        module: &AnalyzedModule,
        inputs: &ModuleLoweringInputs<'_>,
        available: &MirLoweringOptions,
        bridge_identities: &mut BridgeIdentities,
    ) -> Result<Self, MirLoweringError> {
        identity::collect_language(inputs.context, &available.cancel)?;
        #[cfg(test)]
        crate::source::work_metrics::initialize();
        let mut planner = InstancePlanner::new(
            module,
            available,
            inputs,
            bridge_identities,
            Default::default(),
        );
        planner.check()?;
        seed(module, &mut planner)?;
        let owner = planner.owner_handle();
        Ok(Self {
            owner,
            instances: planner.into_state(),
            functions: Vec::new(),
            layouts: Default::default(),
            bridges: Default::default(),
            abi: abi::collect_module_abi(module),
            classifications: Vec::new(),
            source_bodies: Vec::new(),
            failed: false,
            ready: false,
        })
    }

    /// Admit demand deltas and finish the local body/layout/bridge fixed point.
    ///
    /// Shared mutable identities are borrowed only for this call. Completed bodies
    /// and admission charges survive it, including when a later candidate needs
    /// additional products. Closed generated bridge applications may be delivered
    /// in `constructions`; they increase only the output debit. The parent must
    /// check that debit against the whole program allowance before publication.
    /// A failure during processing invalidates this state for further use.
    pub fn process(
        &mut self,
        requests: &DemandRequests,
        inputs: &ModuleLoweringInputs<'_>,
        available: &MirLoweringOptions,
        bridge_identities: &mut BridgeIdentities,
    ) -> Result<ModuleLoweringUsage, MirLoweringError> {
        self.check_budget(available)?;
        let module = *inputs
            .modules_by_handle
            .get(&self.owner)
            .ok_or(MirLoweringError::MissingBinding("lowering source module"))?;
        let mut planner = InstancePlanner::new(
            module,
            available,
            inputs,
            bridge_identities,
            mem::take(&mut self.instances),
        );
        let result = self.process_pending(module, requests, &mut planner);
        self.instances = planner.into_state();
        if let Err(error) = result {
            self.failed = true;
            return Err(error);
        }
        self.ready = true;
        Ok(self.usage())
    }

    fn process_pending(
        &mut self,
        module: &AnalyzedModule,
        requests: &DemandRequests,
        planner: &mut InstancePlanner<'_>,
    ) -> Result<(), MirLoweringError> {
        planner.request_products(requests)?;
        loop {
            self.drain_bodies(planner)?;
            self.layouts
                .collect(module, planner, &self.functions, &self.bridges.functions)?;
            let Some(bridge) = self.bridges.prepare(planner, module)? else {
                break;
            };
            // Preparing shared bridge evidence can admit bodies, tables and layouts.
            // Concrete wrappers need the target's completed executable signature.
            self.drain_bodies(planner)?;
            self.bridges.emit(
                planner,
                module,
                &mut self.abi,
                &self.functions,
                &mut self.classifications,
                bridge,
            )?;
        }
        self.abi.function_origins = planner.function_origins();
        planner.check()
    }

    fn drain_bodies(&mut self, planner: &mut InstancePlanner<'_>) -> Result<(), MirLoweringError> {
        while let Some(instance) = planner.instances.get(self.functions.len()).cloned() {
            planner.check()?;
            let origin = planner.origin(&instance);
            let identity = instance.lower(
                planner.catalog,
                planner.options,
                &planner.definitions(),
                instance.body.span(origin),
            )?;
            let classification = instance.classification.lower(&identity);
            // Intrinsic recipes borrow a source context, not its authored ownership.
            let source = if matches!(
                instance.classification,
                InstanceClassification::IntrinsicAdapter
            ) {
                None
            } else {
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
                Some(source)
            };
            let function = if instance.callable.is_some() {
                function::lower_callable(origin, instance, planner)?
            } else if instance.protocol.is_some() {
                function::lower_protocol(origin, instance, planner)?
            } else if let Some(closure) = instance.closure {
                function::lower_closure(origin, closure, instance, planner)?
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
                        function::lower_function(origin, function, instance, planner)?
                    }
                    InstanceBody::Static(id) => {
                        statics::lower_initializer(origin, id, instance, planner)?
                    }
                }
            };
            planner.check()?;
            debug_assert_eq!(function.id.index(), self.functions.len());
            self.classifications
                .push((identity.clone(), classification));
            if let Some(source) = source {
                self.source_bodies.push((source, identity));
            }
            #[cfg(test)]
            crate::source::work_metrics::body();
            self.functions.push(function);
        }
        Ok(())
    }

    /// Recheck retained local charges after preceding modules' output grows.
    ///
    /// This does not debit output or reset counters. Errors report the supplied
    /// prefix allowance; the parent remaps it to the original public limit.
    pub fn check_budget(&self, available: &MirLoweringOptions) -> Result<(), MirLoweringError> {
        available
            .cancel
            .check()
            .map_err(|_| MirLoweringError::Cancelled)?;
        if let Some(error) = &self.instances.failure {
            return Err(MirLoweringError::diagnostic(error.clone()));
        }
        if self.failed {
            return Err(MirLoweringError::MissingBinding("failed module lowering"));
        }
        for (used, limit, resource) in [
            (
                self.instances.generic_count,
                available.max_generic_instances,
                "generic instances",
            ),
            (
                self.instances.instruction_count,
                available.max_instructions,
                "generated instructions",
            ),
        ] {
            if used > limit {
                return Err(MirLoweringError::diagnostic(
                    Diagnostic::error(DiagnosticKind::CompileLimitExceeded { resource, limit })
                        .with_span(Default::default()),
                ));
            }
        }
        Ok(())
    }

    /// Return local charges and the old program driver's independent output debit.
    /// Instruction debit counts emitted instructions/terminators, not source work.
    pub fn usage(&self) -> ModuleLoweringUsage {
        let products = self.products();
        let functions = || products.functions.iter().chain(products.bridge_functions);
        ModuleLoweringUsage {
            generic_admissions: self.instances.generic_count,
            instruction_charges: self.instances.instruction_count,
            deferred_generic_products: self.instances.closed_bridge_instances.len(),
            generic_products: functions()
                .filter(|function| !function.instance.arguments.is_empty())
                .count()
                + products
                    .structures
                    .iter()
                    .filter(|layout| !layout.arguments.is_empty())
                    .count()
                + products
                    .enumerations
                    .iter()
                    .filter(|layout| !layout.arguments.is_empty())
                    .count()
                + products
                    .interface_instances
                    .iter()
                    .filter(|instance| !instance.arguments.is_empty())
                    .count()
                + products
                    .native_targets
                    .iter()
                    .filter(|target| !target.instance.arguments.is_empty())
                    .count(),
            instructions: functions()
                .flat_map(|function| &function.blocks)
                .map(|block| block.instructions.len() + usize::from(block.terminator.is_some()))
                .sum(),
        }
    }

    pub fn products(&self) -> ModuleLoweringProducts<'_> {
        ModuleLoweringProducts {
            functions: &self.functions,
            bridge_functions: &self.bridges.functions,
            native_targets: &self.instances.native_targets,
            interface_instances: &self.instances.interface_instances,
            structures: &self.layouts.structures,
            enumerations: &self.layouts.enumerations,
            abi: &self.abi,
            classifications: &self.classifications,
            source_bodies: &self.source_bodies,
        }
    }
}

fn seed(
    module: &AnalyzedModule,
    planner: &mut InstancePlanner<'_>,
) -> Result<(), MirLoweringError> {
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
    let definitions = planner.definitions();
    let mut implementations: Vec<_> = module
        .aggregates
        .implementations()
        .filter(|item| {
            definitions
                .resolve(item.id)
                .expect("checked implementation")
                .module()
                == module.lowered.source.module_identity()
                && item.generic_params.is_empty()
        })
        .collect();
    implementations.sort_by(|left, right| {
        definitions
            .compare(left.id, right.id)
            .expect("checked implementation identities")
    });
    for implementation in implementations {
        planner.record_interface(&implementation.id, &[], Default::default())?;
    }
    Ok(())
}
