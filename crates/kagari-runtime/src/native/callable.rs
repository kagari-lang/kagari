//! Prepared function arguments and explicitly traced or rooted retained callbacks.
use crate::{
    Runtime,
    error::RuntimeError,
    gc::{ClosureValueSnapshot, GcObjectKind, RootedValue},
    native::{
        arguments::CallArguments,
        binding::NativeResult,
        context::{ArgumentView, CallContext, ScriptCall},
        scalar::NativeScalar,
        storage::NativePayload,
    },
    value::Value,
};
use kagari_abi::types::AbiType;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct PreparedClosure {
    value: Value,
    closure: Rc<ClosureValueSnapshot>,
}

impl PreparedClosure {
    pub(crate) fn validate(&self, runtime: &Runtime) -> NativeResult<()> {
        runtime.gc().ensure_no_native_borrow()?;
        let Value::Closure(id) = self.value else {
            return Err(RuntimeError::module_validation("native callback handle"));
        };
        if runtime.gc().object_kind(id) != Some(GcObjectKind::Closure) {
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

    pub fn snapshot(&self) -> &ClosureValueSnapshot {
        &self.closure
    }

    fn invoke(&self, cx: &CallContext<'_>, arguments: &[Value]) -> NativeResult<Value> {
        (cx.invoke_script)(
            cx.runtime,
            &self.closure.implementation,
            ScriptCall::Closure(self),
            arguments,
        )
    }

    fn call<R: NativeScalar, A: CallArguments>(
        &self,
        cx: &CallContext<'_>,
        params: &[AbiType],
        result: &AbiType,
        arguments: A,
    ) -> NativeResult<R> {
        if result != &R::abi_type() || !A::matches(params) {
            return Err(RuntimeError::module_validation(
                "native callback conversion differs from its declaration",
            ));
        }
        cx.heap().ensure_no_native_borrow()?;
        arguments.with_values(|arguments| self.invoke(cx, arguments).and_then(R::decode))
    }

    fn call_values(&self, cx: &CallContext<'_>, arguments: &[Value]) -> NativeResult<Value> {
        cx.heap().ensure_no_native_borrow()?;
        let owner = &self.closure.implementation;
        let function = owner
            .bytecode
            .functions
            .get(self.closure.function.index())
            .ok_or_else(|| RuntimeError::module_validation("native callback function"))?;
        let captures = self.closure.captures.len();
        let environment = self.closure.environment.as_deref();
        if captures.checked_add(arguments.len()) != Some(function.metadata.params.len())
            || !arguments.iter().enumerate().all(|(index, value)| {
                function
                    .metadata
                    .semantic
                    .params
                    .get(&(captures + index))
                    .is_some_and(|ty| cx.runtime.matches_type_in(value, ty, owner, environment))
            })
        {
            return Err(RuntimeError::module_validation("native callback arguments"));
        }
        let value = self.invoke(cx, arguments)?;
        if !function
            .metadata
            .semantic
            .result
            .as_ref()
            .is_some_and(|ty| cx.runtime.matches_type_in(&value, ty, owner, environment))
        {
            return Err(RuntimeError::module_validation("native callback result"));
        }
        Ok(value)
    }
}

/// A function argument borrows its existing frame root. Resolving the closure
/// shares immutable capture metadata and never holds a heap borrow across calls.
pub struct CallableHandle<'call> {
    target: PreparedClosure,
    params: &'call [AbiType],
    result: &'call AbiType,
    _arguments: ArgumentView<'call>,
}

impl<'call> CallableHandle<'call> {
    pub(crate) fn from_argument(cx: &CallContext<'call>, index: usize) -> NativeResult<Self> {
        cx.heap().ensure_no_native_borrow()?;
        let AbiType::Function { params, result } = cx.argument_type(index)? else {
            return Err(RuntimeError::module_validation(
                "native callback declaration",
            ));
        };
        let value = cx.argument(index)?;
        let closure = cx.runtime.resolve_closure(&value)?;
        cx.runtime.validate_loaded_module(&closure.implementation)?;
        let expected = cx.argument_type_view(index)?;
        let AbiType::Function {
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
            target: PreparedClosure { value, closure },
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

    /// Values read from mutable storage need working roots before callbacks.
    /// A returned GC value needs an explicit root before further reentry or GC.
    pub fn call_values(
        &self,
        cx: &mut CallContext<'_>,
        arguments: &[Value],
    ) -> NativeResult<Value> {
        self.target.call_values(cx, arguments)
    }

    /// Retained payloads must trace this value; this does not create a global root.
    pub fn store(&self) -> StoredCallable {
        StoredCallable(Rc::new(StoredFunction {
            target: self.target.clone(),
            params: self.params.into(),
            result: self.result.clone(),
        }))
    }
}

#[derive(Debug)]
struct StoredFunction {
    target: PreparedClosure,
    params: Box<[AbiType]>,
    result: AbiType,
}

/// Embed this immutable descriptor in a NativePayload and visit it in trace.
/// Clones share metadata. An unreachable payload and its closure can be collected
/// together, including cycles; no host root is embedded in the script heap.
#[derive(Debug, Clone)]
pub struct StoredCallable(Rc<StoredFunction>);

impl NativePayload for StoredCallable {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        visit(&self.0.target.value);
    }

    fn units(&self) -> usize {
        1
    }
}

impl StoredCallable {
    /// Invoke a descriptor retained by a currently rooted native payload. The
    /// closure is validated before entering the prepared frame; callers retaining
    /// it independently should use root(). Returned values need a root before GC.
    pub fn call_values(
        &self,
        cx: &mut CallContext<'_>,
        arguments: &[Value],
    ) -> NativeResult<Value> {
        self.0.target.validate(cx.runtime)?;
        self.0.target.call_values(cx, arguments)
    }

    /// Root the closure while using a stored descriptor outside its payload borrow.
    /// Reuse this handle for the whole loop, then drop it. Store StoredCallable,
    /// rather than this owning root, in a GC payload.
    pub fn root(&self, cx: &CallContext<'_>) -> NativeResult<RootedCallable> {
        self.0.target.validate(cx.runtime)?;
        let root = cx
            .heap()
            .root_value(self.0.target.value.clone())
            .ok_or_else(|| RuntimeError::module_validation("native callback root"))?;
        Ok(RootedCallable {
            stored: self.clone(),
            _root: root,
        })
    }
}

#[derive(Clone)]
pub struct RootedCallable {
    stored: StoredCallable,
    _root: RootedValue,
}

impl RootedCallable {
    pub fn call<R: NativeScalar, A: CallArguments>(
        &self,
        cx: &mut CallContext<'_>,
        arguments: A,
    ) -> NativeResult<R> {
        let function = &self.stored.0;
        function
            .target
            .call(cx, &function.params, &function.result, arguments)
    }

    pub fn call_values(
        &self,
        cx: &mut CallContext<'_>,
        arguments: &[Value],
    ) -> NativeResult<Value> {
        let function = &self.stored.0;
        function.target.call_values(cx, arguments)
    }
}
