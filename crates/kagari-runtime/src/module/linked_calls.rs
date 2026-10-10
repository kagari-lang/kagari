//! Closed call contracts are immutable links owned by their supplying program.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge,
        call_contracts::{InterfaceCallSite, ScopedInterfaceCall},
    },
    module::{LoadedModule, ModuleStore, execution::calls::PreparedCallTarget},
};
use kagari_contract::ids::FunctionRef;
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct LinkedFunctionCalls {
    calls: Box<[Option<Arc<ScopedInterfaceCall>>]>,
}

impl LinkedFunctionCalls {
    pub(crate) fn get(&self, index: usize) -> Option<&Arc<ScopedInterfaceCall>> {
        self.calls.get(index)?.as_ref()
    }
}

#[derive(Debug)]
pub(super) struct LinkedCalls {
    functions: Box<[Option<Arc<LinkedFunctionCalls>>]>,
}

impl LinkedCalls {
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
    pub(crate) fn link_interface_calls(&self, owner: &LoadedModule) -> Result<(), RuntimeError> {
        let mut functions = Vec::with_capacity(owner.bytecode.functions.len());
        for function in &owner.bytecode.functions {
            let prepared = &owner.execution().functions[function.id.index()];
            if !prepared.has_closed_interface_calls {
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
            functions.push(Some(Arc::new(LinkedFunctionCalls {
                calls: calls.into_boxed_slice(),
            })));
        }
        let mut records = self.modules.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("module store borrowed during call linking")
        })?;
        let record = records
            .resolve_mut(owner)
            .ok_or_else(|| RuntimeError::module_validation("invalid linked call owner"))?;
        if record.calls.is_some() {
            return Err(RuntimeError::module_validation("duplicate linked calls"));
        }
        record.calls = Some(LinkedCalls {
            functions: functions.into_boxed_slice(),
        });
        Ok(())
    }
}

impl ModuleStore {
    pub(crate) fn closed_interface_calls(
        &self,
        owner: &LoadedModule,
        function: FunctionRef,
    ) -> Option<Arc<LinkedFunctionCalls>> {
        let records = self.inner.try_borrow().ok()?;
        records
            .resolve(owner)?
            .calls
            .as_ref()?
            .functions
            .get(function.index())?
            .clone()
    }
}
