//! Host-retained selections own an external lease around the common invocation.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    execution_metadata::interfaces::InterfaceSnapshotId,
    gc::roots::RootedValue,
    module::{
        ExecutableModule,
        dispatch::{CallRoute, SelectedTarget},
    },
    objects::{invocation::MethodInvocation, method_view::MethodView},
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use kagari_types::ty::{NominalTy, Ty};

#[derive(Debug, Clone)]
pub(crate) struct BoundReceiver {
    receiver: Value,
    concrete_type: Ty,
    interface: NominalTy,
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
                concrete_type: view.description.concrete_type.clone(),
                interface: view.description.interface_type().clone(),
            },
            _root: root,
        })
    }

    pub fn receiver(&self) -> &Value {
        &self.bound_receiver.receiver
    }

    pub fn concrete_type(&self) -> &Ty {
        &self.bound_receiver.concrete_type
    }

    pub fn interface_type(&self) -> &NominalTy {
        &self.bound_receiver.interface
    }

    /// Freeze the next invocation without changing this live handle or its clones.
    /// Use an independent root: adding a computation edge to a shared handle root
    /// would unnecessarily pin admitted code after that invocation is released.
    pub(crate) fn admit(&self, runtime: &Runtime) -> Result<Self, RuntimeError> {
        self.validate_root(runtime)?;
        let invocation = self.invocation.admit(runtime)?;
        let root =
            runtime
                .root_value(self._root.value(&runtime.gc).ok_or_else(|| {
                    RuntimeError::module_validation("admitted method interface root")
                })?)
                .ok_or_else(|| RuntimeError::module_validation("admitted method receiver root"))?;
        let admitted = Self {
            invocation,
            bound_receiver: self.bound_receiver.clone(),
            _root: root,
        };
        admitted.refresh_roots(runtime)?;
        Ok(admitted)
    }

    fn validate_root(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        if !self._root.is_valid(&runtime.gc) {
            return Err(RuntimeError::module_validation(
                "method root belongs to another runtime or was released",
            ));
        }
        Ok(())
    }

    pub(crate) fn view<'a>(&self, runtime: &'a Runtime) -> Result<MethodView<'a>, RuntimeError> {
        self.validate_root(runtime)?;
        self.invocation.view(runtime)
    }

    /// Copy the bound relationship's route without retaining its receiver or code.
    /// An admitted computation has no live route; its selected target is exact.
    pub(crate) fn live_call_route(
        &self,
        runtime: &Runtime,
    ) -> Result<Option<CallRoute>, RuntimeError> {
        self.validate_root(runtime)?;
        if self.invocation.is_admitted() {
            return Ok(None);
        }
        let view = self.invocation.view(runtime)?;
        view.proof_implementation()
            .bind_call_route(view.proof_target())
            .map(Some)
    }

    /// Select one exact method code target for source inspection.
    /// Execution also requires the method's receiver/application evidence.
    /// The receiver relationship remains unchanged; named methods follow current
    /// publication while generated adapters retain their original body. An admitted
    /// invocation instead reports the exact target captured at admission.
    pub fn selected_target(&self, runtime: &Runtime) -> Result<SelectedTarget, RuntimeError> {
        Ok(self.view(runtime)?.selected)
    }

    pub fn implementation(&self, runtime: &Runtime) -> Result<ExecutableModule, RuntimeError> {
        Ok(self.view(runtime)?.implementation().clone())
    }

    pub fn target(&self, runtime: &Runtime) -> Result<CallableTarget, RuntimeError> {
        Ok(self.view(runtime)?.target())
    }

    pub fn parameter_types(&self, runtime: &Runtime) -> Result<Vec<Ty>, RuntimeError> {
        Ok(self.view(runtime)?.parameter_types().to_vec())
    }

    pub fn return_type(&self, runtime: &Runtime) -> Result<Ty, RuntimeError> {
        Ok(self.view(runtime)?.return_type().clone())
    }
}
