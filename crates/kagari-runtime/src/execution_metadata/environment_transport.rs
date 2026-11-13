//! Entry environments normalize checked IDs without rebinding lexical values or proofs.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::{RuntimeError, reason::StaleHandleReason},
    execution::roots::SessionId,
    execution_metadata::{environments::AdmittedEnvironment, managed::sources::MetadataSource},
    frame::types::{EnvironmentRecord, TypeEnvironment},
    module::{ExecutableModule, descriptors::SharedScope, dispatch::SelectedTarget},
};
use kagari_contract::callable::shared::SharedCall;

impl Runtime {
    /// Transport an already applied semantic environment to one exact selection.
    /// Type arguments, receiver relationships and construction recipes retain their
    /// original scopes; only binders and witness lookup keys change publication.
    /// The destination dependency suffix protects the result through collection.
    /// Publish its actual edge into the frame/object before retiring preparation.
    pub(crate) fn transport_environment(
        &mut self,
        execution: SessionId,
        source: &ExecutableModule,
        selected: &SelectedTarget,
        environment: &AdmittedEnvironment,
    ) -> Result<AdmittedEnvironment, RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        // Transport reads accepted immutable identity facts, not the old body's
        // executable state. A named relationship may outlive that mutable revision.
        // Actual retained dependencies are admitted through the environment graph;
        // only the selected computation requires executable availability here.
        if !source.belongs_to(self.host.owner()) {
            return Err(StaleHandleReason::ExecutableModuleBelongsToAnotherRuntime.into());
        }
        self.validate_executable_module(selected.owner())?;
        if source.publication_token() == selected.publication_token() {
            self.validate_environment(environment)?;
            return Ok(environment.clone());
        }
        let mapping = selected.mapping_from(source.publication_definitions())?;
        let snapshot = self.environment_snapshot(environment)?;
        let protected = [MetadataSource::AdmittedEnvironment(environment)];
        let record = snapshot
            .record
            .transported(self, selected, &mapping, &snapshot.groups)?;
        self.alloc_environment(execution, record, &protected)
    }

    /// Shared-call bridge for backend entry: bind evidence in the caller's exact
    /// scope, then normalize its keys for the selected body. No trait inference or
    /// implementation search occurs here. The selected target owns binder order.
    pub(crate) fn prepare_selected_shared_environment(
        &mut self,
        execution: SessionId,
        caller: &ExecutableModule,
        caller_environment: Option<TypeEnvironment>,
        selected: &SelectedTarget,
        contract: &SharedCall,
        protected: &mut Vec<MetadataSource<'_>>,
    ) -> Result<AdmittedEnvironment, RuntimeError> {
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
            self.validate_environment_in(protected, environment)?;
        }
        if let Some(prepared) = self.modules.shared_environment(caller, &scope, contract)
            && let Some(prepared) = self.copy_environment_in(execution, &prepared, protected)?
        {
            protected
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("shared preparation sources"))?;
            protected.push(MetadataSource::Environment(prepared.source()));
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
        let operations = self.bind_operations_in(
            execution,
            caller,
            caller_environment.clone(),
            &contract.operations,
            protected,
        )?;
        let groups = self.with_metadata_graph(protected, |cx, graph, resources| {
            graph.operation_groups(cx, resources)
        })?;
        let operations = operations.transported(self, selected, &groups)?;
        let constructions = self
            .bind_constructions_in(
                execution,
                caller,
                caller_environment,
                &contract.constructions,
                protected,
            )?
            .transported(self, selected)?;
        let mut record = EnvironmentRecord::new(
            selected.owner().definitions(),
            body.parameters.clone(),
            arguments,
        )?;
        record.set_constructions(constructions);
        record.extend_operations(operations);
        protected
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("shared preparation sources"))?;
        let prepared = self.alloc_environment(execution, record, protected)?;
        protected.push(MetadataSource::Environment(prepared.source()));
        self.publish_shared_environment(
            caller,
            scope,
            contract,
            prepared.facts().clone(),
            protected,
        )?;
        Ok(prepared)
    }
}
