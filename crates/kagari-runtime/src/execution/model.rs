//! RR01 bounded storage/service proofs. This heap never receives legacy Values.
//! The production interpreter continues to own the old graph until RR03.
mod objects;
mod services;
mod source;
mod tests;

use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{AnyObject, Edge, Gc},
    root::Root,
};

/// The proof exercises Unit, a scalar and an erased managed value. It does not
/// establish language nominal identity from a physical GC representation tag.
#[derive(Clone, Copy)]
enum ScopedValue<'gc> {
    Unit,
    I64(i64),
    Managed(Gc<'gc, AnyObject>),
}

enum StoredValue {
    Unit,
    I64(i64),
    Managed(Edge<AnyObject>),
}

enum RootedValue {
    Unit,
    I64(i64),
    Managed(Root<AnyObject>),
}

impl<'gc> ScopedValue<'gc> {
    fn retain(self, cx: &mut Scope<'gc>) -> Result<RootedValue, GcError> {
        Ok(match self {
            Self::Unit => RootedValue::Unit,
            Self::I64(value) => RootedValue::I64(value),
            Self::Managed(value) => RootedValue::Managed(cx.retain(value)?),
        })
    }
}

impl RootedValue {
    fn get<'gc>(&self, cx: &Scope<'gc>) -> Result<ScopedValue<'gc>, GcError> {
        Ok(match self {
            Self::Unit => ScopedValue::Unit,
            Self::I64(value) => ScopedValue::I64(*value),
            Self::Managed(value) => ScopedValue::Managed(cx.get(value)?),
        })
    }
}
