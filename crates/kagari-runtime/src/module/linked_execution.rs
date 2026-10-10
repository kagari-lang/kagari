//! Runtime-local execution links are admitted with their supplying program.
pub(crate) mod layouts;
mod primitives;
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge,
        call_contracts::{InterfaceCallSite, ScopedInterfaceCall},
        environments::EnvironmentId,
    },
    module::{
        LoadedModule, ModuleStore,
        constants::ConstantPool,
        descriptor_index::DescriptorIndex,
        execution::{calls::PreparedCallTarget, layout::Location},
        linked_execution::layouts::{AppliedLayouts, FunctionLayouts},
    },
    native::{binding::LinkedNativeFunction, primitive::NativePrimitive},
};
use kagari_contract::ids::FunctionRef;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct LinkedFunction {
    pub(crate) constants: Arc<ConstantPool>,
    calls: Arc<[Option<Arc<ScopedInterfaceCall>>]>,
    pub(crate) layouts: Option<Arc<FunctionLayouts>>,
    primitives: Arc<[Option<LinkedPrimitive>]>,
    pub(crate) applied_layouts: Option<Arc<AppliedLayouts>>,
}

#[derive(Debug)]
pub(crate) struct LinkedPrimitive {
    pub(crate) body: LinkedPrimitiveBody,
    pub(crate) destination: Option<Location>,
}

#[derive(Debug)]
pub(crate) enum LinkedPrimitiveBody {
    StringByteLength(Location),
    Vector(LinkedVectorPrimitive),
}

#[derive(Debug)]
pub(crate) struct LinkedVectorPrimitive {
    pub(crate) operation: NativePrimitive,
    pub(crate) arguments: Box<[Location]>,
    pub(crate) function: Arc<LinkedNativeFunction>,
}

impl LinkedFunction {
    pub(crate) fn primitive(&self, index: usize) -> Option<&Option<LinkedPrimitive>> {
        self.primitives.get(index)
    }

    pub(crate) fn call(&self, index: usize) -> Option<&Arc<ScopedInterfaceCall>> {
        self.calls.get(index)?.as_ref()
    }
}

type FunctionApplications = DescriptorIndex<EnvironmentId, FunctionRef, Arc<LinkedFunction>>;

#[derive(Debug)]
pub(super) struct LinkedExecution {
    functions: Box<[Option<Arc<LinkedFunction>>]>,
    applications: Option<Box<FunctionApplications>>,
}

impl LinkedExecution {
    pub(super) fn trace<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        pending.extend(self.functions.iter().flatten().flat_map(|function| {
            function
                .calls
                .iter()
                .flatten()
                .map(|call| MetadataEdge::InterfaceCall(call))
        }));
    }
}

impl Runtime {
    pub(crate) fn link_execution(&self, owner: &LoadedModule) -> Result<(), RuntimeError> {
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
            let mut calls = Vec::with_capacity(prepared.interface_calls);
            for (&pc, call) in &prepared.calls {
                let PreparedCallTarget::Interface { index, closed } = call.target else {
                    continue;
                };
                if index != calls.len() {
                    return Err(RuntimeError::module_validation(
                        "invalid linked interface ordinal",
                    ));
                }
                if !closed {
                    calls.push(None);
                    continue;
                }
                let call = self.build_interface_call(
                    owner,
                    None,
                    InterfaceCallSite {
                        function: function.id,
                        pc,
                    },
                )?;
                let dependencies =
                    self.metadata_dependencies(MetadataEdge::InterfaceCall(&call))?;
                // All closed witnesses were selected from this pinned program's
                // members. Prove that its active root suffices before publishing.
                if dependencies
                    .iter()
                    .any(|dependency| dependency.program_identity() != owner.program_identity())
                {
                    return Err(RuntimeError::module_validation(
                        "foreign closed call dependency",
                    ));
                }
                calls.push(Some(call));
            }
            functions.push(Some(Arc::new(LinkedFunction {
                primitives: self.link_primitives(owner, function, prepared)?.into(),
                layouts: FunctionLayouts::link(self, owner, function, prepared)?,
                applied_layouts: None,
                constants: constants.clone(),
                calls: calls.into(),
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
        });
        Ok(())
    }
}

impl ModuleStore {
    pub(crate) fn linked_function(
        &self,
        owner: &LoadedModule,
        function: FunctionRef,
        environment: Option<EnvironmentId>,
    ) -> Option<Arc<LinkedFunction>> {
        let mut records = self.inner.try_borrow_mut().ok()?;
        let execution = records.resolve_mut(owner)?.execution.as_mut()?;
        let linked = execution.functions.get(function.index())?.as_ref()?;
        let Some(layouts) = linked
            .layouts
            .as_ref()
            .filter(|layouts| layouts.is_scoped())
        else {
            return Some(linked.clone());
        };
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
        applied.applied_layouts = Some(layouts.application(environment));
        let applied = Arc::new(applied);
        // Optional retention failure cannot revoke the new frame's reference.
        let _ = applications.insert(environment, function, applied.clone());
        Some(applied)
    }
}
