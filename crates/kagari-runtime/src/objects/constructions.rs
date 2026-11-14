//! Bind and execute construction capabilities without runtime implementation search.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::SessionId,
    execution_metadata::{application_key::AdapterIdentity, managed::sources::MetadataSource},
    frame::types::{
        EnvironmentRecord, TypeEnvironment,
        bindings::AssociatedInterface,
        construction::{ConstructionBinding, ConstructionKey, ConstructionScope},
    },
    module::ExecutableModule,
    objects::calls::interface_binding,
};
use kagari_contract::callable::{
    construction::evidence::{ConstructionSelection, ConstructionWitness},
    witness::OperationWitness,
};
use kagari_types::ty::Ty;
use std::collections::BTreeMap;

pub(crate) struct ConstructionEvidence<'evidence> {
    pub(crate) operations: &'evidence [OperationWitness],
    pub(crate) constructions: &'evidence [ConstructionWitness],
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("generic construction capability")
}

impl Runtime {
    pub(crate) fn bind_constructions_in(
        &mut self,
        execution: SessionId,
        owner: &ExecutableModule,
        environment: Option<TypeEnvironment>,
        witnesses: &[ConstructionWitness],
        protected: &mut Vec<MetadataSource<'_>>,
    ) -> Result<ConstructionScope, RuntimeError> {
        let caller = self.construction_scope(owner, environment.as_ref(), protected)?;
        let mut scope = caller.clone();
        for (index, witness) in witnesses.iter().enumerate() {
            if index.is_multiple_of(1024) {
                self.resources.poll_execution()?;
            }
            let types = environment
                .as_ref()
                .map(|environment| environment.types.clone());
            let mut arguments = self.type_arguments(
                owner,
                types,
                &[
                    witness.requirement.receiver.clone(),
                    Ty::Trait(witness.requirement.interface.clone()),
                ],
            )?;
            let interface = arguments.pop().ok_or_else(invalid)?;
            let receiver = arguments.pop().ok_or_else(invalid)?;
            let receiver_identity = receiver.identity(owner).ok_or_else(invalid)?;
            let interface_identity = interface.identity(owner).ok_or_else(invalid)?;
            if matches!(witness.selection, ConstructionSelection::Forward) {
                // Search only the caller's exact capabilities, never its table inventory.
                caller
                    .selected(&receiver_identity, &interface_identity)
                    .ok_or_else(invalid)?;
                continue;
            }
            let selection = match &witness.selection {
                ConstructionSelection::Declared {
                    implementation,
                    operations,
                    constructions,
                } => {
                    let environment = self.construction_dependencies(
                        execution,
                        owner,
                        environment.clone(),
                        ConstructionEvidence {
                            operations,
                            constructions,
                        },
                        protected,
                    )?;
                    Some(interface_binding(owner, implementation, environment)?)
                }
                ConstructionSelection::Interface => None,
                ConstructionSelection::Forward => unreachable!("forwarded above"),
            };
            let key = ConstructionKey {
                receiver: receiver_identity,
                interface: interface_identity,
                selection: selection.as_ref().map(|binding| AdapterIdentity {
                    owner: binding.owner.key(),
                    table: binding.table,
                    arguments: binding.arguments.clone(),
                    environment: binding
                        .environment
                        .as_ref()
                        .map(|environment| environment.id),
                }),
            };
            // Lookup binders stay lexical, as do operation-provided associated
            // facts. The TypeArgument above separately owns the closed key.
            let resolve = |ty: &Ty| match &environment {
                Some(environment) => environment.types.resolve(ty),
                None => Ok(ty.clone()),
            };
            let Ty::Trait(applied) = resolve(&Ty::Trait(witness.requirement.interface.clone()))?
            else {
                return Err(invalid());
            };
            let mut outputs = BTreeMap::new();
            for member in applied.associated_types.keys() {
                outputs.insert(
                    *member,
                    interface.derive(self, owner, |ty| {
                        let Ty::Trait(interface) = ty else {
                            return None;
                        };
                        interface.associated_types.get(member).cloned()
                    })?,
                );
            }
            let associated = AssociatedInterface {
                lookup_scope: None,
                receiver: resolve(&witness.requirement.receiver)?,
                interface: applied,
                owner: owner.clone(),
                outputs,
            };
            scope.insert(ConstructionBinding {
                lookup_scope: None,
                key,
                interface,
                owner: owner.clone(),
                selection,
                associated,
            });
        }
        Ok(scope)
    }

    /// Receiver-bound dependencies use the same environment owner as shared calls.
    /// Immutable parent type facts and explicit executable edges retain provenance.
    pub(crate) fn construction_dependencies(
        &mut self,
        execution: SessionId,
        owner: &ExecutableModule,
        parent: Option<TypeEnvironment>,
        evidence: ConstructionEvidence<'_>,
        protected: &mut Vec<MetadataSource<'_>>,
    ) -> Result<Option<TypeEnvironment>, RuntimeError> {
        let ConstructionEvidence {
            operations,
            constructions,
        } = evidence;
        if operations.is_empty() && constructions.is_empty() {
            return Ok(parent);
        }
        let operations =
            self.bind_operations_in(execution, owner, parent.clone(), operations, protected)?;
        let constructions =
            self.bind_constructions_in(execution, owner, parent.clone(), constructions, protected)?;
        let mut environment = EnvironmentRecord::new(owner.definitions(), vec![], vec![])?;
        environment.include(parent)?;
        environment.set_constructions(constructions);
        environment.extend_operations(operations);
        protected
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("construction metadata sources"))?;
        let environment = self.alloc_environment(execution, environment, protected)?;
        protected.push(MetadataSource::Environment(environment.source()));
        Ok(Some(environment.facts().clone()))
    }
}
