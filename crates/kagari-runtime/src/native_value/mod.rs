//! Typed native adapters retain explicit roots and portable applied type checks.
pub mod arguments;
pub mod array;
pub mod continuation;
mod conversions;
#[doc(hidden)]
pub mod declaration;
pub mod iterator;
pub mod never;
pub mod number;
pub mod option;
pub mod parse;
pub mod range;
pub mod reorder;
pub mod representation;
pub mod result;
pub mod scalar_protocol;
pub mod selected;
pub mod text;
mod tuples;

use crate::{
    error::RuntimeError,
    gc::{GcHeap, RootSet},
    module::{LoadedModule, ModuleStore},
    native::{NativeAction, NativeContext, NativeInvocationState, factory::NativeFactory},
    native_module::types::TypeExpression,
    value::Value,
};
use kagari_abi::{
    native_import::{NativeSignature, callables::NativeCallableApplication},
    types::AbiType,
};
use std::{cell::RefCell, rc::Rc};

pub type NativeResult<T> = Result<T, RuntimeError>;

/// Rust value conversion under a checked application. Implementations must retain
/// heap values they own and may not expose unrestricted Rust references to script.
pub trait NativeValue: Sized + 'static {
    fn type_expression(generics: &[&'static str]) -> TypeExpression;
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self>;
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value>;
}

/// An owned call view: heap roots and dependency versions remain pinned until drop.
#[derive(Clone)]
pub struct NativeCall {
    pub(super) heap: Rc<GcHeap>,
    pub(super) owner: LoadedModule,
    pub(super) modules: ModuleStore,
    signature: NativeSignature,
    pub(super) callables: Vec<NativeCallableApplication>,
    arguments: RootSet,
    temporaries: Rc<RefCell<Vec<RootSet>>>,
}

impl NativeCall {
    pub(crate) fn new(context: &NativeContext<'_>) -> NativeResult<Self> {
        let heap = context.heap_owner();
        let values = (0..context.signature().params.len())
            .map(|slot| context.argument(slot))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(invalid)?;
        let arguments = heap.root_execution_values(values).ok_or_else(invalid)?;
        Ok(Self {
            heap,
            owner: context.module_owner(),
            modules: context.module_store(),
            signature: context.signature().clone(),
            callables: context.selected_applications().to_vec(),
            arguments,
            temporaries: Rc::new(RefCell::new(vec![])),
        })
    }
    pub fn argument<T: NativeValue>(&self, slot: usize) -> NativeResult<T> {
        let expected = self.signature.params.get(slot).ok_or_else(invalid)?;
        let value = self.arguments.get(slot).ok_or_else(invalid)?;
        self.check(&value, expected)?;
        T::read(self, value, expected)
    }
    pub fn result_type(&self) -> &AbiType {
        &self.signature.result
    }
    /// Charge bounded synchronous native work before performing it.
    /// Length-dependent algorithms must account for their logical input work.
    pub fn charge_work(&self, steps: u64) -> NativeResult<()> {
        self.heap.charge_native_work(steps)
    }
    pub(super) fn check(&self, value: &Value, expected: &AbiType) -> NativeResult<()> {
        if self.heap.matches_abi(value, expected, &self.owner) {
            Ok(())
        } else {
            Err(invalid())
        }
    }
    pub(super) fn retain(&self, value: Value) -> NativeResult<Value> {
        let roots = self
            .heap
            .root_execution_values(vec![value.clone()])
            .ok_or_else(invalid)?;
        self.temporaries.borrow_mut().push(roots);
        Ok(value)
    }
    pub(super) fn compatible(&self, other: &Self) -> NativeResult<()> {
        if Rc::ptr_eq(&self.heap, &other.heap) {
            Ok(())
        } else {
            Err(invalid())
        }
    }
    // Keep the pinned signature/owner, but release conversion roots after a
    // request hands its arguments to the independently rooted callback.
    pub(super) fn conversion_scope(&self) -> Self {
        let mut scope = self.clone();
        scope.temporaries = Rc::new(RefCell::new(vec![]));
        scope
    }
}

/// One compiled Rust value proxy per script generic slot, not a runtime Rust compiler.
/// The concrete script type comes from the pinned native application.
pub struct GenericValue<const SLOT: usize> {
    call: NativeCall,
    root: RootSet,
}
impl<const SLOT: usize> NativeValue for GenericValue<SLOT> {
    fn type_expression(_: &[&'static str]) -> TypeExpression {
        TypeExpression::Parameter(SLOT)
    }
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        call.check(&value, expected)?;
        Ok(Self {
            call: call.clone(),
            root: call
                .heap
                .root_execution_values(vec![value])
                .ok_or_else(invalid)?,
        })
    }
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        call.compatible(&self.call)?;
        let value = self.root.get(0).ok_or_else(invalid)?;
        call.check(&value, expected)?;
        call.retain(value)
    }
}

pub trait NativeReturn: 'static {
    const SCRATCH_SLOTS: usize = 1;
    fn type_expression(generics: &[&'static str]) -> TypeExpression;
    fn into_state(
        self,
        call: &NativeCall,
        context: &mut NativeContext<'_>,
        delay: bool,
    ) -> NativeResult<Box<dyn NativeInvocationState>>;
}
impl<T: NativeValue> NativeReturn for T {
    fn type_expression(generics: &[&'static str]) -> TypeExpression {
        T::type_expression(generics)
    }
    fn into_state(
        self,
        call: &NativeCall,
        context: &mut NativeContext<'_>,
        delay: bool,
    ) -> NativeResult<Box<dyn NativeInvocationState>> {
        context.retain(0, self.write(call, call.result_type())?)?;
        Ok(Box::new(Completed { delay }))
    }
}
impl<T: NativeReturn> NativeReturn for NativeResult<T> {
    const SCRATCH_SLOTS: usize = T::SCRATCH_SLOTS;
    fn type_expression(generics: &[&'static str]) -> TypeExpression {
        T::type_expression(generics)
    }
    fn into_state(
        self,
        call: &NativeCall,
        context: &mut NativeContext<'_>,
        delay: bool,
    ) -> NativeResult<Box<dyn NativeInvocationState>> {
        self?.into_state(call, context, delay)
    }
}

impl NativeFactory {
    pub fn typed<R: NativeReturn>(
        delay: bool,
        function: impl Fn(&NativeCall) -> NativeResult<R> + 'static,
    ) -> Self {
        Self::new(R::SCRATCH_SLOTS, move |context| {
            let call = NativeCall::new(context)?;
            function(&call)?.into_state(&call, context, delay)
        })
    }
}
struct Completed {
    delay: bool,
}
impl NativeInvocationState for Completed {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
        if self.delay {
            self.delay = false;
            return Ok(NativeAction::Continue);
        }
        Ok(NativeAction::Complete(
            context.retained(0).ok_or_else(invalid)?,
        ))
    }
}

pub(super) fn named(name: &'static str) -> TypeExpression {
    TypeExpression::Named {
        path: vec![name],
        arguments: vec![],
        bindings: vec![],
    }
}
pub(super) fn invalid() -> RuntimeError {
    RuntimeError::module_validation("typed native value or application mismatch")
}
