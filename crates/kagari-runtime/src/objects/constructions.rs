//! Bind and execute construction capabilities without runtime implementation search.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::application_key::AdapterIdentity,
    frame::types::{
        EnvironmentRecord, TypeEnvironment,
        arguments::TypeArgument,
        bindings::AssociatedInterface,
        construction::{ConstructionBinding, ConstructionKey, ConstructionScope},
    },
    module::ExecutableModule,
    objects::calls::interface_binding,
    value::Value,
};
use kagari_contract::callable::{
    construction::evidence::{ConstructionRequirement, ConstructionSelection, ConstructionWitness},
    witness::OperationWitness,
};
use kagari_types::ty::Ty;
use std::collections::BTreeMap;

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("generic construction capability")
}

impl Runtime {
    pub(crate) fn bind_constructions_in(
        &self,
        owner: &ExecutableModule,
        environment: Option<TypeEnvironment>,
        witnesses: &[ConstructionWitness],
    ) -> Result<ConstructionScope, RuntimeError> {
        let caller = self.construction_scope(owner, environment.as_ref())?;
        let mut scope = caller.clone();
        for witness in witnesses {
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
                        owner,
                        environment.clone(),
                        operations,
                        constructions,
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
            let Ty::Trait(applied) = interface.ty() else {
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
                receiver: receiver.ty().clone(),
                interface: applied.clone(),
                owner: owner.clone(),
                outputs,
            };
            scope.insert(ConstructionBinding {
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
    pub(super) fn construction_dependencies(
        &self,
        owner: &ExecutableModule,
        parent: Option<TypeEnvironment>,
        operations: &[OperationWitness],
        constructions: &[ConstructionWitness],
    ) -> Result<Option<TypeEnvironment>, RuntimeError> {
        if operations.is_empty() && constructions.is_empty() {
            return Ok(parent);
        }
        let operations = self.bind_operations_in(owner, parent.clone(), operations)?;
        let constructions = self.bind_constructions_in(owner, parent.clone(), constructions)?;
        let mut environment = EnvironmentRecord::new(self.definition_context(), vec![], vec![])?;
        environment.include(parent)?;
        environment.set_constructions(constructions);
        environment.extend_operations(operations);
        Ok(Some(self.alloc_environment(environment)?))
    }

    /// Construct using a checked body's exact capability and supplying generation.
    /// Returns an error for stale environments, foreign values or absent evidence.
    pub fn make_bound_interface(
        &self,
        owner: &ExecutableModule,
        environment: &TypeEnvironment,
        required: &ConstructionRequirement,
        data: Value,
    ) -> Result<Value, RuntimeError> {
        self.validate_executable_module(owner)?;
        let types = self.type_arguments(
            owner,
            Some(environment.types.clone()),
            &[
                required.receiver.clone(),
                Ty::Trait(required.interface.clone()),
            ],
        )?;
        let receiver = types[0].identity(owner).ok_or_else(invalid)?;
        let interface = types[1].identity(owner).ok_or_else(invalid)?;
        let binding = self
            .gc
            .environment(environment.id)
            .ok_or_else(invalid)?
            .constructions
            .selected(&receiver, &interface)
            .ok_or_else(invalid)?
            .clone();
        let Ty::Trait(target) = binding.interface.ty() else {
            return Err(invalid());
        };
        if let Some(selection) = &binding.selection {
            let arguments: Vec<TypeArgument> = self.type_arguments(
                &binding.owner,
                selection
                    .environment
                    .as_ref()
                    .map(|environment| environment.types.clone()),
                &selection.arguments,
            )?;
            let snapshot = self.prepare_interface_snapshot(
                &selection.owner,
                selection.table,
                &arguments,
                selection.environment.as_ref(),
                data,
                false,
            )?;
            let source = snapshot.description.interface_type().clone();
            let value = self.publish_interface(snapshot)?;
            self.upcast_interface(&value, &source, target)
        } else {
            let Ty::Trait(source) = types[0].ty() else {
                return Err(invalid());
            };
            self.upcast_interface(&data, source, target)
        }
    }
}
