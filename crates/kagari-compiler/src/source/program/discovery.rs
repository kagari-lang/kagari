//! Delta use extraction and contract-owned construction closure over retained summaries.

use crate::source::{
    lower::{MirLoweringError, instances::MirLoweringOptions},
    program::{
        SourceProgramError,
        interface_requests::ForeignInterfaces,
        scheduler::{ScheduledModule, lowering_error},
        update_requests::RequestQueue,
    },
};
use kagari_common::identity::{ModuleIdentity, table::DefinitionTable};
use kagari_contract::{
    callable::{
        construction::{ConstructionBody, closed_constructions, native_construction},
        evidence::EvidenceRef,
        witness::OperationWitness,
    },
    native_import::NativeImport,
    types::ConcreteFunctionIdentity,
};
use kagari_hir::AnalyzedModule;
use kagari_mir::program::construction::{
    function_construction,
    methods::{ConstructionModule, construction_methods},
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::ty::substitution::TypeTransformError;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Default)]
struct ProductCursor {
    functions: usize,
    bridges: usize,
    natives: usize,
    interfaces: usize,
}

// Match whole-program collection precedence even when an earlier module's
// native target or ordinary body is discovered after a later module's import.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SummaryOrigin {
    Script {
        module: usize,
        bridge: bool,
        function: usize,
    },
    Native {
        module: usize,
        section: NativeSection,
        body: usize,
        import: usize,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NativeSection {
    Target,
    Function,
    Bridge,
}

#[derive(Default)]
pub(super) struct Discovery {
    cursors: Vec<ProductCursor>,
    materialized: HashSet<ConcreteFunctionIdentity>,
    interfaces: HashSet<ConcreteFunctionIdentity>,
    foreign: HashSet<(usize, ConcreteFunctionIdentity)>,
    bodies: Vec<ConstructionBody>,
    body_indices: HashMap<ConcreteFunctionIdentity, (usize, SummaryOrigin)>,
    function_count: usize,
    native_results: usize,
}

impl Discovery {
    pub fn scan(
        &mut self,
        states: &[ScheduledModule],
        sources: &[AnalyzedModule],
        definitions: &DefinitionTable,
        requests: &mut RequestQueue,
        root: &ModuleIdentity,
        options: &MirLoweringOptions,
    ) -> Result<(), SourceProgramError> {
        self.cursors
            .resize_with(states.len(), ProductCursor::default);
        let error = |error| lowering_error(root, error, options);
        // Register the whole new output batch before routing uses. Otherwise a
        // generated adapter in a later module could be mistaken for a source root.
        for (index, (retained, source)) in states.iter().zip(sources).enumerate() {
            let products = retained.state.products();
            let cursor = self.cursors[index];
            let owner = definitions
                .lookup_module(source.lowered.source.module_identity())
                .expect("checked source module");
            for function in products.functions[cursor.functions..]
                .iter()
                .chain(&products.bridge_functions[cursor.bridges..])
            {
                options
                    .cancel
                    .check()
                    .map_err(|_| error(MirLoweringError::Cancelled))?;
                self.materialized.insert(function.instance.clone());
            }
            for native in &products.native_targets[cursor.natives..] {
                options
                    .cancel
                    .check()
                    .map_err(|_| error(MirLoweringError::Cancelled))?;
                // A foreign import retained for a local slot does not publish that
                // callback in its owning module.
                if definitions
                    .module_of(native.instance.declaration)
                    .expect("native owner")
                    == owner
                {
                    self.materialized.insert(native.instance.clone());
                }
            }
            for instance in &products.interface_instances[cursor.interfaces..] {
                options
                    .cancel
                    .check()
                    .map_err(|_| error(MirLoweringError::Cancelled))?;
                if definitions
                    .module_of(instance.declaration)
                    .expect("interface owner")
                    == owner
                {
                    self.interfaces.insert(instance.clone());
                }
            }
        }
        let foreign = ForeignInterfaces {
            modules: sources,
            root,
            definitions,
            cancel: &options.cancel,
        };
        for (index, (retained, source)) in states.iter().zip(sources).enumerate() {
            let products = retained.state.products();
            let cursor = self.cursors[index];
            let caller = source.lowered.source.module_identity();
            for (bridge, function_index, function) in products.functions[cursor.functions..]
                .iter()
                .enumerate()
                .map(|(i, function)| (false, cursor.functions + i, function))
                .chain(
                    products.bridge_functions[cursor.bridges..]
                        .iter()
                        .enumerate()
                        .map(|(i, function)| (true, cursor.bridges + i, function)),
                )
            {
                #[cfg(test)]
                crate::source::work_metrics::scanned_body();
                let uses = function
                    .callable_uses(&options.cancel)
                    .map_err(|cause| error(construction_error(cause, options)))?;
                for application in uses.shared {
                    if !self.materialized.contains(&application.body) {
                        requests.shared(application, definitions).map_err(error)?;
                    }
                }
                for instance in uses.functions {
                    if !self.materialized.contains(&instance) {
                        requests.function(instance, definitions).map_err(error)?;
                    }
                }
                for instance in uses.interfaces {
                    if self.foreign.insert((index, instance.clone())) {
                        foreign.request(caller, &instance, requests)?;
                    }
                }
                let body = function_construction(function, products.functions, &options.cancel)
                    .map_err(|cause| error(construction_error(cause, options)))?;
                self.record_body(
                    body,
                    SummaryOrigin::Script {
                        module: index,
                        bridge,
                        function: function_index,
                    },
                );
                self.function_count += 1;
                for (import_index, import) in uses.natives.into_iter().enumerate() {
                    self.record_native(
                        import,
                        SummaryOrigin::Native {
                            module: index,
                            section: if bridge {
                                NativeSection::Bridge
                            } else {
                                NativeSection::Function
                            },
                            body: function_index,
                            import: import_index,
                        },
                    );
                }
            }
            for (import_index, import) in
                products.native_targets[cursor.natives..].iter().enumerate()
            {
                options
                    .cancel
                    .check()
                    .map_err(|_| error(MirLoweringError::Cancelled))?;
                for operation in import
                    .callables
                    .iter()
                    .map(EvidenceRef::Operation)
                    .flat_map(EvidenceRef::operations)
                {
                    options
                        .cancel
                        .check()
                        .map_err(|_| error(MirLoweringError::Cancelled))?;
                    if let OperationWitness::Selected(call) = operation
                        && !self.materialized.contains(&call.instance)
                    {
                        requests
                            .function(call.instance.clone(), definitions)
                            .map_err(error)?;
                    }
                }
                self.record_native(
                    import,
                    SummaryOrigin::Native {
                        module: index,
                        section: NativeSection::Target,
                        body: 0,
                        import: cursor.natives + import_index,
                    },
                );
            }
            for instance in &products.interface_instances[cursor.interfaces..] {
                if self.foreign.insert((index, instance.clone())) {
                    foreign.request(caller, instance, requests)?;
                }
            }
            self.cursors[index] = ProductCursor {
                functions: products.functions.len(),
                bridges: products.bridge_functions.len(),
                natives: products.native_targets.len(),
                interfaces: products.interface_instances.len(),
            };
        }
        Ok(())
    }

    fn record_native(&mut self, import: &NativeImport, origin: SummaryOrigin) {
        self.native_results += usize::from(import.result_adapter.is_some());
        if let Some(body) = native_construction(import) {
            self.record_body(body, origin);
        }
    }

    fn record_body(&mut self, body: ConstructionBody, origin: SummaryOrigin) {
        if let Some((index, previous)) = self.body_indices.get_mut(&body.identity) {
            if origin < *previous {
                self.bodies[*index] = body;
                *previous = origin;
            }
        } else {
            self.body_indices
                .insert(body.identity.clone(), (self.bodies.len(), origin));
            self.bodies.push(body);
        }
    }

    pub fn close_constructions(
        &self,
        states: &[ScheduledModule],
        sources: &[AnalyzedModule],
        definitions: &DefinitionTable,
        requests: &mut RequestQueue,
        root: &ModuleIdentity,
        options: &MirLoweringOptions,
    ) -> Result<bool, SourceProgramError> {
        let views = states
            .iter()
            .zip(sources)
            .map(|(retained, source)| {
                let products = retained.state.products();
                ConstructionModule {
                    identity: source.lowered.source.module_identity(),
                    functions: products.functions,
                    bridge_functions: products.bridge_functions,
                    native_targets: products.native_targets,
                    interface_instances: products.interface_instances,
                    abi: products.abi,
                }
            })
            .collect::<Vec<_>>();
        let error = |cause| lowering_error(root, construction_error(cause, options), options);
        let methods = construction_methods(&views, definitions, &options.cancel).map_err(error)?;
        let limit = options
            .max_generic_instances
            .saturating_add(self.function_count + self.native_results);
        let constructions =
            closed_constructions(&self.bodies, &methods, limit, &options.cancel).map_err(error)?;
        let mut changed = false;
        for instance in constructions {
            options
                .cancel
                .check()
                .map_err(|_| error(TypeTransformError::Cancelled))?;
            if !self.interfaces.contains(&instance) {
                let added = requests
                    .construction(instance, definitions)
                    .map_err(|cause| lowering_error(root, cause, options))?;
                if !added {
                    return Err(lowering_error(
                        root,
                        MirLoweringError::MissingBinding("unmaterialized interface construction"),
                        options,
                    ));
                }
                changed = true;
            }
        }
        Ok(changed)
    }
}

fn construction_error(error: TypeTransformError, options: &MirLoweringOptions) -> MirLoweringError {
    match error {
        TypeTransformError::Cancelled => MirLoweringError::Cancelled,
        TypeTransformError::LimitExceeded => MirLoweringError::diagnostic(
            Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                resource: "generic instances",
                limit: options.max_generic_instances,
            })
            .with_span(Default::default()),
        ),
        TypeTransformError::InvalidContract => {
            MirLoweringError::MissingBinding("shared construction contract")
        }
    }
}
