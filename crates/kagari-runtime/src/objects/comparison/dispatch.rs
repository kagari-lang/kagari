//! Apply compiler-authored receiver argument recipes to retained implementations.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{EnvironmentRecord, arguments::TypeArgument},
    gc::interfaces::{InterfaceResultBinding, ReceiverComparisonBinding},
    module::LoadedModule,
    objects::calls::interface_binding,
};
use kagari_contract::{
    callable::receiver::dispatch::{ReceiverCase, receiver_child},
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_types::ty::GenericParam;
use std::slice;

impl Runtime {
    pub(super) fn bind_receiver_dispatch(
        &self,
        binding: &InterfaceResultBinding,
        receiver: &TypeArgument,
        cases: &[ReceiverCase],
        scopes: &[LoadedModule],
    ) -> Result<Option<ReceiverComparisonBinding>, RuntimeError> {
        let invalid = || RuntimeError::module_validation("receiver comparison dispatch");
        let identity = receiver.identity(&binding.owner).ok_or_else(invalid)?;
        let mut selected = None;
        let mut unknown = false;
        for case in cases {
            self.resources().poll_execution()?;
            let module = binding.owner.definition(case.implementation)?.module();
            let owner = binding
                .owner
                .members()
                .find(|owner| &owner.bytecode.identity == module)
                .ok_or_else(invalid)?;
            let contract = owner
                .bytecode
                .public_items
                .iter()
                .find_map(|item| match item {
                    PublicItem::InterfaceTable(contract)
                        if contract.declaration == case.implementation =>
                    {
                        Some(contract)
                    }
                    _ => None,
                })
                .ok_or_else(invalid)?;
            let Some(arguments) =
                self.project_receiver_arguments(receiver, &binding.owner, case)?
            else {
                continue;
            };
            let mut environment = EnvironmentRecord::new(
                self.definition_context(),
                contract.generic_params.clone(),
                arguments,
            )?;
            environment.include(binding.environment.clone())?;
            environment.constructions =
                self.construction_scope(&binding.owner, binding.environment.as_ref())?;
            let environment = self.alloc_environment(environment)?;
            let applied = self.type_arguments(
                &owner,
                Some(environment.types.clone()),
                slice::from_ref(&contract.for_type),
            )?;
            if applied[0].identity(&owner).as_ref() != Some(&identity) {
                continue;
            }
            let application = ConcreteFunctionIdentity {
                declaration: case.implementation,
                arguments: contract
                    .generic_params
                    .iter()
                    .map(GenericParam::as_type)
                    .collect(),
            };
            let candidate = interface_binding(&owner, &application, Some(environment))?;
            let comparison = candidate.owner.bytecode.interface_tables[candidate.table]
                .comparison
                .as_ref()
                .and_then(|comparison| comparison.selection())
                .ok_or_else(invalid)?;
            match self.bind_comparison_conditions(
                &candidate,
                &contract.bounds,
                &comparison.operation,
                scopes,
            )? {
                Some(ReceiverComparisonBinding::Identity) => {}
                Some(choice) => {
                    if selected.is_some() {
                        return Err(invalid());
                    }
                    selected = Some(choice);
                }
                None => unknown = true,
            }
        }
        Ok(if unknown {
            None
        } else {
            Some(selected.unwrap_or(ReceiverComparisonBinding::Identity))
        })
    }

    fn project_receiver_arguments(
        &self,
        receiver: &TypeArgument,
        owner: &LoadedModule,
        case: &ReceiverCase,
    ) -> Result<Option<Vec<TypeArgument>>, RuntimeError> {
        let mut arguments = Vec::with_capacity(case.arguments.len());
        for path in &case.arguments {
            let mut argument = receiver.clone();
            for index in path {
                self.resources().poll_execution()?;
                if receiver_child(argument.ty(), index).is_none() {
                    return Ok(None);
                }
                argument = argument.derive(self, owner, |ty| receiver_child(ty, index).cloned())?;
            }
            arguments.push(argument);
        }
        Ok(Some(arguments))
    }
}
