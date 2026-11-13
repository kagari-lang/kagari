//! Linked programs own applied calls, selected witnesses and shared environments.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge,
        application_key::ApplicationKey,
        applications::{AdmittedApplication, ApplicationId},
        call_contracts::{InterfaceCallSite, ScopedInterfaceCall},
        environments::EnvironmentId,
        managed::{ProtectedGraph, ProtectedMetadata, sources::MetadataSource},
    },
    frame::types::{TypeEnvironment, operations::OperationBindings},
    module::{
        ExecutableModule, ModuleKey, ModuleStore, ModuleStoreInner,
        descriptor_index::DescriptorIndex,
    },
    native::application::NativeApplication,
};
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_contract::callable::{shared::SharedCall, witness::OperationWitness};
use std::sync::Arc;

#[derive(Debug)]
struct Published<T> {
    value: T,
    dependencies: Vec<ExecutableModule>,
}

impl<T> Published<T> {
    fn is_available(&self, store: &ModuleStoreInner) -> bool {
        self.dependencies
            .iter()
            .all(|owner| store.resolve(owner).is_some())
    }
}

type WitnessIndex =
    DescriptorIndex<Option<EnvironmentId>, Arc<[OperationWitness]>, Published<OperationBindings>>;
type SharedIndex = DescriptorIndex<SharedScope, Arc<SharedCall>, Published<TypeEnvironment>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SharedScope {
    pub(crate) environment: Option<EnvironmentId>,
    pub(crate) target_owner: ModuleKey,
    pub(crate) target: CallableTarget,
}

#[derive(Debug, Default)]
pub(super) struct LinkedDescriptors {
    interface_calls:
        DescriptorIndex<EnvironmentId, InterfaceCallSite, Published<Arc<ScopedInterfaceCall>>>,
    applications: DescriptorIndex<(), ApplicationKey, Published<ApplicationId>>,
    witnesses: WitnessIndex,
    shared: SharedIndex,
    native: DescriptorIndex<EnvironmentId, NativeImportId, Published<Arc<NativeApplication>>>,
}

impl ModuleStore {
    pub(crate) fn interface_call(
        &self,
        owner: &ExecutableModule,
        environment: EnvironmentId,
        site: InterfaceCallSite,
    ) -> Option<Arc<ScopedInterfaceCall>> {
        let records = self.inner.try_borrow().ok()?;
        let entry = records
            .resolve(owner)?
            .descriptors
            .interface_calls
            .get(&environment, &site)?;
        entry.is_available(&records).then(|| entry.value.clone())
    }

    pub(crate) fn native_application(
        &self,
        owner: &ExecutableModule,
        import: NativeImportId,
        environment: EnvironmentId,
    ) -> Option<Arc<NativeApplication>> {
        let records = self.inner.try_borrow().ok()?;
        let entry = records
            .resolve(owner)?
            .descriptors
            .native
            .get(&environment, &import)?;
        entry.is_available(&records).then(|| entry.value.clone())
    }

    pub(crate) fn method_application(
        &self,
        owner: &ExecutableModule,
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
        owner: &ExecutableModule,
        environment: Option<EnvironmentId>,
        witnesses: &[OperationWitness],
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
        owner: &ExecutableModule,
        scope: &SharedScope,
        contract: &SharedCall,
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
    pub(crate) fn interface_call_is_protected(
        &mut self,
        protected: &[MetadataSource<'_>],
        call: &ScopedInterfaceCall,
    ) -> Result<bool, RuntimeError> {
        self.with_metadata_graph(protected, |cx, graph, resources| {
            Ok(graph
                .interface_call_dependencies(cx, call, resources)?
                .is_some())
        })
    }

    pub(crate) fn publish_interface_call(
        &mut self,
        owner: &ExecutableModule,
        environment: EnvironmentId,
        site: InterfaceCallSite,
        prepared: Arc<ScopedInterfaceCall>,
        protected: &[MetadataSource<'_>],
    ) -> Result<(), RuntimeError> {
        self.validate_executable_module(owner)?;
        let previous = self.modules.interface_call(owner, environment, site);
        let dependencies = self
            .with_metadata_graph(protected, |cx, graph, resources| {
                if let Some(previous) = &previous
                    && graph
                        .interface_call_dependencies(cx, previous, resources)?
                        .is_some()
                {
                    return Err(
                        RuntimeError::module_validation("duplicate prepared descriptor").into(),
                    );
                }
                graph.interface_call_dependencies(cx, &prepared, resources)
            })?
            .ok_or_else(|| {
                RuntimeError::module_validation("interface call dependencies are not protected")
            })?;
        let mut records = self.modules.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("records borrowed during call publication")
        })?;
        let record = records
            .resolve_mut(owner)
            .ok_or_else(|| RuntimeError::module_validation("invalid interface call owner"))?;
        let value = Published {
            value: prepared,
            dependencies,
        };
        match record
            .descriptors
            .interface_calls
            .replace(&environment, &site, value)
        {
            Ok(_) => Ok(()),
            Err(value) => record
                .descriptors
                .interface_calls
                .insert(environment, site, value),
        }
    }

    pub(crate) fn publish_native_application(
        &mut self,
        owner: &ExecutableModule,
        import: NativeImportId,
        prepared: Arc<NativeApplication>,
        protected: &[MetadataSource<'_>],
    ) -> Result<(), RuntimeError> {
        self.validate_executable_module(owner)?;
        let environment = prepared.environment.id;
        let previous = self.modules.native_application(owner, import, environment);
        let dependencies = self
            .with_metadata_graph(protected, |cx, graph, resources| {
                if let Some(previous) = &previous
                    && graph
                        .native_dependencies(cx, previous, resources)?
                        .is_some()
                {
                    return Err(
                        RuntimeError::module_validation("duplicate prepared descriptor").into(),
                    );
                }
                graph.native_dependencies(cx, &prepared, resources)
            })?
            .ok_or_else(|| {
                RuntimeError::module_validation("native cache providers are not protected")
            })?;
        let mut records = self.modules.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("module store is borrowed during native publication")
        })?;
        let record = records
            .resolve_mut(owner)
            .ok_or_else(|| RuntimeError::module_validation("invalid native cache owner"))?;
        let value = Published {
            value: prepared,
            dependencies,
        };
        match record
            .descriptors
            .native
            .replace(&environment, &import, value)
        {
            Ok(_) => Ok(()),
            Err(value) => record.descriptors.native.insert(environment, import, value),
        }
    }

    pub(crate) fn native_application_is_protected(
        &mut self,
        protected: &[MetadataSource<'_>],
        application: &NativeApplication,
    ) -> Result<bool, RuntimeError> {
        self.with_metadata_graph(protected, |cx, graph, resources| {
            Ok(graph
                .native_dependencies(cx, application, resources)?
                .is_some())
        })
    }

    pub(crate) fn publish_method_application(
        &mut self,
        owner: &ExecutableModule,
        key: ApplicationKey,
        prepared: &AdmittedApplication,
        protected: &[MetadataSource<'_>],
    ) -> Result<(), RuntimeError> {
        self.validate_executable_module(owner)?;
        let previous = self.modules.method_application(owner, &key);
        let dependencies = self.with_metadata_graph(protected, |cx, graph, resources| {
            if let Some(previous) = previous
                && graph
                    .find(cx, MetadataEdge::Application(previous), resources)?
                    .is_some()
            {
                return Err(
                    RuntimeError::module_validation("duplicate prepared descriptor").into(),
                );
            }
            let value = graph
                .find(cx, MetadataEdge::Application(prepared.id()), resources)?
                .ok_or_else(|| {
                    RuntimeError::module_validation("method application is not protected")
                })?;
            let ProtectedMetadata::Application(application) = value else {
                return Err(RuntimeError::module_validation("method application provider").into());
            };
            if !cx.read(application, |application| {
                Arc::ptr_eq(application.record(), prepared.record())
            }) {
                return Err(RuntimeError::module_validation("method application facts").into());
            }
            ProtectedGraph::new(cx, &[value], resources)?.programs(cx, resources)
        })?;
        let mut records = self.modules.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("module store is borrowed during method publication")
        })?;
        let record = records
            .resolve_mut(owner)
            .ok_or_else(|| RuntimeError::module_validation("invalid method cache owner"))?;
        let value = Published {
            value: prepared.id(),
            dependencies,
        };
        match record.descriptors.applications.replace(&(), &key, value) {
            Ok(_) => Ok(()),
            Err(value) => record.descriptors.applications.insert((), key, value),
        }
    }

    pub(crate) fn publish_operation_bindings(
        &mut self,
        owner: &ExecutableModule,
        environment: Option<EnvironmentId>,
        witnesses: &[OperationWitness],
        prepared: OperationBindings,
        protected: &[MetadataSource<'_>],
    ) -> Result<(), RuntimeError> {
        self.validate_executable_module(owner)?;
        let previous = self
            .modules
            .operation_bindings(owner, environment, witnesses);
        let dependencies = self
            .with_metadata_graph(protected, |cx, graph, resources| {
                if let Some(previous) = &previous
                    && graph
                        .operation_dependencies(cx, previous, resources)?
                        .is_some()
                {
                    return Err(
                        RuntimeError::module_validation("duplicate prepared descriptor").into(),
                    );
                }
                graph.operation_dependencies(cx, &prepared, resources)
            })?
            .ok_or_else(|| {
                RuntimeError::module_validation("operation cache dictionaries are not protected")
            })?;
        let mut records = self.modules.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("module store is borrowed during witness publication")
        })?;
        let record = records
            .resolve_mut(owner)
            .ok_or_else(|| RuntimeError::module_validation("invalid witness cache owner"))?;
        let value = Published {
            value: prepared,
            dependencies,
        };
        match record
            .descriptors
            .witnesses
            .replace(&environment, witnesses, value)
        {
            Ok(_) => Ok(()),
            Err(value) => {
                record
                    .descriptors
                    .witnesses
                    .insert(environment, Arc::from(witnesses), value)
            }
        }
    }

    pub(crate) fn operations_are_protected(
        &mut self,
        protected: &[MetadataSource<'_>],
        operations: &OperationBindings,
    ) -> Result<bool, RuntimeError> {
        self.with_metadata_graph(protected, |cx, graph, resources| {
            Ok(graph
                .operation_dependencies(cx, operations, resources)?
                .is_some())
        })
    }

    pub(crate) fn publish_shared_environment(
        &mut self,
        owner: &ExecutableModule,
        scope: SharedScope,
        contract: &SharedCall,
        prepared: TypeEnvironment,
        protected: &[MetadataSource<'_>],
    ) -> Result<(), RuntimeError> {
        self.validate_executable_module(owner)?;
        let previous = self.modules.shared_environment(owner, &scope, contract);
        let dependencies = self.with_metadata_graph(protected, |cx, graph, resources| {
            if let Some(previous) = &previous
                && graph.environment(cx, previous, resources)?.is_some()
            {
                return Err(
                    RuntimeError::module_validation("duplicate prepared descriptor").into(),
                );
            }
            let value = graph
                .environment(cx, &prepared, resources)?
                .ok_or_else(|| {
                    RuntimeError::module_validation("shared cache environment is not protected")
                })?;
            let dependencies =
                ProtectedGraph::new(cx, &[ProtectedMetadata::Environment(value)], resources)?;
            dependencies.programs(cx, resources)
        })?;
        let mut records = self.modules.inner.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("module store is borrowed during shared publication")
        })?;
        let record = records
            .resolve_mut(owner)
            .ok_or_else(|| RuntimeError::module_validation("invalid shared cache owner"))?;
        let value = Published {
            value: prepared,
            dependencies,
        };
        match record.descriptors.shared.replace(&scope, contract, value) {
            Ok(_) => Ok(()),
            Err(value) => {
                record
                    .descriptors
                    .shared
                    .insert(scope, Arc::new(contract.clone()), value)
            }
        }
    }
}
