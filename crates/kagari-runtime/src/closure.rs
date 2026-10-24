//! Checked closure call metadata, independent of collector implementation.
mod native;
#[cfg(test)]
mod tests;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{
        TypeEnvironment, arguments::ScopedSignature, bindings::TypeBindings,
        compatibility::TypeView,
    },
    gc::roots::RootedValue,
    module::{
        ExecutableModule,
        dispatch::{CallRoute, CallRouteKind, PublicationToken},
    },
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget, program::ModuleRef};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{ids::FunctionRef, representation::semantic_representation};
use kagari_types::ty::Ty;
use std::{slice, sync::Arc};

/// Copied exact target identity for source/debug association. This contains no
/// executable lease, captured values, GC roots or permission to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallableObservation {
    pub publication: PublicationToken,
    pub module: ModuleRef,
    pub target: ObservedCallableTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedCallableTarget {
    Entry(CallableTarget),
    /// Known native declaration in this Runtime's definition scope. A producer
    /// need not have an independently observable script/native import slot.
    NativeBinding(DefinitionId),
}

impl CallableObservation {
    pub(crate) fn entry(owner: &ExecutableModule, target: CallableTarget) -> Self {
        Self {
            publication: owner.publication_token(),
            module: owner.slot(),
            target: ObservedCallableTarget::Entry(target),
        }
    }
}

impl Runtime {
    /// Observe a rooted callable or cold Future without polling or claiming it.
    /// Consumed/composed Futures have no single body and return `None`. Named
    /// values select the current route; retained computations report their body.
    /// The returned token does not retain the supplied root or executable code.
    pub fn observe_callable_value(
        &self,
        root: &RootedValue,
    ) -> Result<Option<CallableObservation>, RuntimeError> {
        self.gc().ensure_no_native_borrow()?;
        self.resources().ensure_execution_allowed()?;
        let value = root
            .value(self.gc())
            .ok_or_else(|| RuntimeError::module_validation("callable observation root"))?;
        match value {
            Value::Closure(_) => self.resolve_closure(&value)?.observe(self).map(Some),
            Value::GcHandle(id) => {
                let (owner, _, _) = self.gc().future_contract(id)?;
                self.validate_executable_module(&owner)?;
                if let Some(closure) = self.gc().future_script(id)? {
                    return self.resolve_closure(&closure)?.observe(self).map(Some);
                }
                self.gc().future_native_origin(id)
            }
            _ => Err(RuntimeError::module_validation(
                "observation requires a closure or Future",
            )),
        }
    }
}

/// A checked closure may enter ordinary script code or an installed native entry.
#[derive(Debug, Clone)]
pub enum ClosureTarget {
    /// An exact generated body, including a cold Future's private resume entry.
    Script(FunctionRef),
    /// An exact native entry selected for an admitted computation.
    Native(NativeClosure),
    /// A named function value selects its verified route on each invocation.
    Named(NamedClosure),
}

/// Stable named-call policy plus the original checked conversion contract.
/// The enclosing snapshot retains lexical type/environment metadata, not an old
/// ordinary-call selection. Capture count never determines this classification.
#[derive(Debug, Clone)]
pub struct NamedClosure {
    pub(crate) route: CallRoute,
    pub(crate) target: CallableTarget,
    pub(crate) signature: Arc<ScopedSignature>,
}

/// Only runtime preparation constructs native closure metadata. Its signature
/// keeps lexical type scopes; executable retention uses the enclosing graph edges.
#[derive(Debug, Clone)]
pub struct NativeClosure {
    pub(crate) import: NativeImportId,
    pub(crate) signature: Arc<ScopedSignature>,
}

/// Closure metadata stored by value in the runtime heap. An explicit diagnostic
/// copy does not root captures or authorize execution after its source is collected.
#[derive(Debug, Clone)]
pub struct ClosureValueSnapshot {
    pub environment: Option<TypeEnvironment>,
    pub implementation: ExecutableModule,
    pub target: ClosureTarget,
    pub captures: Vec<Value>,
}

impl ClosureValueSnapshot {
    fn observe(&self, runtime: &Runtime) -> Result<CallableObservation, RuntimeError> {
        if let Some(route) = self.call_route() {
            let selected = runtime.select_call_route(route)?;
            return Ok(CallableObservation::entry(
                selected.owner(),
                selected.target(),
            ));
        }
        runtime.validate_executable_module(&self.implementation)?;
        Ok(CallableObservation::entry(
            &self.implementation,
            self.callable_target(),
        ))
    }

    pub fn script_function(&self) -> Option<FunctionRef> {
        match self.callable_target() {
            CallableTarget::Script(function) => Some(function),
            CallableTarget::Native(_) => None,
        }
    }

    /// Original exact identity for observation; this does not select current code.
    pub fn callable_target(&self) -> CallableTarget {
        match &self.target {
            ClosureTarget::Script(function) => CallableTarget::Script(*function),
            ClosureTarget::Native(native) => CallableTarget::Native(native.import),
            ClosureTarget::Named(named) => named.target,
        }
    }

    /// Checked ordinary route, absent for already-selected computations.
    pub fn call_route(&self) -> Option<&CallRoute> {
        match &self.target {
            ClosureTarget::Named(named) => Some(&named.route),
            _ => None,
        }
    }

    pub(crate) fn bind_named(&mut self, runtime: &Runtime) -> Result<(), RuntimeError> {
        let target = self.callable_target();
        let route = self.implementation.bind_call_route(target)?;
        if route.kind() != CallRouteKind::BodyOwned {
            self.target = ClosureTarget::Named(NamedClosure {
                route,
                target,
                signature: self.signature(runtime)?,
            });
        }
        Ok(())
    }

    /// Select once for an invocation or queue admission. The returned snapshot is
    /// exact, but is not a root; its caller must publish its metadata/captures before GC.
    pub(crate) fn select_call(&self, runtime: &Runtime) -> Result<Self, RuntimeError> {
        let ClosureTarget::Named(named) = &self.target else {
            runtime.validate_executable_module(&self.implementation)?;
            return Ok(self.clone());
        };
        let selected = runtime.select_call_route(&named.route)?;
        let environment = self
            .environment
            .as_ref()
            .map(|environment| {
                runtime.transport_environment(&self.implementation, &selected, environment)
            })
            .transpose()?;
        let target = match selected.target() {
            CallableTarget::Script(function) => ClosureTarget::Script(function),
            CallableTarget::Native(import) => {
                let types = environment
                    .as_ref()
                    .map(|environment| environment.types.clone());
                let params = runtime.type_arguments(
                    selected.owner(),
                    types.clone(),
                    &selected.signature().params,
                )?;
                let result = runtime
                    .type_arguments(
                        selected.owner(),
                        types,
                        slice::from_ref(&selected.signature().result),
                    )?
                    .pop()
                    .ok_or_else(|| {
                        RuntimeError::module_validation("selected native closure result scope")
                    })?;
                ClosureTarget::Native(NativeClosure {
                    import,
                    signature: Arc::new(ScopedSignature { params, result }),
                })
            }
        };
        Ok(Self {
            implementation: selected.owner().clone(),
            target,
            environment,
            captures: self.captures.clone(),
        })
    }

    fn scoped_signature(&self) -> Option<&Arc<ScopedSignature>> {
        match &self.target {
            ClosureTarget::Named(named) => Some(&named.signature),
            ClosureTarget::Native(native) => Some(&native.signature),
            ClosureTarget::Script(_) => None,
        }
    }

    pub(crate) fn signature(
        &self,
        runtime: &Runtime,
    ) -> Result<Arc<ScopedSignature>, RuntimeError> {
        if let Some(signature) = self.scoped_signature() {
            return Ok(signature.clone());
        }
        let function = self.script_function().expect("script closure target");
        let metadata = &self
            .implementation
            .bytecode
            .functions
            .get(function.index())
            .ok_or_else(|| RuntimeError::module_validation("closure function"))?
            .metadata;
        let types = (self.captures.len()..metadata.params.len())
            .map(|index| {
                metadata
                    .semantic
                    .params
                    .get(&index)
                    .cloned()
                    .ok_or_else(|| RuntimeError::module_validation("closure parameter type"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let environment = self
            .environment
            .as_ref()
            .map(|environment| environment.types.clone());
        let params = runtime.type_arguments(&self.implementation, environment.clone(), &types)?;
        let result = metadata
            .semantic
            .result
            .as_ref()
            .ok_or_else(|| RuntimeError::module_validation("closure result type"))?;
        let result = runtime
            .type_arguments(&self.implementation, environment, slice::from_ref(result))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("closure result scope"))?;
        Ok(Arc::new(ScopedSignature { params, result }))
    }

    pub(crate) fn matches_physical_signature(
        &self,
        expected: impl ExactSizeIterator<Item = ValueType>,
        result: ValueType,
    ) -> Result<bool, RuntimeError> {
        if let Some(signature) = self.scoped_signature() {
            return Ok(signature.params.len() == expected.len()
                && signature
                    .params
                    .iter()
                    .zip(expected)
                    .all(|(ty, expected)| semantic_representation(ty.ty()) == expected)
                && semantic_representation(signature.result.ty()) == result);
        }
        let function = self.script_function().expect("script closure target");
        let function = self
            .implementation
            .bytecode
            .functions
            .get(function.index())
            .ok_or_else(|| RuntimeError::module_validation("closure function"))?;
        let parameters = function
            .metadata
            .params
            .get(self.captures.len()..)
            .ok_or_else(|| RuntimeError::module_validation("closure captures"))?;
        if parameters.len() != expected.len() {
            return Ok(false);
        }
        let resolve = |physical, semantic: Option<&Ty>| {
            if physical != ValueType::Generic {
                return Ok(physical);
            }
            let ty =
                semantic.ok_or_else(|| RuntimeError::module_validation("generic closure type"))?;
            let environment = self
                .environment
                .as_ref()
                .ok_or_else(|| RuntimeError::module_validation("generic closure environment"))?;
            Ok(semantic_representation(&environment.types.resolve(ty)?))
        };
        for (index, (actual, expected)) in parameters.iter().zip(expected).enumerate() {
            if resolve(
                *actual,
                function
                    .metadata
                    .semantic
                    .params
                    .get(&(self.captures.len() + index)),
            )? != expected
            {
                return Ok(false);
            }
        }
        Ok(resolve(
            function.metadata.return_type,
            function.metadata.semantic.result.as_ref(),
        )? == result)
    }

    pub(crate) fn matches_function(
        &self,
        params: &[Ty],
        result: &Ty,
        owner: &ExecutableModule,
        environment: Option<&TypeBindings>,
    ) -> bool {
        if let Some(signature) = self.scoped_signature() {
            return signature.params.len() == params.len()
                && signature
                    .params
                    .iter()
                    .zip(params)
                    .all(|(actual, expected)| {
                        actual.view(&self.implementation).compatible(TypeView::new(
                            expected,
                            owner,
                            environment,
                        ))
                    })
                && signature
                    .result
                    .view(&self.implementation)
                    .compatible(TypeView::new(result, owner, environment));
        }
        let function = self.script_function().expect("script closure target");
        let Some(function) = self.implementation.bytecode.functions.get(function.index()) else {
            return false;
        };
        let compatible = |actual: &Ty, expected: &Ty| {
            TypeView::new(
                actual,
                &self.implementation,
                self.environment
                    .as_ref()
                    .map(|environment| environment.types.as_ref()),
            )
            .compatible(TypeView::new(expected, owner, environment))
        };
        let captures = self.captures.len();
        function
            .metadata
            .params
            .get(captures..)
            .is_some_and(|suffix| suffix.len() == params.len())
            && params.iter().enumerate().all(|(index, expected)| {
                function
                    .metadata
                    .semantic
                    .params
                    .get(&(captures + index))
                    .is_some_and(|actual| compatible(actual, expected))
            })
            && function
                .metadata
                .semantic
                .result
                .as_ref()
                .is_some_and(|actual| compatible(actual, result))
    }
}
