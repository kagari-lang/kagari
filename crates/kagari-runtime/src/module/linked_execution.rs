//! Runtime-local execution links are admitted with their supplying program.
pub(crate) mod builtin_results;
pub(crate) mod calls;
pub(crate) mod interfaces;
pub(crate) mod layouts;
mod metadata;
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::SessionId,
    execution_metadata::{
        call_contracts::ScopedInterfaceCall, environments::EnvironmentId,
        managed::sources::MetadataSource,
    },
    module::{
        ExecutableModule, ModuleStore,
        descriptor_index::DescriptorIndex,
        dispatch::bindings::PublishedRoutes,
        linked_execution::{
            builtin_results::BuiltinResults,
            calls::LinkedCall,
            interfaces::{AppliedUpcasts, FunctionUpcasts},
            layouts::{AppliedLayouts, FunctionLayouts},
        },
    },
};
use kagari_contract::ids::FunctionRef;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct LinkedFunction {
    calls: Arc<[LinkedCall]>,
    pub(crate) layouts: Option<Arc<FunctionLayouts>>,
    pub(crate) applied_layouts: Option<Arc<AppliedLayouts>>,
    pub(crate) upcasts: Option<Arc<FunctionUpcasts>>,
    pub(crate) applied_upcasts: Option<Arc<AppliedUpcasts>>,
}

impl LinkedFunction {
    pub(crate) fn call(&self, index: usize) -> Option<&Arc<ScopedInterfaceCall>> {
        self.calls.get(index)?.interface()
    }

    pub(crate) fn call_contract(&self, index: usize) -> Option<&LinkedCall> {
        self.calls.get(index)
    }
}

type FunctionApplications = DescriptorIndex<EnvironmentId, FunctionRef, Arc<LinkedFunction>>;

#[derive(Debug)]
pub(super) struct LinkedExecution {
    functions: Box<[Option<Arc<LinkedFunction>>]>,
    applications: Option<Box<FunctionApplications>>,
    builtin_results: Option<Box<BuiltinResults>>,
}

impl LinkedExecution {
    pub(in crate::module) fn rebound(
        &self,
        routes: &PublishedRoutes,
    ) -> Result<Self, RuntimeError> {
        let functions = self
            .functions
            .iter()
            .map(|function| {
                function
                    .as_ref()
                    .map(|function| {
                        let mut next = function.as_ref().clone();
                        next.calls = function
                            .calls
                            .iter()
                            .map(|call| call.rebound(routes))
                            .collect::<Result<Vec<_>, _>>()?
                            .into();
                        Ok(Arc::new(next))
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?
            .into_boxed_slice();
        Ok(Self {
            functions,
            applications: None,
            builtin_results: None,
        })
    }
}

impl Runtime {
    pub(crate) fn link_execution(
        &mut self,
        execution: SessionId,
        owner: &ExecutableModule,
        protected: &mut Vec<MetadataSource<'_>>,
    ) -> Result<Vec<Arc<ScopedInterfaceCall>>, RuntimeError> {
        let mut closed_calls = Vec::new();
        let mut functions = Vec::with_capacity(owner.bytecode.functions.len());
        for function in &owner.bytecode.functions {
            let prepared = &owner.execution().functions[function.id.index()];
            if !prepared.needs_runtime_links() {
                functions.push(None);
                continue;
            }
            let calls = self.link_calls(execution, owner, function, prepared, protected)?;
            closed_calls
                .try_reserve(calls.len())
                .map_err(|_| RuntimeError::resource_limit("closed call metadata"))?;
            closed_calls.extend(calls.iter().filter_map(LinkedCall::interface).cloned());
            functions.push(Some(Arc::new(LinkedFunction {
                layouts: FunctionLayouts::link(self, owner, function, prepared)?,
                applied_layouts: None,
                upcasts: FunctionUpcasts::link(self, owner, &prepared.upcasts)?,
                applied_upcasts: None,
                calls,
            })));
        }
        let mut records = self.modules.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("module store borrowed during execution linking")
        })?;
        let record = records
            .resolve_mut(owner)
            .ok_or_else(|| RuntimeError::module_validation("invalid execution link owner"))?;
        if record.execution.is_some() {
            return Err(RuntimeError::module_validation("duplicate execution links"));
        }
        record.execution = Some(LinkedExecution {
            functions: functions.into_boxed_slice(),
            applications: None,
            builtin_results: None,
        });
        Ok(closed_calls)
    }
}

impl ModuleStore {
    pub(crate) fn linked_function(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
        environment: Option<EnvironmentId>,
    ) -> Option<Arc<LinkedFunction>> {
        let mut records = self.inner.try_borrow_mut().ok()?;
        let execution = records.resolve_mut(owner)?.execution.as_mut()?;
        let linked = execution.functions.get(function.index())?.as_ref()?;
        let layouts = linked
            .layouts
            .as_ref()
            .filter(|layouts| layouts.is_scoped());
        let upcasts = linked
            .upcasts
            .as_ref()
            .filter(|upcasts| upcasts.is_scoped());
        if layouts.is_none() && upcasts.is_none() {
            return Some(linked.clone());
        }
        let environment = environment?;
        let applications = execution
            .applications
            .get_or_insert_with(|| Box::new(DescriptorIndex::default()));
        if let Some(applied) = applications.get(&environment, &function) {
            return Some(applied.clone());
        }
        // One exact function/environment descriptor is the frame's sole execution
        // reference. Its layout cells contain pure facts, not executable edges.
        let mut applied = linked.as_ref().clone();
        applied.applied_layouts = layouts.map(|layouts| layouts.application(environment));
        applied.applied_upcasts = upcasts.map(|upcasts| upcasts.application(environment));
        let applied = Arc::new(applied);
        // Optional retention failure cannot revoke the new frame's reference.
        let _ = applications.insert(environment, function, applied.clone());
        Some(applied)
    }
}
