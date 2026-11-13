//! Source-backed callback selection and ordinary traced stored callback providers.
use crate::{
    closure::calls::SelectedClosure,
    error::RuntimeError,
    native::{
        arguments::CallArguments,
        binding::NativeResult,
        context::{NativeCall, NativeEntry, ScriptCall, ScriptInvoker},
        conversion::{arguments::ArgumentValues, context::ConversionContext},
        scalar::NativeScalar,
        storage::initializer::NativeInit,
    },
    value::{carriers::ScopedValue, closures::ClosureObject, view::ValueView},
    value_check,
};
use kagari_bytecode::module::CallableTarget;
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Edge, Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use kagari_types::ty::Ty;

/// The Native source protects the original callback and its one selected meaning.
/// Publication cannot occur during this synchronous activation. Ordinary stored
/// callbacks keep the original relationship and select again on their next use.
pub struct CallableHandle<'call> {
    selected: SelectedClosure<'call>,
}

impl<'call> CallableHandle<'call> {
    pub(crate) fn from_argument(cx: &mut NativeCall<'call>, index: usize) -> NativeResult<Self> {
        let value = cx.argument(index)?;
        let selected = cx
            .runtime
            .select_closure_call(cx.pending.execution(), &value)?;
        if !selected
            .record()
            .matches_view(cx.argument_type_view(index)?)
        {
            return Err(RuntimeError::module_validation("native callback signature"));
        }
        Ok(Self { selected })
    }

    pub fn call<R: NativeScalar, A: CallArguments>(
        &self,
        cx: &mut NativeCall<'_>,
        arguments: A,
    ) -> NativeResult<R> {
        let signature = self.selected.record().signature();
        if signature.result.ty() != &Ty::Builtin(R::abi_type())
            || !A::matches(signature.params.iter().map(|argument| argument.ty()))
        {
            return Err(RuntimeError::module_validation(
                "native callback conversion differs from its declaration",
            ));
        }
        let invoke = cx.invoke_script;
        arguments.with_values(|arguments| {
            let mut conversion = ConversionContext::in_native_call(cx)?;
            let value = self.invoke(&mut conversion, invoke, ArgumentValues::scalars(arguments))?;
            R::decode(value)
        })
    }

    /// Dynamic original sources are admitted before the preparation suffix grows.
    /// Completed results move into the enclosing Native suffix before cleanup.
    pub fn call_values<'invoke>(
        &self,
        cx: &mut NativeCall<'invoke>,
        arguments: &[ValueView<'_>],
    ) -> NativeResult<ValueView<'invoke>> {
        let signature = self.selected.record().signature();
        if arguments.len() != signature.params.len() {
            return Err(RuntimeError::module_validation("native callback arguments"));
        }
        let invoke = cx.invoke_script;
        let execution = cx.pending.execution();
        let mut conversion = ConversionContext::in_native_call(cx)?;
        let arguments = conversion.argument_values(arguments)?;
        for (index, expected) in signature.params.iter().enumerate() {
            let value = arguments.get(&mut conversion, index)?;
            if !value_check::matches_protected(
                conversion.runtime_mut(),
                &self.selected.record().implementation,
                expected,
                &value,
            )? {
                return Err(RuntimeError::module_validation("native callback arguments"));
            }
        }
        let value = self.invoke(&mut conversion, invoke, arguments)?;
        if !value_check::matches_protected(
            conversion.runtime_mut(),
            &self.selected.record().implementation,
            &signature.result,
            &value,
        )? {
            return Err(RuntimeError::module_validation("native callback result"));
        }
        let value = conversion.into_native_value(execution, value)?;
        Ok(value.reborrow())
    }

    fn invoke<'result>(
        &self,
        conversion: &mut ConversionContext<'result>,
        invoke: ScriptInvoker,
        arguments: ArgumentValues<'_>,
    ) -> NativeResult<ValueView<'result>> {
        let (location, destination) = conversion.result_place()?;
        let published = if self.selected.environment().is_none()
            && self.selected.record().capture_types.is_empty()
            && let CallableTarget::Native(import) = self.selected.target()
        {
            let function = conversion
                .runtime()
                .modules
                .native_binding(self.selected.owner(), import)
                .ok_or_else(|| RuntimeError::module_validation("unlinked native closure"))?;
            let mut call = NativeCall::new(
                conversion.runtime_mut(),
                self.selected.owner(),
                NativeEntry::Closed(&function),
                self.selected.origin(),
                arguments.reborrow(),
                invoke,
            )?;
            let published = call.invoke(destination)?;
            drop(call);
            published
        } else {
            invoke(
                conversion.runtime_mut(),
                self.selected.owner(),
                ScriptCall::Closure(&self.selected),
                arguments,
                destination,
            )?
        };
        conversion.published_result(published, location)
    }

    /// Allocate a traced child in the enclosing provider's branded constructor.
    /// Store its returned Gc in a reviewed provider edge; it cannot escape the brand.
    pub fn store<'gc>(
        &self,
        init: &mut NativeInit<'_, '_, 'gc>,
    ) -> Result<Gc<'gc, StoredCallable>, GcError> {
        let object = init.object(&self.selected.value())?;
        let object = init.scope().downcast::<ClosureObject>(object)?;
        // SAFETY: the actual source is admitted before construction, and the
        // returned object owns/scans exactly this edge. No unrooted edge escapes.
        unsafe {
            init.scope().alloc_with(|mut init| {
                Ok(StoredCallable {
                    value: init.edge_scoped(object)?,
                })
            })
        }
    }
}

/// A concrete managed provider, not a clonable detached heap-ID descriptor.
/// Cloning an owning container is responsible for importing its actual edge;
/// callback access first publishes a child view and then releases all payload loans.
#[derive(Debug)]
pub struct StoredCallable {
    value: Edge<ClosureObject>,
}

// SAFETY: the one initialized edge is immutable and fully scanned. No shared
// mutation, hidden script state, peer access during Drop or indirect backing exists.
unsafe impl GcObject for StoredCallable {
    type ScanState = bool;
    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> bool {
        false
    }

    fn scan(
        &self,
        complete: &mut bool,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if *complete {
            return Ok(ScanStatus::Complete);
        }
        if !budget.take(1) {
            return Ok(ScanStatus::Pending);
        }
        // SAFETY: this admitted provider retains the initialized edge.
        let status = unsafe { visitor.try_visit_stored(&self.value, budget)? };
        *complete = status == ScanStatus::Complete;
        Ok(status)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge { bytes: 0, units: 1 }
    }
}

impl<'call> NativeCall<'call> {
    /// Select an actual stored callback in a concrete Native payload. The provider
    /// selector is nonexecuting and receives no Runtime service. It may return the
    /// payload itself or a followed child; its Gc brand cannot cross this loan.
    pub fn callable_payload<S: GcObject>(
        &mut self,
        receiver: &ValueView<'_>,
        select: for<'gc> fn(&Scope<'gc>, Gc<'gc, S>) -> Option<Gc<'gc, StoredCallable>>,
    ) -> NativeResult<Option<CallableHandle<'call>>> {
        let execution = self.pending.execution();
        let value = self.with_payload_source(receiver, |slots, payload| {
            let Some(stored) = select(slots.scope(), payload) else {
                return Ok(None);
            };
            let closure = slots
                .scope()
                .follow(stored, |stored| Some(&stored.value))
                .expect("initialized stored callback");
            let value = ScopedValue::managed(closure);
            let location = slots.push_local(execution, value)?;
            ValueView::protected(value, Some(location)).map(Some)
        })?;
        let Some(value) = value else {
            return Ok(None);
        };
        let selected = self.runtime.select_closure_call(execution, &value)?;
        Ok(Some(CallableHandle { selected }))
    }
}

#[cfg(test)]
mod tests;
