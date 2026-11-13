//! Branded construction exposes sources only during a noncollecting operation.
use crate::{
    execution::roots::Slots,
    value::{carriers::ScopedValue, properties::ValueProperties, view::ValueView},
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{AnyObject, Gc},
};

use std::{marker::PhantomData, rc::Rc};

/// Low-level provider construction. A custom provider still owns GcObject's unsafe
/// tracing, initialization, publication, charge and destruction obligations.
/// No Runtime execution/collection service is available while this scope is lent.
pub struct NativeInit<'loan, 'source, 'gc> {
    pub(crate) slots: &'loan mut Slots<'source, 'gc>,
    pub(crate) _thread: PhantomData<Rc<()>>,
}

impl<'gc> NativeInit<'_, '_, 'gc> {
    pub fn scope(&mut self) -> &mut Scope<'gc> {
        self.slots.scope()
    }

    /// Admit a protected storable script value for a concrete provider field.
    /// Scalars stay inline; managed values borrow this brand without a Host root.
    /// Store the result through
    /// [StoredValue::initialize](crate::value::carriers::StoredValue::initialize)
    /// in the allocated provider. Expired/foreign sources and scoped values fail
    /// admission before construction.
    pub fn value(&mut self, value: &ValueView<'_>) -> Result<ScopedValue<'gc>, GcError> {
        let value = value.get(self.slots)?;
        if !ValueProperties::of(self.slots.scope(), value)?.storable {
            return Err(GcError::Representation);
        }
        Ok(value)
    }

    /// Admit a protected storable script object, without extending its retention
    /// beyond this brand. Scalar inputs have no object and return Representation.
    pub fn object(&mut self, value: &ValueView<'_>) -> Result<Gc<'gc, AnyObject>, GcError> {
        self.value(value)?.object().ok_or(GcError::Representation)
    }
}
