//! Host selections retain actual interface/application graphs independently.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    execution_metadata::managed::{ProtectedMetadata, sources::MetadataSource},
    frame::types::construction::ConstructionScope,
    module::{
        ExecutableModule,
        dispatch::{CallRoute, SelectedTarget},
        managed::source::RevisionSource,
    },
    native::conversion::context::ConversionContext,
    objects::{
        application::MethodApplicationInput, invocation::MethodInvocation, method_view::MethodView,
    },
    value::{carriers::RootedValue, interfaces::InterfaceObject},
};
use kagari_bytecode::module::CallableTarget;
use kagari_gc::error::GcError;
use kagari_types::ty::{NominalTy, Ty};

#[derive(Debug, Clone)]
pub(crate) struct BoundReceiver {
    concrete_type: Ty,
    interface: NominalTy,
}

impl RootedInterfaceMethod {
    pub(super) fn from_interface(
        runtime: &mut Runtime,
        root: RootedValue,
        slot: usize,
    ) -> Result<Self, RuntimeError> {
        let protected = [MetadataSource::RootedInterface(&root)];
        let description = runtime.with_metadata_graph(&protected, |cx, graph, _| {
            let Some(ProtectedMetadata::Interface(object)) = graph.single_root() else {
                return Err(GcError::Representation.into());
            };
            Ok(cx.read(object, |object| object.description().clone()))
        })?;
        let invocation =
            MethodInvocation::from_interface(runtime, description.clone(), slot, &protected)?;
        Ok(Self {
            invocation,
            bound_receiver: BoundReceiver {
                concrete_type: description.concrete_type.clone(),
                interface: description.interface_type().clone(),
            },
            _root: root,
            application_root: None,
            computation_root: None,
        })
    }

    /// Retain the concrete receiver for an escaping Host use. Invocation itself
    /// reads the field directly from this handle's interface provider.
    pub fn receiver(&self, runtime: &mut Runtime) -> Result<RootedValue, RuntimeError> {
        self.invocation.facts(runtime, &self.sources()?)?;
        let result = runtime.memory.scope(&runtime.resources, |cx, _| {
            let value = self._root.get(cx)?;
            let object =
                cx.downcast::<InterfaceObject>(value.object().ok_or(GcError::Representation)?)?;
            InterfaceObject::data(cx, object)?.retain(cx)
        });
        result.map_err(|error| runtime.managed_memory_error(error))
    }

    pub fn concrete_type(&self) -> &Ty {
        &self.bound_receiver.concrete_type
    }

    pub fn interface_type(&self) -> &NominalTy {
        &self.bound_receiver.interface
    }

    pub(crate) fn body_origin(&self) -> Result<RevisionSource<'_>, RuntimeError> {
        self.computation_root
            .as_ref()
            .map(RevisionSource::HostApplication)
            .ok_or_else(|| {
                RuntimeError::module_validation("method must be admitted before execution")
            })
    }

    pub(crate) fn sources(&self) -> Result<Vec<MetadataSource<'_>>, RuntimeError> {
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(3)
            .map_err(|_| RuntimeError::resource_limit("Host method sources"))?;
        sources.push(MetadataSource::RootedInterface(&self._root));
        sources.extend(
            self.application_root
                .as_ref()
                .map(MetadataSource::HostApplication),
        );
        sources.extend(
            self.computation_root
                .as_ref()
                .map(MetadataSource::HostApplication),
        );
        Ok(sources)
    }

    /// Freeze one computation in its own actual lease. Clones of the reusable
    /// live handle do not acquire this selected body/environment graph.
    pub fn admit(&self, runtime: &mut Runtime) -> Result<Self, RuntimeError> {
        let facts = self.invocation.facts(runtime, &self.sources()?)?;
        if self.invocation.is_admitted() {
            return Ok(self.clone());
        }
        let owner = facts.proof_implementation().clone();
        let mut conversion = ConversionContext::new(runtime, &owner)?;
        let (invocation, root) =
            conversion.admit_interface_method(self.invocation.clone(), self.sources()?)?;
        Ok(Self {
            invocation,
            bound_receiver: self.bound_receiver.clone(),
            _root: self._root.clone(),
            application_root: self.application_root.clone(),
            computation_root: Some(root),
        })
    }

    pub(crate) fn view(&self, runtime: &mut Runtime) -> Result<MethodView, RuntimeError> {
        self.invocation.view(runtime, &self.sources()?)
    }

    /// Reusable handles select current ordinary code; admitted computations retain
    /// their selected target and therefore have no live route.
    pub(crate) fn live_call_route(
        &self,
        runtime: &mut Runtime,
    ) -> Result<Option<CallRoute>, RuntimeError> {
        let view = self.invocation.facts(runtime, &self.sources()?)?;
        if self.invocation.is_admitted() {
            return Ok(None);
        }
        view.proof_implementation()
            .bind_call_route(view.proof_target())
            .map(Some)
    }

    pub fn selected_target(&self, runtime: &mut Runtime) -> Result<SelectedTarget, RuntimeError> {
        Ok(self.view(runtime)?.selected)
    }

    pub fn implementation(&self, runtime: &mut Runtime) -> Result<ExecutableModule, RuntimeError> {
        Ok(self.view(runtime)?.implementation().clone())
    }

    pub fn target(&self, runtime: &mut Runtime) -> Result<CallableTarget, RuntimeError> {
        Ok(self.view(runtime)?.target())
    }

    pub fn parameter_types(&self, runtime: &mut Runtime) -> Result<Vec<Ty>, RuntimeError> {
        Ok(self.view(runtime)?.parameter_types().to_vec())
    }

    pub fn return_type(&self, runtime: &mut Runtime) -> Result<Ty, RuntimeError> {
        Ok(self.view(runtime)?.return_type().clone())
    }
}

impl Runtime {
    pub(super) fn apply_interface_method(
        &mut self,
        mut method: RootedInterfaceMethod,
        application: MethodApplicationInput<'_>,
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        let MethodApplicationInput {
            arguments,
            operations,
            mut constructions,
            sources: supplied,
        } = application;
        let mut sources = method.sources()?;
        sources
            .try_reserve(supplied.len())
            .map_err(|_| RuntimeError::resource_limit("Host method witness sources"))?;
        sources.extend_from_slice(supplied);
        let view = method.invocation.facts(self, &sources)?;
        constructions.extend(&ConstructionScope::new(view.proof_implementation()));
        let owner = view.proof_implementation().clone();
        let mut conversion = ConversionContext::new(self, &owner)?;
        let (invocation, root) = conversion.apply_interface_method(
            method.invocation.clone(),
            arguments,
            operations,
            constructions,
            sources,
        )?;
        drop(conversion);
        method.invocation = invocation;
        method.application_root = root;
        method.computation_root = None;
        Ok(method)
    }
}
