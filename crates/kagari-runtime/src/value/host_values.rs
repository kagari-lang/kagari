//! Immutable Host descriptors contain capabilities, never Rust state addresses.
pub(crate) mod paths;
#[cfg(test)]
mod tests;
use crate::{
    host::HostRootHandle,
    value::{
        EphemeralValue,
        carriers::{ManagedRepresentation, ScopedValue},
        kind::ObjectKind,
    },
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};

/// The owning Host registry validates this capability before any projection.
/// Retaining its GC wrapper cannot extend the underlying Host reservation.
#[derive(Debug)]
pub(crate) struct HostRootObject {
    handle: HostRootHandle,
}

impl HostRootObject {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        handle: HostRootHandle,
    ) -> Result<Gc<'gc, Self>, GcError> {
        cx.alloc_leaf(Self { handle })
    }

    pub(crate) fn handle(&self) -> HostRootHandle {
        self.handle
    }
}

/// Frame loans and scoped Host projections remain revocable Host capabilities.
/// Runtime-only opaque temporaries stay inline and allocate no wrapper.
#[derive(Debug)]
pub(crate) struct EphemeralObject {
    value: EphemeralValue,
}

impl EphemeralObject {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        value: EphemeralValue,
    ) -> Result<ScopedValue<'gc>, GcError> {
        match value {
            EphemeralValue::Runtime(id) => Ok(ScopedValue::runtime_ephemeral(id)),
            value => cx.alloc_leaf(Self { value }).map(ScopedValue::managed),
        }
    }

    pub(crate) fn value(&self) -> EphemeralValue {
        self.value
    }
}

macro_rules! host_leaf {
    ($object:ty, $kind:ident) => {
        impl ManagedRepresentation for $object {
            const KIND: ObjectKind = ObjectKind::$kind;
        }

        // SAFETY: immutable capability IDs have no managed edges, Rust pointers,
        // indirect backing or peer-aware destructors. Actual Host access stays
        // at the checked registry/loan boundary, outside the script heap.
        unsafe impl GcObject for $object {
            type ScanState = ();

            const SCAN_KIND: ScanKind = ScanKind::EdgeFree;

            fn begin_scan(&self) {}

            fn scan(
                &self,
                _: &mut (),
                _: &mut ScanBudget,
                _: &mut EdgeVisitor<'_>,
            ) -> Result<ScanStatus, GcError> {
                Ok(ScanStatus::Complete)
            }
        }
    };
}

host_leaf!(HostRootObject, HostRoot);
host_leaf!(EphemeralObject, Ephemeral);
