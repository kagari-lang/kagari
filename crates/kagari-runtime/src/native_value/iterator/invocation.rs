//! Every step uses fresh roots; completion checks the retained state result ABI.
use crate::{
    gc::{CollectionIteration, managed_iter::ManagedIterLease},
    native::{NativeAction, NativeContext, NativeInvocationState},
    native_value::{
        NativeCall, NativeResult, NativeReturn, NativeValue,
        continuation::NativeContinuation,
        invalid,
        iterator::{NativeStateCall, data::NativeStateData, resumed_call},
    },
    value::Value,
};
use kagari_abi::{operations::IterOp, types::AbiType};
use std::{marker::PhantomData, rc::Rc};

pub(crate) trait ManagedStateFactory {
    fn start(
        &self,
        call: NativeCall,
        lease: Rc<ManagedIterLease>,
        context: &mut NativeContext<'_>,
    ) -> NativeResult<Box<dyn NativeInvocationState>>;
}
pub(super) struct StateFactory<P: NativeStateData, T: NativeValue> {
    step: fn(NativeStateCall<P>) -> NativeResult<NativeContinuation<Option<T>>>,
}
impl<P: NativeStateData, T: NativeValue> StateFactory<P, T> {
    pub(super) fn new(
        step: fn(NativeStateCall<P>) -> NativeResult<NativeContinuation<Option<T>>>,
    ) -> Self {
        Self { step }
    }
}
impl<P: NativeStateData, T: NativeValue> ManagedStateFactory for StateFactory<P, T> {
    fn start(
        &self,
        call: NativeCall,
        lease: Rc<ManagedIterLease>,
        context: &mut NativeContext<'_>,
    ) -> NativeResult<Box<dyn NativeInvocationState>> {
        (self.step)(NativeStateCall {
            call: call.clone(),
            lease,
            _payload: PhantomData,
        })?
        .into_state(&call, context, false)
    }
}
pub(super) fn next<T: NativeValue>(
    call: &NativeCall,
    value: Value,
    item: &AbiType,
) -> NativeResult<NativeContinuation<Option<T>>> {
    Ok(NativeContinuation::new(Start {
        call: call.clone(),
        value,
        item: item.clone(),
        inner: None,
        lease: None,
        guards: vec![],
        retained_call: None,
    }))
}
struct Start {
    call: NativeCall,
    value: Value,
    item: AbiType,
    inner: Option<Box<dyn NativeInvocationState>>,
    lease: Option<Rc<ManagedIterLease>>,
    guards: Vec<CollectionIteration>,
    retained_call: Option<NativeCall>,
}
impl Drop for Start {
    fn drop(&mut self) {
        if let Some(lease) = &self.lease {
            lease.finish();
        }
    }
}
impl Start {
    fn checked_action(&self, action: NativeAction) -> NativeResult<NativeAction> {
        if let NativeAction::Complete(value) = &action {
            self.retained_call
                .as_ref()
                .unwrap_or(&self.call)
                .check(value, self.call.result_type())?;
        }
        Ok(action)
    }
}
impl NativeInvocationState for Start {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
        if let Some(inner) = &mut self.inner {
            let action = inner.advance(context)?;
            return self.checked_action(action);
        }
        let Some(request) = self
            .call
            .heap
            .managed_iter_request(&self.value, &self.item)?
        else {
            return context
                .iterator_operation(
                    &self.call.owner,
                    &self.value,
                    &AbiType::Iter(Box::new(self.item.clone())),
                    IterOp::Next,
                )
                .map(NativeAction::Complete);
        };
        let call = resumed_call(&self.call, &request.contract, request.captures)?;
        for dependency in request.dependencies {
            self.guards
                .push(self.call.heap.begin_collection_iteration(&dependency)?);
        }
        self.lease = Some(request.lease.clone());
        let mut inner = request
            .factory
            .start(call.clone(), request.lease.clone(), context)?;
        self.retained_call = Some(call);
        let action = inner.advance(context)?;
        self.inner = Some(inner);
        self.checked_action(action)
    }
    fn receive(
        &mut self,
        context: &mut NativeContext<'_>,
        value: Value,
    ) -> NativeResult<NativeAction> {
        let action = self
            .inner
            .as_mut()
            .ok_or_else(invalid)?
            .receive(context, value)?;
        self.checked_action(action)
    }
}
