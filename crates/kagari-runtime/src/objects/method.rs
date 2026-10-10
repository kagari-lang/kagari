//! Host-retained selections own an external lease around the common invocation.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    execution_metadata::interfaces::InterfaceSnapshotId,
    gc::roots::RootedValue,
    module::LoadedModule,
    objects::{invocation::MethodInvocation, method_view::MethodView},
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::{NominalTy, Ty};

#[derive(Clone)]
pub(crate) struct BoundReceiver {
    receiver: Value,
    concrete_type: Ty<DefinitionId>,
    interface: NominalTy<DefinitionId>,
}

impl RootedInterfaceMethod {
    pub(super) fn refresh_roots(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        let mut metadata = Vec::new();
        self.invocation.append_metadata(&mut metadata);
        self._root.set_metadata(runtime, metadata)
    }

    pub(super) fn from_interface(
        runtime: &Runtime,
        root: RootedValue,
        snapshot: InterfaceSnapshotId,
        slot: usize,
    ) -> Result<Self, RuntimeError> {
        let invocation = MethodInvocation::from_interface(runtime, snapshot, slot)?;
        let view = runtime
            .gc
            .interface_metadata(snapshot)
            .ok_or_else(|| RuntimeError::module_validation("invalid interface snapshot"))?;
        Ok(Self {
            invocation,
            bound_receiver: BoundReceiver {
                receiver: view.data,
                concrete_type: view.concrete_type.clone(),
                interface: view.interface_type.clone(),
            },
            _root: root,
        })
    }

    pub fn receiver(&self) -> &Value {
        &self.bound_receiver.receiver
    }

    pub fn concrete_type(&self) -> &Ty<DefinitionId> {
        &self.bound_receiver.concrete_type
    }

    pub fn interface_type(&self) -> &NominalTy<DefinitionId> {
        &self.bound_receiver.interface
    }

    pub(crate) fn view<'a>(&self, runtime: &'a Runtime) -> Result<MethodView<'a>, RuntimeError> {
        if !self._root.is_valid(&runtime.gc) {
            return Err(RuntimeError::module_validation(
                "method root belongs to another runtime or was released",
            ));
        }
        self.invocation.view(runtime)
    }

    pub fn implementation(&self, runtime: &Runtime) -> Result<LoadedModule, RuntimeError> {
        Ok(self.view(runtime)?.implementation().clone())
    }

    pub fn target(&self, runtime: &Runtime) -> Result<CallableTarget, RuntimeError> {
        Ok(self.view(runtime)?.target())
    }

    pub fn parameter_types(
        &self,
        runtime: &Runtime,
    ) -> Result<Vec<Ty<DefinitionId>>, RuntimeError> {
        Ok(self.view(runtime)?.parameter_types().to_vec())
    }

    pub fn return_type(&self, runtime: &Runtime) -> Result<Ty<DefinitionId>, RuntimeError> {
        Ok(self.view(runtime)?.return_type().clone())
    }
}
