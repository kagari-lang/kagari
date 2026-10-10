//! Runtime-local execution links are admitted with their supplying program.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge,
        call_contracts::{InterfaceCallSite, ScopedInterfaceCall},
    },
    module::{
        LoadedModule, ModuleStore, constants::ConstantPool, execution::calls::PreparedCallTarget,
    },
};
use kagari_bytecode::instruction::BytecodeInstruction;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::ids::FunctionRef;
use kagari_types::ty::Ty;
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct LinkedFunction {
    pub(crate) constants: Arc<ConstantPool>,
    calls: Box<[Option<Arc<ScopedInterfaceCall>>]>,
    fields: Box<[Option<LinkedField>]>,
}

#[derive(Debug)]
struct LinkedField {
    arguments: Box<[Ty<DefinitionId>]>,
}

impl LinkedFunction {
    pub(crate) fn field_arguments(&self, index: usize) -> Option<&[Ty<DefinitionId>]> {
        Some(&self.fields.get(index)?.as_ref()?.arguments)
    }

    pub(crate) fn call(&self, index: usize) -> Option<&Arc<ScopedInterfaceCall>> {
        self.calls.get(index)?.as_ref()
    }
}

#[derive(Debug)]
pub(super) struct LinkedExecution {
    functions: Box<[Option<Arc<LinkedFunction>>]>,
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
            let fields = if prepared.has_scoped_fields {
                prepared
                    .fields
                    .iter()
                    .map(|operation| {
                        if operation.concrete {
                            return Ok(None);
                        }
                        let field = match &function.instructions[operation.pc] {
                            BytecodeInstruction::ReadAggregateField { field, .. }
                            | BytecodeInstruction::WriteAggregateField { field, .. } => field,
                            _ => {
                                return Err(RuntimeError::module_validation(
                                    "invalid linked field origin",
                                ));
                            }
                        };
                        Ok(Some(LinkedField {
                            arguments: field.arguments.clone().into_boxed_slice(),
                        }))
                    })
                    .collect::<Result<Box<[_]>, RuntimeError>>()?
            } else {
                Box::default()
            };
            functions.push(Some(Arc::new(LinkedFunction {
                fields,
                constants: constants.clone(),
                calls: calls.into_boxed_slice(),
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
        });
        Ok(())
    }
}

impl ModuleStore {
    pub(crate) fn linked_function(
        &self,
        owner: &LoadedModule,
        function: FunctionRef,
    ) -> Option<Arc<LinkedFunction>> {
        let records = self.inner.try_borrow().ok()?;
        records
            .resolve(owner)?
            .execution
            .as_ref()?
            .functions
            .get(function.index())?
            .clone()
    }
}
