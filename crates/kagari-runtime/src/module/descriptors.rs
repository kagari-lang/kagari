//! Linked programs own applied calls, selected witnesses and shared environments.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge,
        application_key::ApplicationKey,
        applications::ApplicationId,
        call_contracts::{InterfaceCallSite, ScopedInterfaceCall},
        environments::EnvironmentId,
    },
    frame::types::{TypeEnvironment, operations::OperationBindings},
    gc::GcHeap,
    module::{
        ExecutableModule, ModuleKey, ModuleStore, ModuleStoreInner,
        descriptor_index::{DescriptorIndex, DescriptorSlot},
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

impl<T: Descriptor + Clone> Published<T> {
    fn promote(&self, modules: &ModuleStore, store: &ModuleStoreInner, gc: &GcHeap) -> Option<T> {
        if !self
            .dependencies
            .iter()
            .all(|owner| store.resolve(owner).is_some())
        {
            return None;
        }
        gc.publish_metadata_edge(modules, self.value.edge()).ok()?;
        Some(self.value.clone())
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

trait Descriptor {
    fn edge(&self) -> MetadataEdge<'_>;
}

impl Descriptor for Arc<ScopedInterfaceCall> {
    fn edge(&self) -> MetadataEdge<'_> {
        MetadataEdge::InterfaceCall(self)
    }
}

impl Descriptor for Arc<NativeApplication> {
    fn edge(&self) -> MetadataEdge<'_> {
        MetadataEdge::NativeApplication(self)
    }
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

pub(super) struct DescriptorView<'a> {
    pub(super) dependencies: &'a [ExecutableModule],
    pub(super) edge: MetadataEdge<'a>,
}

impl DescriptorView<'_> {
    pub(super) fn field_count(&self) -> usize {
        match self.edge {
            MetadataEdge::Operations(operations) => operations.scan_len(),
            _ => 1,
        }
    }

    pub(super) fn field(&self, index: usize) -> Option<MetadataEdge<'_>> {
        match self.edge {
            MetadataEdge::Operations(operations) => operations.scan_field(index),
            edge => (index == 0).then_some(edge),
        }
    }
}

impl LinkedDescriptors {
    pub(super) fn scan_extents(&self) -> [usize; 5] {
        [
            self.interface_calls.scan_extent(),
            self.native.scan_extent(),
            self.applications.scan_extent(),
            self.witnesses.scan_extent(),
            self.shared.scan_extent(),
        ]
    }

    pub(super) fn scan_start(&mut self, kind: usize, index: usize) -> Option<DescriptorSlot> {
        match kind {
            0 => self.interface_calls.scan_start(index),
            1 => self.native.scan_start(index),
            2 => self.applications.scan_start(index),
            3 => self.witnesses.scan_start(index),
            4 => self.shared.scan_start(index),
            _ => None,
        }
    }

    pub(super) fn scan_view(
        &self,
        kind: usize,
        slot: DescriptorSlot,
    ) -> Option<DescriptorView<'_>> {
        fn view<T: Descriptor>(value: &Published<T>) -> DescriptorView<'_> {
            DescriptorView {
                dependencies: &value.dependencies,
                edge: value.value.edge(),
            }
        }
        match kind {
            0 => self.interface_calls.scan_entry(slot).map(view),
            1 => self.native.scan_entry(slot).map(view),
            2 => self.applications.scan_entry(slot).map(view),
            3 => self.witnesses.scan_entry(slot).map(view),
            4 => self.shared.scan_entry(slot).map(view),
            _ => None,
        }
    }

    pub(super) fn scan_finish(&mut self, kind: usize, slot: DescriptorSlot) {
        match kind {
            0 => self.interface_calls.scan_finish(slot),
            1 => self.native.scan_finish(slot),
            2 => self.applications.scan_finish(slot),
            3 => self.witnesses.scan_finish(slot),
            4 => self.shared.scan_finish(slot),
            _ => unreachable!("descriptor kind"),
        }
    }
}

impl ModuleStore {
    pub(crate) fn interface_call(
        &self,
        gc: &GcHeap,
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
        entry.promote(self, &records, gc)
    }

    pub(crate) fn native_application(
        &self,
        gc: &GcHeap,
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
        entry.promote(self, &records, gc)
    }

    pub(crate) fn method_application(
        &self,
        gc: &GcHeap,
        owner: &ExecutableModule,
        key: &ApplicationKey,
    ) -> Option<ApplicationId> {
        let records = self.inner.try_borrow().ok()?;
        let entry = records
            .resolve(owner)?
            .descriptors
            .applications
            .get(&(), key)?;
        entry.promote(self, &records, gc)
    }

    pub(crate) fn operation_bindings(
        &self,
        gc: &GcHeap,
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
        entry.promote(self, &records, gc)
    }

    pub(crate) fn shared_environment(
        &self,
        gc: &GcHeap,
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
        entry.promote(self, &records, gc)
    }
}

impl Runtime {
    pub(crate) fn publish_interface_call(
        &self,
        owner: &ExecutableModule,
        environment: EnvironmentId,
        site: InterfaceCallSite,
        prepared: Arc<ScopedInterfaceCall>,
    ) -> Result<(), RuntimeError> {
        self.publish_descriptor(owner, prepared, |descriptors, value| {
            descriptors.interface_calls.insert(environment, site, value)
        })
    }

    pub(crate) fn publish_native_application(
        &self,
        owner: &ExecutableModule,
        import: NativeImportId,
        prepared: Arc<NativeApplication>,
    ) -> Result<(), RuntimeError> {
        let environment = prepared.environment.id;
        self.publish_descriptor(owner, prepared, |descriptors, value| {
            descriptors.native.insert(environment, import, value)
        })
    }

    fn publish_descriptor<T: Descriptor>(
        &self,
        owner: &ExecutableModule,
        value: T,
        install: impl FnOnce(&mut LinkedDescriptors, Published<T>) -> Result<(), RuntimeError>,
    ) -> Result<(), RuntimeError> {
        self.gc.ensure_execution_allowed()?;
        self.validate_metadata(MetadataEdge::Program(owner))?;
        let dependencies = self.metadata_dependencies(value.edge())?;
        self.publish_metadata_edge(value.edge())?;
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
        owner: &ExecutableModule,
        key: ApplicationKey,
        prepared: ApplicationId,
    ) -> Result<(), RuntimeError> {
        self.publish_descriptor(owner, prepared, |descriptors, value| {
            descriptors.applications.insert((), key, value)
        })
    }

    pub(crate) fn publish_operation_bindings(
        &self,
        owner: &ExecutableModule,
        environment: Option<EnvironmentId>,
        witnesses: &[OperationWitness],
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
        owner: &ExecutableModule,
        scope: SharedScope,
        contract: &SharedCall,
        prepared: TypeEnvironment,
    ) -> Result<(), RuntimeError> {
        self.publish_descriptor(owner, prepared, |descriptors, value| {
            descriptors
                .shared
                .insert(scope, Arc::new(contract.clone()), value)
        })
    }
}
