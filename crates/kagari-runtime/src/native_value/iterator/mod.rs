//! Returned iterators trace idle captures and create fresh bounded invocations.
pub mod data;
pub(crate) mod invocation;
use crate::{
    gc::{
        RootSet,
        managed_iter::{ManagedIterContract, ManagedIterLease},
    },
    native_module::types::TypeExpression,
    native_value::{
        NativeCall, NativeResult, NativeValue,
        continuation::NativeContinuation,
        invalid,
        iterator::{data::NativeStateData, invocation::StateFactory},
        representation::NativeRepresentation,
    },
    value::Value,
};
use kagari_abi::{
    native_import::NativeSignature,
    standard::surface::StandardEnum,
    types::{AbiType, native::NativeTypeConstructor},
};
use std::{cell::RefCell, marker::PhantomData, rc::Rc};

/// Shared iterator identity, with checked metadata and an active invocation root.
pub struct NativeIterator<T: NativeValue> {
    call: NativeCall,
    root: RootSet,
    item: AbiType,
    _item: PhantomData<T>,
}

/// Declared traversal dependencies; optional storage supports script sources
/// whose own selected next implementation controls their traversal policy.
#[derive(Clone, Copy, Debug)]
pub enum NativeStateDependency {
    Collection(usize),
    OptionalCollection(usize),
}

/// One fresh access to managed captures and owned cursor data. No heap borrow
/// crosses a callback; aliases cannot start another step while this lease is live.
pub struct NativeStateCall<P: NativeStateData> {
    call: NativeCall,
    lease: Rc<ManagedIterLease>,
    _payload: PhantomData<P>,
}
impl<P: NativeStateData> NativeStateCall<P> {
    pub fn call(&self) -> NativeResult<&NativeCall> {
        self.lease.validate()?;
        Ok(&self.call)
    }
    pub fn data(&self) -> NativeResult<P> {
        self.lease.validate()?;
        self.lease.heap.managed_iter_data(self.lease.id)
    }
    /// Commit cursor progress explicitly; completed progress survives later traps.
    pub fn set_data(&self, data: P) -> NativeResult<()> {
        self.lease.validate()?;
        self.lease.heap.set_managed_iter_data(self.lease.id, data)
    }
    /// Replace an existing capture under its constructor's exact concrete type.
    pub fn set_argument<T: NativeValue>(&self, slot: usize, value: T) -> NativeResult<()> {
        self.lease.validate()?;
        let expected = self.call.signature.params.get(slot).ok_or_else(invalid)?;
        let scope = self.call.conversion_scope();
        let value = value.write(&scope, expected)?;
        self.lease
            .heap
            .set_managed_iter_capture(self.lease.id, slot, value.clone())?;
        self.call
            .arguments
            .set(&self.call.heap, slot, value)
            .ok_or_else(invalid)
    }
}
impl<T: NativeValue> NativeIterator<T> {
    /// Capture the constructor's checked arguments and selected targets as GC
    /// edges. The function pointer cannot capture an untraced Rust environment.
    /// Dependencies name collection arguments whose traversal guards are scoped
    /// to each next call; construction neither traverses nor invokes callbacks.
    pub fn new<P: NativeStateData>(
        call: &NativeCall,
        data: P,
        dependencies: &[NativeStateDependency],
        step: fn(NativeStateCall<P>) -> NativeResult<NativeContinuation<Option<T>>>,
    ) -> NativeResult<Self> {
        let AbiType::Iter(item) = call.result_type() else {
            return Err(invalid());
        };
        let captures = (0..call.signature.params.len())
            .map(|slot| call.arguments.get(slot))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(invalid)?;
        let retention = call
            .modules
            .retain_runtime_program(&call.owner)
            .ok_or_else(invalid)?;
        let contract = ManagedIterContract {
            owner: call.owner.clone(),
            signature: NativeSignature {
                params: call.signature.params.clone(),
                result: AbiType::StandardEnum {
                    kind: StandardEnum::Option,
                    args: vec![(**item).clone()],
                },
            },
            item: (**item).clone(),
            callables: call.callables.clone(),
        };
        let id = call.heap.alloc_managed_iter(
            contract,
            captures,
            dependencies,
            data,
            Rc::new(StateFactory::new(step)),
            retention,
        )?;
        Self::read(call, Value::GcHandle(id), call.result_type())
    }
    /// Indexed and application-managed iterators share one checked public entry.
    pub fn next(&self) -> NativeResult<NativeContinuation<Option<T>>> {
        let expected = AbiType::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![self.item.clone()],
        };
        let mut call = self.call.conversion_scope();
        call.signature.result = expected;
        invocation::next(&call, self.root.get(0).ok_or_else(invalid)?, &self.item)
    }
}
impl<T: NativeValue> NativeRepresentation for NativeIterator<T> {
    const CONSTRUCTOR: NativeTypeConstructor = NativeTypeConstructor::Iter;
}
impl<T: NativeValue> NativeValue for NativeIterator<T> {
    fn type_expression(generics: &[&'static str]) -> TypeExpression {
        TypeExpression::Iter(Box::new(T::type_expression(generics)))
    }
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        let AbiType::Iter(item) = expected else {
            return Err(invalid());
        };
        call.check(&value, expected)?;
        Ok(Self {
            call: call.clone(),
            root: call
                .heap
                .root_execution_values(vec![value])
                .ok_or_else(invalid)?,
            item: (**item).clone(),
            _item: PhantomData,
        })
    }
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        call.compatible(&self.call)?;
        if expected != &AbiType::Iter(Box::new(self.item)) {
            return Err(invalid());
        }
        let value = self.root.get(0).ok_or_else(invalid)?;
        call.check(&value, expected)?;
        call.retain(value)
    }
}

fn resumed_call(
    call: &NativeCall,
    contract: &ManagedIterContract,
    captures: Vec<Value>,
) -> NativeResult<NativeCall> {
    Ok(NativeCall {
        heap: call.heap.clone(),
        modules: call.modules.clone(),
        owner: contract.owner.clone(),
        signature: contract.signature.clone(),
        callables: contract.callables.clone(),
        arguments: call
            .heap
            .root_execution_values(captures)
            .ok_or_else(invalid)?,
        temporaries: Rc::new(RefCell::new(vec![])),
    })
}
