//! Native applications can have several exact, module-owned witness environments.
use crate::{
    types::ConcreteFunctionIdentity,
    updates::{
        BodyApplication, DemandEnvelope,
        demands::{Walker, apply, apply_instance, parameters},
        limits::{Budget, UpdateError},
    },
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::{Ty, substitution::TypeSubstitution};

impl Walker<'_, '_> {
    /// Return every checked importing environment, never an arbitrary representative.
    /// Generated adapter placement is not part of the native application's identity,
    /// but remains part of each exact witness and its executable coverage demand.
    pub(super) fn find_natives(
        &mut self,
        instance: &ConcreteFunctionIdentity,
        binding: Option<DefinitionId>,
        budget: &mut Budget<'_>,
    ) -> Result<Vec<usize>, UpdateError> {
        let mut found: Vec<usize> = Vec::new();
        for &index in self.native_indices.get(instance).into_iter().flatten() {
            budget.visit()?;
            let native = &self.coverage.natives[index];
            if binding.is_some_and(|id| native.import.binding != id) {
                return Err(UpdateError::InvalidEvidence);
            }
            if !self.native_validated.contains(&index) {
                let declaration = self
                    .catalog
                    .native_declaration(instance.declaration)
                    .ok_or_else(|| UpdateError::MissingNative(instance.clone()))?;
                if !native
                    .import
                    .matches_declaration(declaration, self.catalog, budget.cancel)?
                {
                    return Err(UpdateError::InvalidEvidence);
                }
                self.native_validated.insert(index);
            }
            if let Some(previous) = found.first() {
                let previous = self.coverage.natives[*previous].import;
                // Calling convention and entry contract must agree. Only selected
                // callable/result witnesses and trusted callback closures may differ.
                if previous.binding != native.import.binding
                    || previous.signature != native.import.signature
                    || previous.generic != native.import.generic
                    || previous.requirements != native.import.requirements
                    || previous.receiver_access != native.import.receiver_access
                {
                    return Err(UpdateError::InvalidEvidence);
                }
            }
            budget.records(1)?;
            found.push(index);
        }
        Ok(found)
    }

    pub(super) fn native_arguments(
        &mut self,
        index: usize,
        arguments: &[Ty],
        envelope: &DemandEnvelope,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        let native = &self.coverage.natives[index];
        let params = native
            .import
            .generic
            .as_ref()
            .map_or(&[][..], |generic| generic.parameters.as_slice());
        if params.len() != arguments.len() {
            return Err(UpdateError::InvalidEvidence);
        }
        let mut bindings = TypeSubstitution::default();
        for (parameter, argument) in params.iter().zip(arguments) {
            bindings.bind(&parameter.owner, parameter.position, argument);
        }
        self.native(index, &bindings, envelope, budget)
    }

    pub(super) fn native(
        &mut self,
        index: usize,
        bindings: &TypeSubstitution<'_>,
        envelope: &DemandEnvelope,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        let native = &self.coverage.natives[index];
        let import = native.import;
        let arguments = apply(&parameters(import.generic.as_ref()), bindings, budget)?;
        let symbolic = !arguments.iter().all(Ty::is_concrete);
        let key = BodyApplication {
            body: if symbolic {
                import.instance.clone()
            } else {
                apply_instance(&import.instance, bindings, budget)?
            },
            arguments: if symbolic {
                parameters(import.generic.as_ref())
            } else {
                arguments
            },
        };
        budget.application(&key)?;
        // The index belongs to this immutable candidate view, not to wire identity.
        // Equal applications must not suppress another import's distinct witnesses.
        if !self.native_seen.insert((index, key)) {
            return Ok(());
        }
        let scope = import.generic.as_ref();
        if let Some(callbacks) = native.callbacks {
            for callback in callbacks {
                self.call(callback, bindings, scope, envelope, budget)?;
            }
        }
        // None remains unknown. This traversal proves coverage, not sealed closure.
        for operation in &import.callables {
            self.operation(operation, bindings, scope, envelope, budget)?;
        }
        if let Some(adapter) = &import.result_adapter {
            self.construction(&adapter.implementation, bindings, scope, budget)?;
        }
        Ok(())
    }
}
