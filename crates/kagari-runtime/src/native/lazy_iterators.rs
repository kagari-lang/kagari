//! Runtime-owned lazy construction and steps on the shared execution scope.
mod constructor;
mod step;
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::{RootSet, lazy_iter::IteratorRequest},
    native::{NativeAction, NativeInvocation, NativeState},
    resource::ResourceState,
    value::Value,
};
use constructor::LazyConstructor;
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::{EngineNativeImport, NativeWitness, NativeWitnessImplementation},
    standard::{bindings::NativeDefaultMethod, traits::StandardTrait},
    types::PublicAbiItem,
};
use kagari_bytecode::{EngineImportId, Register};
use std::rc::Rc;
use step::LazyStep;

pub(super) struct StepCallScope(Rc<ResourceState>);
impl Drop for StepCallScope {
    fn drop(&mut self) {
        self.0.leave_call();
    }
}
impl NativeInvocation {
    pub(crate) fn iterator_step(
        runtime: &Runtime,
        request: IteratorRequest,
        destination: Option<Register>,
    ) -> Result<Self, RuntimeError> {
        runtime.validate_loaded_module(&request.implementation)?;
        let contract = request
            .implementation
            .bytecode
            .engine_imports
            .get(request.import.index())
            .ok_or_else(|| RuntimeError::module_validation("lazy step lost its import"))?;
        let EngineNativeBinding::TraitDefault(operation) = contract.binding else {
            return Err(RuntimeError::module_validation(
                "lazy step import is not a constructor",
            ));
        };
        if !operation.lazy() {
            return Err(RuntimeError::module_validation(
                "invalid lazy step operation",
            ));
        }
        runtime.resources.enter_call()?;
        let scope = StepCallScope(runtime.resources.clone());
        let captures = request.captures.len();
        let roots = runtime
            .gc()
            .root_execution_values(
                request
                    .captures
                    .into_iter()
                    .chain([request.iterator])
                    .chain(vec![Value::Unit; step::SCRATCH])
                    .collect(),
            )
            .ok_or_else(|| RuntimeError::module_validation("invalid lazy step roots"))?;
        Ok(Self {
            destination,
            implementation: request.implementation,
            import: request.import,
            roots,
            state: NativeState::Lazy(LazyInvocation {
                state: LazyState::Step(LazyStep::start(operation, captures)),
            }),
            entry: None,
            _step_scope: Some(scope),
        })
    }
}
pub(super) struct LazyInvocation {
    state: LazyState,
}
enum LazyState {
    Constructor(LazyConstructor),
    Step(LazyStep),
}
impl LazyInvocation {
    pub(super) fn constructor(operation: NativeDefaultMethod, arguments: &[Value]) -> Self {
        Self {
            state: LazyState::Constructor(LazyConstructor::start(operation, arguments)),
        }
    }
    pub(super) fn constructor_roots(&self) -> Result<Vec<Value>, RuntimeError> {
        match self.state {
            LazyState::Constructor(_) => Ok(vec![Value::Unit; constructor::SCRATCH]),
            LazyState::Step(_) => Err(RuntimeError::module_validation(
                "step used constructor entry",
            )),
        }
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        import: EngineImportId,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match &mut self.state {
            LazyState::Constructor(state) => state.advance(runtime, owner, import, contract, roots),
            LazyState::Step(state) => state.advance(runtime, owner, contract, roots),
        }
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        match &mut self.state {
            LazyState::Constructor(state) => state.receive(runtime, roots, value),
            LazyState::Step(state) => state.receive(runtime, roots, value),
        }
    }
}

fn source_list<'a>(
    owner: &LoadedModule,
    contract: &'a EngineNativeImport,
) -> Result<&'a NativeWitness, RuntimeError> {
    contract
        .witnesses
        .iter()
        .find(|witness| {
            contract.signature.params.first() == Some(&witness.receiver)
                && StandardTrait::from_id(&witness.interface.declaration)
                    == Some(StandardTrait::List)
                && match &witness.implementation {
                    NativeWitnessImplementation::Table(target) => owner.members().any(|module| {
                        module.bytecode.public_items.iter().any(|item| {
                            matches!(item, PublicAbiItem::InterfaceTable(table)
                                if table.declaration == target.declaration && !table.native_bridge)
                        })
                    }),
                    NativeWitnessImplementation::Interface => true,
                    _ => false,
                }
        })
        .ok_or_else(|| RuntimeError::module_validation("lazy source List application mismatch"))
}
