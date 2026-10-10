//! Linked programs own applied calls, selected witnesses and shared environments.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge, application_key::ApplicationKey, applications::ApplicationId,
        environments::EnvironmentId,
    },
    frame::types::{TypeEnvironment, operations::OperationBindings},
    module::{
        LoadedModule, ModuleKey, ModuleStore, ModuleStoreInner,
        descriptor_index::{DescriptorIndex, Published},
    },
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::callable::{shared::SharedCall, witness::OperationWitness};
use std::sync::Arc;

type WitnessIndex = DescriptorIndex<
    Option<EnvironmentId>,
    Arc<[OperationWitness<DefinitionId>]>,
    OperationBindings,
>;
type SharedIndex = DescriptorIndex<SharedScope, Arc<SharedCall<DefinitionId>>, TypeEnvironment>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SharedScope {
    pub(crate) environment: Option<EnvironmentId>,
    pub(crate) target_owner: ModuleKey,
    pub(crate) target: CallableTarget,
}

#[derive(Debug, Default)]
pub(super) struct LinkedDescriptors {
    applications: DescriptorIndex<(), ApplicationKey, ApplicationId>,
    witnesses: WitnessIndex,
    shared: SharedIndex,
}

trait Descriptor {
    fn edge(&self) -> MetadataEdge<'_>;
}

impl Descriptor for ApplicationId {
    fn edge(&self) -> MetadataEdge<'_> {
        MetadataEdge::Application(*self)
    }
}

impl Descriptor for OperationBindings {
    fn edge(&self) -> MetadataEdge<'_> {
        MetadataEdge::Operations(self)
    }
}

impl Descriptor for TypeEnvironment {
    fn edge(&self) -> MetadataEdge<'_> {
        MetadataEdge::Environment(self.id)
    }
}

impl LinkedDescriptors {
    pub(super) fn trace<'a>(
        &'a self,
        store: &ModuleStoreInner,
        pending: &mut Vec<MetadataEdge<'a>>,
    ) {
        // Expiration removes optional retention, not validation of independently
        // rooted facts. All three graphs are immutable after publication.
        pending.extend(
            self.applications
                .values()
                .filter(|value| value.is_available(store))
                .map(|value| value.value.edge()),
        );
        pending.extend(
            self.witnesses
                .values()
                .filter(|value| value.is_available(store))
                .map(|value| value.value.edge()),
        );
        pending.extend(
            self.shared
                .values()
                .filter(|value| value.is_available(store))
                .map(|value| value.value.edge()),
        );
    }
}

impl ModuleStore {
    pub(crate) fn method_application(
        &self,
        owner: &LoadedModule,
        key: &ApplicationKey,
    ) -> Option<ApplicationId> {
        let records = self.inner.try_borrow().ok()?;
        let entry = records
            .resolve(owner)?
            .descriptors
            .applications
            .get(&(), key)?;
        entry.is_available(&records).then_some(entry.value)
    }

    pub(crate) fn operation_bindings(
        &self,
        owner: &LoadedModule,
        environment: Option<EnvironmentId>,
        witnesses: &[OperationWitness<DefinitionId>],
    ) -> Option<OperationBindings> {
        let records = self.inner.try_borrow().ok()?;
        let entry = records
            .resolve(owner)?
            .descriptors
            .witnesses
            .get(&environment, witnesses)?;
        entry.is_available(&records).then(|| entry.value.clone())
    }

    pub(crate) fn shared_environment(
        &self,
        owner: &LoadedModule,
        scope: &SharedScope,
        contract: &SharedCall<DefinitionId>,
    ) -> Option<TypeEnvironment> {
        let records = self.inner.try_borrow().ok()?;
        let entry = records
            .resolve(owner)?
            .descriptors
            .shared
            .get(scope, contract)?;
        entry.is_available(&records).then(|| entry.value.clone())
    }
}

impl Runtime {
    fn publish_descriptor<T: Descriptor>(
        &self,
        owner: &LoadedModule,
        value: T,
        install: impl FnOnce(&mut LinkedDescriptors, Published<T>) -> Result<(), RuntimeError>,
    ) -> Result<(), RuntimeError> {
        self.gc.ensure_execution_allowed()?;
        self.validate_metadata(MetadataEdge::Program(owner))?;
        let dependencies = self.metadata_dependencies(value.edge())?;
        let mut records = self.modules.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation(
                "module store is borrowed during descriptor publication",
            )
        })?;
        let record = records
            .resolve_mut(owner)
            .ok_or_else(|| RuntimeError::module_validation("invalid descriptor owner"))?;
        install(
            &mut record.descriptors,
            Published {
                value,
                dependencies,
            },
        )
    }

    pub(crate) fn publish_method_application(
        &self,
        owner: &LoadedModule,
        key: ApplicationKey,
        prepared: ApplicationId,
    ) -> Result<(), RuntimeError> {
        self.publish_descriptor(owner, prepared, |descriptors, value| {
            descriptors.applications.insert((), key, value)
        })
    }

    pub(crate) fn publish_operation_bindings(
        &self,
        owner: &LoadedModule,
        environment: Option<EnvironmentId>,
        witnesses: &[OperationWitness<DefinitionId>],
        prepared: OperationBindings,
    ) -> Result<(), RuntimeError> {
        self.publish_descriptor(owner, prepared, |descriptors, value| {
            descriptors
                .witnesses
                .insert(environment, Arc::from(witnesses), value)
        })
    }

    pub(crate) fn publish_shared_environment(
        &self,
        owner: &LoadedModule,
        scope: SharedScope,
        contract: &SharedCall<DefinitionId>,
        prepared: TypeEnvironment,
    ) -> Result<(), RuntimeError> {
        self.publish_descriptor(owner, prepared, |descriptors, value| {
            descriptors
                .shared
                .insert(scope, Arc::new(contract.clone()), value)
        })
    }
}
