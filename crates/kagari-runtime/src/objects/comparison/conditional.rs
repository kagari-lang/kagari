//! Bind positive conditional evidence from retained, independently checked operations.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::groups::OperationId,
    frame::types::{
        EnvironmentRecord, arguments::TypeArgument, compatibility::TypeIdentity,
        construction::ConstructionScope, operations::OperationBindings,
    },
    gc::interfaces::InterfaceResultBinding,
    module::LoadedModule,
    objects::adapters::AdapterApplication,
};
use kagari_contract::callable::witness::OperationWitness;
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    language::Protocol,
    ty::{Constraint, Ty},
};
use std::slice;

impl Runtime {
    pub(super) fn bind_conditional_comparison(
        &self,
        binding: &InterfaceResultBinding,
        scopes: &[LoadedModule],
    ) -> Result<Option<InterfaceResultBinding>, RuntimeError> {
        let Some(comparison) = &binding.owner.bytecode.interface_tables[binding.table].comparison
        else {
            return Ok(None);
        };
        if comparison.conditions.is_empty() {
            return Ok(None);
        }
        let mut evidence = ComparisonEvidence {
            runtime: self,
            fallback: &binding.owner,
            scopes,
            remaining: 100_000,
        };
        let mut operations = OperationBindings::default();
        for condition in &comparison.conditions {
            // A methodless marker (for example Eq) needs its own eligibility
            // certificate. Merely finding another callable cannot prove it.
            if condition.constraints.iter().any(|constraint| {
                !matches!(constraint, Constraint::Trait(interface)
                    if interface == &comparison.operation.requirement().interface
                        && Protocol::from_definition(&interface.declaration, binding.owner.definitions())
                            == Some(Protocol::PartialEq))
            }) {
                return Ok(None);
            }
            let argument = self
                .type_arguments(
                    &binding.owner,
                    binding.environment.as_ref().map(|env| env.types.clone()),
                    slice::from_ref(&condition.ty),
                )?
                .remove(0);
            let required = NativeCallableRequirement {
                receiver: argument.ty().clone(),
                ..comparison.operation.requirement().clone()
            };
            let Some(operation) = evidence.operation(&argument, &required, 0)? else {
                // Unavailable evidence is deliberately not a negative certificate.
                return Ok(None);
            };
            operations.push(&self.gc, operation)?;
        }
        let mut environment = EnvironmentRecord::new(self.definition_context(), vec![], vec![])?;
        environment.include(binding.environment.clone())?;
        environment.constructions =
            self.construction_scope(&binding.owner, binding.environment.as_ref())?;
        environment.extend_operations(operations);
        Ok(Some(InterfaceResultBinding {
            environment: Some(self.alloc_environment(environment)?),
            ..binding.clone()
        }))
    }
}

struct ComparisonEvidence<'a> {
    runtime: &'a Runtime,
    fallback: &'a LoadedModule,
    scopes: &'a [LoadedModule],
    remaining: usize,
}

impl ComparisonEvidence<'_> {
    fn step(&mut self, depth: usize) -> Result<(), RuntimeError> {
        if depth >= 64 || self.remaining == 0 {
            return Err(RuntimeError::module_validation(
                "comparison evidence preparation limit",
            ));
        }
        self.remaining -= 1;
        Ok(())
    }

    fn operation(
        &mut self,
        argument: &TypeArgument,
        required: &NativeCallableRequirement,
        depth: usize,
    ) -> Result<Option<OperationId>, RuntimeError> {
        self.step(depth)?;
        let invalid = || RuntimeError::module_validation("conditional comparison evidence");
        let identity = argument.identity(self.fallback).ok_or_else(invalid)?;
        if let Some(operation) = self.selected_operation(required, &identity, depth)? {
            return Ok(Some(operation));
        }
        let Ty::Tuple(items) = argument.ty() else {
            return Ok(None);
        };
        // Only the canonical component bounds can be supplied here. A checked
        // body with additional requirements is not a suitable adapter. Rebind
        // components individually, preserving their actual argument provenance.
        for scope in self.scopes {
            for owner in scope.members() {
                for target in &owner.bytecode.functions {
                    self.step(depth)?;
                    let semantic = &target.metadata.semantic;
                    let (Some(adapter), Some(body)) =
                        (&semantic.protocol_adapter, &semantic.generic)
                    else {
                        continue;
                    };
                    if adapter.interface != required.interface
                        || adapter.member != required.member
                        || body.parameters.len() != items.len()
                        || !matches!(adapter.receiver, Ty::Tuple(_))
                        || body.bounds.len() != body.parameters.len()
                        || body
                            .parameters
                            .iter()
                            .zip(&body.bounds)
                            .any(|(parameter, bound)| {
                                bound.ty != parameter.as_type()
                                    || bound.constraints.as_slice()
                                        != [Constraint::Trait(required.interface.clone())]
                            })
                    {
                        continue;
                    }
                    let mut arguments = Vec::with_capacity(items.len());
                    let mut operations = OperationBindings::default();
                    for index in 0..items.len() {
                        let item = argument.parameter(self.runtime, self.fallback, index)?;
                        let component = NativeCallableRequirement {
                            receiver: item.ty().clone(),
                            ..required.clone()
                        };
                        let Some(operation) = self.operation(&item, &component, depth + 1)? else {
                            return Ok(None);
                        };
                        operations.push(&self.runtime.gc, operation)?;
                        arguments.push(item);
                    }
                    let mut environment = EnvironmentRecord::new(
                        self.runtime.definition_context(),
                        body.parameters.clone(),
                        arguments,
                    )?;
                    environment.extend_operations(operations);
                    for scope in self.scopes {
                        environment
                            .constructions
                            .extend(&ConstructionScope::new(scope));
                    }
                    return self
                        .runtime
                        .bind_adapter_application(
                            &owner,
                            target,
                            AdapterApplication {
                                environment,
                                requirement: required.clone(),
                                receiver_identity: identity,
                            },
                        )
                        .map(Some);
                }
            }
        }
        Ok(None)
    }

    fn selected_operation(
        &mut self,
        required: &NativeCallableRequirement,
        identity: &TypeIdentity,
        depth: usize,
    ) -> Result<Option<OperationId>, RuntimeError> {
        let invalid = || RuntimeError::module_validation("closed comparison evidence");
        for scope in self.scopes {
            for owner in scope.members() {
                // These closed applications were proved with no open caller
                // assumptions. Inspect selected records, never implementation
                // headers or a live module catalog.
                let witnesses = owner
                    .bytecode
                    .native_imports
                    .iter()
                    .filter(|import| import.generic.is_none())
                    .flat_map(|import| &import.callables)
                    .chain(
                        owner
                            .bytecode
                            .interface_tables
                            .iter()
                            .filter(|table| table.arguments.iter().all(Ty::is_concrete))
                            .filter_map(|table| table.comparison.as_ref())
                            .filter(|comparison| comparison.conditions.is_empty())
                            .map(|comparison| &comparison.operation),
                    );
                for witness in witnesses.flat_map(OperationWitness::walk) {
                    self.step(depth)?;
                    if witness.requirement() != required
                        || witness.walk().any(|child| {
                            matches!(child, OperationWitness::Forward(_))
                                || !child.requirement().receiver.is_concrete()
                        })
                    {
                        continue;
                    }
                    let selected = self
                        .runtime
                        .resolve_type_arguments(&owner, slice::from_ref(&required.receiver))?;
                    if selected[0].identity(&owner).as_deref() != Some(identity) {
                        continue;
                    }
                    let operations =
                        self.runtime
                            .bind_operations_in(&owner, None, slice::from_ref(witness))?;
                    return operations
                        .operation(&self.runtime.gc, required, identity)
                        .map(Some)
                        .ok_or_else(invalid);
                }
            }
        }
        Ok(None)
    }
}
