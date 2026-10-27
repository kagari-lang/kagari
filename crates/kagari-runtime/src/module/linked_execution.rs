//! Runtime-local execution links are admitted with their supplying program.
pub(crate) mod builtin_results;
pub(crate) mod calls;
pub(crate) mod layouts;
mod native_calls;
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge, call_contracts::ScopedInterfaceCall, environments::EnvironmentId,
    },
    module::{
        ExecutableModule, ModuleStore,
        constants::ConstantPool,
        descriptor_index::DescriptorIndex,
        execution::layout::Location,
        linked_execution::{
            builtin_results::BuiltinResults,
            calls::LinkedCall,
            layouts::{AppliedLayouts, FunctionLayouts},
        },
    },
    native::binding::LinkedNativeFunction,
};
use kagari_contract::ids::FunctionRef;
use std::{cell::RefCell, sync::Arc};

#[derive(Debug, Clone)]
pub(crate) struct LinkedFunction {
    pub(crate) constants: Arc<ConstantPool>,
    calls: Arc<[LinkedCall]>,
    pub(crate) layouts: Option<Arc<FunctionLayouts>>,
    native_calls: Arc<[LinkedNativeCall]>,
    pub(crate) applied_layouts: Option<Arc<AppliedLayouts>>,
}

#[derive(Debug)]
pub(crate) struct LinkedNativeCall {
    pub(crate) body: LinkedNativeBody,
    pub(crate) destination: Option<Location>,
}

#[derive(Debug)]
pub(crate) enum LinkedNativeBody {
    StringByteLength(Location),
    Callback(LinkedNativeCallback),
}

#[derive(Debug)]
pub(crate) struct LinkedNativeCallback {
    pub(crate) arguments: Box<[Location]>,
    pub(crate) function: Arc<LinkedNativeFunction>,
}

impl LinkedFunction {
    pub(crate) fn native_call(&self, index: usize) -> Option<&LinkedNativeCall> {
        self.native_calls.get(index)
    }

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
    // Only the optional derived-layout cache mutates during frame preparation.
    // Executable ownership and the base links stay under the shared store borrow.
    applications: RefCell<Option<Box<FunctionApplications>>>,
    builtin_results: Option<Box<BuiltinResults>>,
}

impl LinkedExecution {
    // Applied functions share the base calls/constants and add only AggregateLayout
    // facts. Their environment-keyed cache is not an additional executable edge:
    // the frame owns its environment, including in release builds where the layout
    // assertion's EnvironmentId is absent.
    pub(super) fn scan_len(&self) -> usize {
        self.functions.len()
    }

    pub(super) fn scan_call_count(&self, function: usize) -> usize {
        self.functions
            .get(function)
            .and_then(Option::as_ref)
            .map_or(0, |function| function.calls.len())
    }

    pub(super) fn scan_call(&self, function: usize, call: usize) -> Option<MetadataEdge<'_>> {
        self.functions
            .get(function)?
            .as_ref()?
            .calls
            .get(call)?
            .interface()
            .map(|call| MetadataEdge::InterfaceCall(call))
    }
}

impl Runtime {
    pub(crate) fn link_execution(&self, owner: &ExecutableModule) -> Result<(), RuntimeError> {
        let constants = self
            .modules
            .inner
            .try_borrow()
            .map_err(|_| {
                RuntimeError::module_validation("module store borrowed during execution linking")
            })?
            .resolve(owner)
            .ok_or_else(|| RuntimeError::module_validation("invalid execution link owner"))?
            .constants
            .clone();
        let mut functions = Vec::with_capacity(owner.bytecode.functions.len());
        for function in &owner.bytecode.functions {
            let prepared = &owner.execution().functions[function.id.index()];
            if !prepared.needs_runtime_links() {
                functions.push(None);
                continue;
            }
            let calls = self.link_calls(owner, function, prepared)?;
            functions.push(Some(Arc::new(LinkedFunction {
                native_calls: self.link_native_calls(owner, function, prepared)?.into(),
                layouts: FunctionLayouts::link(self, owner, function, prepared)?,
                applied_layouts: None,
                constants: constants.clone(),
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
        let execution = LinkedExecution {
            functions: functions.into_boxed_slice(),
            applications: RefCell::new(None),
            builtin_results: None,
        };
        for function in 0..execution.scan_len() {
            for call in 0..execution.scan_call_count(function) {
                if let Some(edge) = execution.scan_call(function, call) {
                    let id = self.gc.metadata_scan_identity(edge)?;
                    self.gc.publish_heap_identity(id)?;
                }
            }
        }
        record.execution = Some(execution);
        Ok(())
    }
}

impl ModuleStore {
    pub(crate) fn linked_function(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
        environment: Option<EnvironmentId>,
    ) -> Option<Arc<LinkedFunction>> {
        let records = self.inner.try_borrow().ok()?;
        records
            .resolve(owner)?
            .execution
            .as_ref()?
            .function(function, environment)
    }
}

impl LinkedExecution {
    pub(super) fn function(
        &self,
        function: FunctionRef,
        environment: Option<EnvironmentId>,
    ) -> Option<Arc<LinkedFunction>> {
        let linked = self.functions.get(function.index())?.as_ref()?;
        let Some(layouts) = linked
            .layouts
            .as_ref()
            .filter(|layouts| layouts.is_scoped())
        else {
            return Some(linked.clone());
        };
        let environment = environment?;
        let mut applications = self.applications.try_borrow_mut().ok()?;
        let applications = applications.get_or_insert_with(|| Box::new(DescriptorIndex::default()));
        if let Some(applied) = applications.get(&environment, &function) {
            return Some(applied.clone());
        }
        // One exact function/environment descriptor is the frame's sole execution
        // reference. Its layout cells contain pure facts, not executable edges.
        let mut applied = linked.as_ref().clone();
        applied.applied_layouts = Some(layouts.application(environment));
        let applied = Arc::new(applied);
        // Optional retention failure cannot revoke the new frame's reference.
        let _ = applications.insert(environment, function, applied.clone());
        Some(applied)
    }
}
