//! Entry environments normalize checked IDs without rebinding lexical values or proofs.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{EnvironmentRecord, TypeEnvironment},
    module::{ExecutableModule, descriptors::SharedScope, dispatch::SelectedTarget},
};
use kagari_contract::callable::shared::SharedCall;

impl Runtime {
    /// Transport an already applied semantic environment to one exact selection.
    /// Type arguments, receiver relationships and construction recipes retain their
    /// original scopes; only binders and witness lookup keys change publication.
    /// The caller must publish the returned environment before a collection boundary.
    pub(crate) fn transport_environment(
        &self,
        source: &ExecutableModule,
        selected: &SelectedTarget,
        environment: &TypeEnvironment,
    ) -> Result<TypeEnvironment, RuntimeError> {
        self.validate_environment(environment.id)?;
        if source.publication_token() == selected.publication_token() {
            return Ok(environment.clone());
        }
        let mapping = selected.mapping_from(source.publication_definitions())?;
        let record = self
            .gc
            .environment(environment.id)
            .ok_or_else(|| RuntimeError::module_validation("selected entry environment"))?
            .clone();
        self.alloc_environment(record.transported(self, selected, &mapping)?)
    }

    /// Shared-call bridge for backend entry: bind evidence in the caller's exact
    /// scope, then normalize its keys for the selected body. No trait inference or
    /// implementation search occurs here. The selected target owns binder order.
    pub(crate) fn prepare_selected_shared_environment(
        &self,
        caller: &ExecutableModule,
        caller_environment: Option<TypeEnvironment>,
        selected: &SelectedTarget,
        contract: &SharedCall,
    ) -> Result<TypeEnvironment, RuntimeError> {
        self.validate_executable_module(caller)?;
        self.validate_executable_module(selected.owner())?;
        let scope = SharedScope {
            environment: caller_environment
                .as_ref()
                .map(|environment| environment.id),
            target_owner: selected.owner().key(),
            target: selected.target(),
        };
        if let Some(environment) = &caller_environment {
            self.validate_environment(environment.id)?;
        }
        if let Some(prepared) = self
            .modules
            .shared_environment(&self.gc, caller, &scope, contract)
        {
            return Ok(prepared);
        }
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::SharedPreparation);
        let body = selected
            .generic()
            .ok_or_else(|| RuntimeError::module_validation("selected shared body"))?;
        let arguments = self.type_arguments(
            caller,
            caller_environment
                .as_ref()
                .map(|environment| environment.types.clone()),
            &contract.arguments,
        )?;
        let operations = self
            .bind_operations_in(caller, caller_environment.clone(), &contract.operations)?
            .transported(self, selected)?;
        let constructions = self
            .bind_constructions_in(caller, caller_environment, &contract.constructions)?
            .transported(self, selected)?;
        let mut record = EnvironmentRecord::new(
            self.definition_context(),
            body.parameters.clone(),
            arguments,
        )?;
        record.set_constructions(constructions);
        record.extend_operations(operations);
        let prepared = self.alloc_environment(record)?;
        self.publish_shared_environment(caller, scope, contract, prepared.clone())?;
        Ok(prepared)
    }
}
