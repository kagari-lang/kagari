//! Prepared function arguments and explicitly traced or rooted retained callbacks.
use crate::native::storage::scan::{NativeScanKind, ScanBudget, ScanStatus};
use crate::{
    Runtime,
    closure::ClosureValueSnapshot,
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    gc::{GcObjectKind, roots::RootedValue},
    native::{
        arguments::CallArguments,
        binding::NativeResult,
        context::{CallContext, ScriptCall, arguments::ArgumentView},
        scalar::NativeScalar,
        storage::NativePayload,
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_types::ty::Ty;
use std::cell::Ref;

#[derive(Debug, Clone)]
pub struct PreparedClosure {
    value: Value,
}

impl PreparedClosure {
    pub(crate) fn from_value(runtime: &Runtime, value: Value) -> NativeResult<Self> {
        let closure = Self { value };
        closure.validate(runtime)?;
        Ok(closure)
    }

    pub(crate) fn validate(&self, runtime: &Runtime) -> NativeResult<()> {
        runtime.gc().ensure_no_storage_borrow()?;
        let Value::Closure(id) = self.value else {
            return Err(RuntimeError::module_validation("native callback handle"));
        };
        if runtime.gc().identity_kind(id) != Some(GcObjectKind::Closure) {
            return Err(RuntimeError::module_validation(
                "native callback generation",
            ));
        }
        if !runtime.gc().validate_candidate_value(&self.value) {
            return Err(RuntimeError::execution_phase_violation(
                "external closure in candidate execution",
            ));
        }
        Ok(())
    }

    pub(crate) fn snapshot<'runtime>(
        &self,
        runtime: &'runtime Runtime,
    ) -> NativeResult<Ref<'runtime, ClosureValueSnapshot>> {
        self.validate(runtime)?;
        runtime.resolve_closure(&self.value)
    }

    /// Freeze a named route for one admitted call while retaining all captures.
    /// The returned root owns the exact selected computation, not the original
    /// ordinary route. Retained closures reuse their existing heap identity.
    pub(crate) fn prepare_call(&self, runtime: &Runtime) -> NativeResult<(Self, RootedValue)> {
        let (selected, named) = {
            let snapshot = self.snapshot(runtime)?;
            (
                snapshot.select_call(runtime)?,
                snapshot.call_route().is_some(),
            )
        };
        let value = if named {
            runtime.validate_metadata(MetadataEdge::Closure(&selected))?;
            Value::Closure(runtime.gc().alloc_closure(runtime.modules(), selected)?)
        } else {
            self.value
        };
        let root = runtime
            .root_value(value)
            .ok_or_else(|| RuntimeError::module_validation("selected closure root"))?;
        Ok((Self { value }, root))
    }

    /// Expose a backend descriptor's transport without creating a root.
    ///
    /// # Safety
    /// The caller must retain the closure independently through every use and
    /// destination publication. The descriptor may outlive its original root;
    /// an expired descriptor is not a live input to backend execution.
    #[doc(hidden)]
    pub unsafe fn value_transport(&self) -> &Value {
        self.value()
    }

    pub(crate) fn value(&self) -> &Value {
        &self.value
    }

    fn invoke(&self, cx: &CallContext<'_>, arguments: &[Value]) -> NativeResult<Value> {
        let (prepared, _root) = self.prepare_call(cx.runtime)?;
        let owner = prepared.snapshot(cx.runtime)?.implementation.clone();
        cx.invoke_script.invoke(
            cx.runtime,
            &owner,
            ScriptCall::Closure(&prepared),
            arguments,
        )
    }

    fn call<R: NativeScalar, A: CallArguments>(
        &self,
        cx: &CallContext<'_>,
        params: &[Ty],
        result: &Ty,
        arguments: A,
    ) -> NativeResult<R> {
        if result != &Ty::Builtin(R::abi_type()) || !A::matches(params) {
            return Err(RuntimeError::module_validation(
                "native callback conversion differs from its declaration",
            ));
        }
        cx.heap().ensure_no_storage_borrow()?;
        arguments.with_values(|arguments| self.invoke(cx, arguments).and_then(R::decode))
    }

    fn call_values(&self, cx: &CallContext<'_>, arguments: &[Value]) -> NativeResult<Value> {
        cx.heap().ensure_no_storage_borrow()?;
        let (owner, signature) = {
            let closure = self.snapshot(cx.runtime)?;
            (
                closure.implementation.clone(),
                closure.signature(cx.runtime)?,
            )
        };
        if arguments.len() != signature.params.len()
            || signature
                .params
                .iter()
                .zip(arguments)
                .any(|(ty, value)| !ty.matches(cx.runtime, value, &owner))
        {
            return Err(RuntimeError::module_validation("native callback arguments"));
        }
        let value = self.invoke(cx, arguments)?;
        if !signature.result.matches(cx.runtime, &value, &owner) {
            return Err(RuntimeError::module_validation("native callback result"));
        }
        Ok(value)
    }
}

/// A function argument borrows its existing frame root. Resolving the closure
/// checks immutable capture metadata and never holds a heap borrow across calls.
pub struct CallableHandle<'call> {
    target: PreparedClosure,
    params: &'call [Ty],
    result: &'call Ty,
    _arguments: ArgumentView<'call>,
}

impl<'call> CallableHandle<'call> {
    pub(crate) fn from_argument(cx: &CallContext<'call>, index: usize) -> NativeResult<Self> {
        cx.heap().ensure_no_storage_borrow()?;
        let Ty::Function { params, result } = cx.argument_type(index)? else {
            return Err(RuntimeError::module_validation(
                "native callback declaration",
            ));
        };
        let value = cx.argument(index)?;
        let closure = cx.runtime.resolve_closure(&value)?;
        if let Some(route) = closure.call_route() {
            cx.runtime.select_call_route(route)?;
        } else {
            cx.runtime
                .validate_executable_module(&closure.implementation)?;
        }
        let expected = cx.argument_type_view(index)?;
        let Ty::Function {
            params: expected_params,
            result: expected_result,
        } = expected.ty
        else {
            return Err(RuntimeError::module_validation(
                "native callback type scope",
            ));
        };
        if !closure.matches_function(
            expected_params,
            expected_result,
            expected.owner,
            expected.environment,
        ) {
            return Err(RuntimeError::module_validation("native callback signature"));
        }
        Ok(Self {
            target: PreparedClosure { value },
            params,
            result,
            _arguments: cx.arguments(),
        })
    }

    pub fn call<R: NativeScalar, A: CallArguments>(
        &self,
        cx: &mut CallContext<'_>,
        arguments: A,
    ) -> NativeResult<R> {
        self.target.call(cx, self.params, self.result, arguments)
    }

    /// Invoke a closure with protected provider transport.
    ///
    /// # Safety
    /// The selected closure and every input must remain protected throughout the
    /// call. A stored descriptor requires its independently rooted tracing owner;
    /// the descriptor itself is not a root. Protect mutable-slot copies before
    /// reentry and keep host capabilities inside their original scope. Retain or
    /// publish the returned value before any callback or safepoint.
    #[doc(hidden)]
    pub unsafe fn call_values_transport(
        &self,
        cx: &mut CallContext<'_>,
        arguments: &[Value],
    ) -> NativeResult<Value> {
        self.call_values(cx, arguments)
    }

    /// Values read from mutable storage need working roots before callbacks.
    /// A returned GC value needs an explicit root before further reentry or GC.
    pub(crate) fn call_values(
        &self,
        cx: &mut CallContext<'_>,
        arguments: &[Value],
    ) -> NativeResult<Value> {
        self.target.call_values(cx, arguments)
    }

    /// Extract an unrooted closure edge for an explicitly traced provider payload.
    ///
    /// # Safety
    /// Keep the closure protected through every scan/call and transfer into a
    /// traced owner. Extracted copies do not extend that owner's protection.
    /// This descriptor is not owning host storage; use Function for that purpose.
    #[doc(hidden)]
    pub unsafe fn store_transport(&self) -> StoredCallable {
        StoredCallable(self.target.clone())
    }
}

/// An advanced traced heap edge. It does not retain its closure independently;
/// the owning payload must trace it and remain rooted across invocation. Ordinary
/// host storage uses Function, and managed payload fields retain heap edges.
#[derive(Debug, Clone)]
pub struct StoredCallable(PreparedClosure);

unsafe impl NativePayload for StoredCallable {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::Bounded { max_work: 8 };

    fn begin_scan(&self) {}

    fn scan<'payload>(
        &'payload self,
        _: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'payload Value),
    ) -> ScanStatus {
        budget.charge(1);
        visit(&self.0.value);

        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(1, 0)
    }
}

impl StoredCallable {
    /// Invoke a closure with protected provider transport.
    ///
    /// # Safety
    /// The selected closure and every input must remain protected throughout the
    /// call. A stored descriptor requires its independently rooted tracing owner;
    /// the descriptor itself is not a root. Protect mutable-slot copies before
    /// reentry and keep host capabilities inside their original scope. Retain or
    /// publish the returned value before any callback or safepoint.
    #[doc(hidden)]
    pub unsafe fn call_values_transport(
        &self,
        cx: &mut CallContext<'_>,
        arguments: &[Value],
    ) -> NativeResult<Value> {
        self.call_values(cx, arguments)
    }

    /// Invoke a descriptor retained by a currently rooted native payload. The
    /// closure is validated before entering the prepared frame; callers retaining
    /// it independently should use Function. Returned values need a root before GC.
    pub(crate) fn call_values(
        &self,
        cx: &mut CallContext<'_>,
        arguments: &[Value],
    ) -> NativeResult<Value> {
        self.0.validate(cx.runtime)?;
        self.0.call_values(cx, arguments)
    }
}

#[cfg(test)]
mod tests;
