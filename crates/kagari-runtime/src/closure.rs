//! Checked closure call metadata, independent of collector implementation.
mod access;
pub mod calls;
#[cfg(test)]
mod tests;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{
        TypeEnvironment,
        arguments::{ScopedSignature, TypeArgument},
        compatibility::TypeView,
    },
    module::{
        ExecutableModule,
        dispatch::{CallRoute, CallRouteKind, PublicationToken},
    },
    native::future::claims,
    value::{carriers::RootedValue, closures::ClosureObject, kind::ObjectKind},
    value_check,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget, program::ModuleRef};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{ids::FunctionRef, representation::semantic_representation};
use kagari_gc::error::GcError;
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
        &mut self,
        root: &RootedValue,
    ) -> Result<Option<CallableObservation>, RuntimeError> {
        self.resources().ensure_synchronous_thread()?;
        self.resources().ensure_execution_allowed()?;
        value_check::candidate_root(self, root)?;
        let closure = self
            .memory
            .scope(&self.resources, |cx, _| {
                let value = root.get(cx)?;
                if value.kind() != Some(ObjectKind::Closure) {
                    return Ok(None);
                }
                let object =
                    cx.downcast::<ClosureObject>(value.object().ok_or(GcError::Representation)?)?;
                Ok(Some(cx.read(object, |object| object.record().clone())))
            })
            .map_err(|error| match error {
                GcError::ForeignHeap | GcError::InvalidRoot | GcError::Representation => {
                    RuntimeError::module_validation("callable observation root")
                }
                error => self.managed_memory_error(error),
            })?;
        if let Some(closure) = closure {
            return closure.observe(self).map(Some);
        }
        claims::observe(self, root)
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
}

/// Only runtime preparation constructs native closure metadata. Its signature
/// keeps lexical type scopes; executable retention uses the enclosing graph edges.
#[derive(Debug, Clone)]
pub struct NativeClosure {
    pub(crate) import: NativeImportId,
}

/// Immutable closure meaning. Captures and executable retention belong to the
/// actual ClosureObject; cloning this record never retains managed state.
#[derive(Debug)]
pub struct ClosureRecord {
    pub environment: Option<TypeEnvironment>,
    pub implementation: ExecutableModule,
    pub target: ClosureTarget,
    pub(crate) capture_types: Box<[Option<TypeArgument>]>,
    signature: Arc<ScopedSignature>,
}

#[derive(Clone, Copy)]
pub(crate) enum ClosureMode {
    Ordinary,
    Retained,
}

impl ClosureRecord {
    pub(crate) fn script(
        runtime: &Runtime,
        owner: &ExecutableModule,
        function: FunctionRef,
        capture_count: usize,
        environment: Option<TypeEnvironment>,
        mode: ClosureMode,
    ) -> Result<Self, RuntimeError> {
        runtime.validate_executable_module(owner)?;
        if owner.is_static_initializer(function) {
            return Err(RuntimeError::module_validation(
                "static initializer is not a callable value",
            ));
        }
        let function_metadata = &owner
            .bytecode
            .functions
            .get(function.index())
            .ok_or_else(|| RuntimeError::module_validation("invalid closure function"))?
            .metadata;
        if capture_count > function_metadata.params.len() {
            return Err(RuntimeError::module_validation(
                "closure capture contract mismatch",
            ));
        }
        if function_metadata
            .semantic
            .generic
            .as_ref()
            .is_some_and(|body| {
                environment
                    .as_ref()
                    .is_none_or(|environment| !environment.types.matches(body))
            })
        {
            return Err(RuntimeError::module_validation(
                "closure generic environment",
            ));
        }
        let types = environment
            .as_ref()
            .map(|environment| environment.types.clone());
        let mut capture_types = Vec::new();
        capture_types
            .try_reserve_exact(capture_count)
            .map_err(|_| RuntimeError::resource_limit("closure capture types"))?;
        for index in 0..capture_count {
            if index.is_multiple_of(1024) {
                runtime.resources().poll_execution()?;
            }
            let argument = function_metadata
                .semantic
                .params
                .get(&index)
                .map(|ty| {
                    runtime
                        .type_arguments(owner, types.clone(), slice::from_ref(ty))?
                        .pop()
                        .ok_or_else(|| RuntimeError::module_validation("closure capture type"))
                })
                .transpose()?;
            capture_types.push(argument);
        }
        let mut params = Vec::new();
        params
            .try_reserve_exact(function_metadata.params.len() - capture_count)
            .map_err(|_| RuntimeError::resource_limit("closure parameter types"))?;
        for index in capture_count..function_metadata.params.len() {
            if index.is_multiple_of(1024) {
                runtime.resources().poll_execution()?;
            }
            params.push(
                function_metadata
                    .semantic
                    .params
                    .get(&index)
                    .cloned()
                    .ok_or_else(|| RuntimeError::module_validation("closure parameter type"))?,
            );
        }
        let params = runtime.type_arguments(owner, types.clone(), &params)?;
        let result = function_metadata
            .semantic
            .result
            .as_ref()
            .ok_or_else(|| RuntimeError::module_validation("closure result type"))?;
        let result = runtime
            .type_arguments(owner, types, slice::from_ref(result))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("closure result scope"))?;
        let entry = CallableTarget::Script(function);
        let target = if matches!(mode, ClosureMode::Ordinary) {
            let route = owner.bind_call_route(entry)?;
            if route.kind() == CallRouteKind::BodyOwned {
                ClosureTarget::Script(function)
            } else {
                ClosureTarget::Named(NamedClosure {
                    route,
                    target: entry,
                })
            }
        } else {
            ClosureTarget::Script(function)
        };
        Ok(Self {
            implementation: owner.clone(),
            target,
            environment,
            capture_types: capture_types.into_boxed_slice(),
            signature: Arc::new(ScopedSignature { params, result }),
        })
    }

    pub(crate) fn native(
        runtime: &Runtime,
        owner: ExecutableModule,
        import: NativeImportId,
        environment: Option<TypeEnvironment>,
        signature: Arc<ScopedSignature>,
        mode: ClosureMode,
    ) -> Result<Self, RuntimeError> {
        runtime.validate_executable_module(&owner)?;
        let imported = owner
            .bytecode
            .native_imports
            .get(import.index())
            .ok_or_else(|| RuntimeError::module_validation("native closure import"))?;
        if signature.params.len() != imported.signature.params.len()
            || imported.generic.as_ref().is_some_and(|body| {
                environment
                    .as_ref()
                    .is_none_or(|environment| !environment.types.matches(body))
            })
        {
            return Err(RuntimeError::module_validation(
                "native closure signature scope",
            ));
        }
        let types = environment
            .as_ref()
            .map(|environment| environment.types.as_ref());
        for (index, (actual, expected)) in signature
            .params
            .iter()
            .zip(&imported.signature.params)
            .enumerate()
        {
            if index.is_multiple_of(1024) {
                runtime.resources().poll_execution()?;
            }
            actual.validate(runtime)?;
            if !actual
                .view(&owner)
                .compatible(TypeView::new(expected, &owner, types))
            {
                return Err(RuntimeError::module_validation(
                    "native closure parameter scope",
                ));
            }
        }
        signature.result.validate(runtime)?;
        if !signature.result.view(&owner).compatible(TypeView::new(
            &imported.signature.result,
            &owner,
            types,
        )) {
            return Err(RuntimeError::module_validation(
                "native closure result scope",
            ));
        }
        let entry = CallableTarget::Native(import);
        let target = if matches!(mode, ClosureMode::Ordinary) {
            let route = owner.bind_call_route(entry)?;
            if route.kind() == CallRouteKind::BodyOwned {
                ClosureTarget::Native(NativeClosure { import })
            } else {
                ClosureTarget::Named(NamedClosure {
                    route,
                    target: entry,
                })
            }
        } else {
            ClosureTarget::Native(NativeClosure { import })
        };
        Ok(Self {
            environment,
            implementation: owner,
            target,
            capture_types: Box::default(),
            signature,
        })
    }

    pub(crate) fn observe(&self, runtime: &Runtime) -> Result<CallableObservation, RuntimeError> {
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

    /// Observe the original exact entry without selecting current code.
    pub fn callable_target(&self) -> CallableTarget {
        match &self.target {
            ClosureTarget::Script(function) => CallableTarget::Script(*function),
            ClosureTarget::Native(native) => CallableTarget::Native(native.import),
            ClosureTarget::Named(named) => named.target,
        }
    }

    pub fn call_route(&self) -> Option<&CallRoute> {
        match &self.target {
            ClosureTarget::Named(named) => Some(&named.route),
            _ => None,
        }
    }

    pub(crate) fn signature(&self) -> &Arc<ScopedSignature> {
        &self.signature
    }

    pub(crate) fn matches_physical_signature(
        &self,
        expected: impl ExactSizeIterator<Item = ValueType>,
        result: ValueType,
    ) -> bool {
        self.signature.params.len() == expected.len()
            && self
                .signature
                .params
                .iter()
                .zip(expected)
                .all(|(ty, expected)| semantic_representation(ty.ty()) == expected)
            && semantic_representation(self.signature.result.ty()) == result
    }

    pub(crate) fn matches_view(&self, expected: TypeView<'_>) -> bool {
        let Some(expected) = expected.normalized() else {
            return false;
        };
        let Ty::Function { params, .. } = expected.ty else {
            return false;
        };
        self.signature.params.len() == params.len()
            && self
                .signature
                .params
                .iter()
                .enumerate()
                .all(|(index, actual)| {
                    expected.function_parameter(index).is_some_and(|expected| {
                        actual.view(&self.implementation).compatible(expected)
                    })
                })
            && expected.function_result().is_some_and(|expected| {
                self.signature
                    .result
                    .view(&self.implementation)
                    .compatible(expected)
            })
    }
}
