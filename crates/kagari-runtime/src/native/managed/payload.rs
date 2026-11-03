//! Stable Native shells and one reviewed boundary for edge-free Rust payloads.

use crate::native::{
    binding::NativeResult,
    managed::{call::CallRegion, error::failure},
};
use kagari_gc::{
    access::{Init, Mutation},
    error::GcError,
    object::{Edge, Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};

/// Rust data with no managed edges, roots, executable leases or script callbacks.
///
/// # Safety
/// All values, including interior state, must stay recursively free of those
/// capabilities or unaccounted owned backing capacity. Drop cannot inspect managed peers or reenter the owning heap.
/// Ordinary Native methods need no additional unsafe implementation.
pub unsafe trait EdgeFree: Send + 'static {}

// SAFETY: integers contain no references, roots, destructors or callbacks.
unsafe impl EdgeFree for u64 {}

/// An edge-free representation admitted through the EdgeFree provider contract.
pub struct Leaf<T: EdgeFree>(pub T);

// SAFETY: EdgeFree proves the complete absence of managed dependencies.
unsafe impl<T: EdgeFree> GcObject for Leaf<T> {
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

/// Stable physical representation: take changes payload state, never its descriptor.
pub struct NativeObject<P: GcObject> {
    pub(super) payload: Option<P>,
}

impl<P: GcObject> NativeObject<P> {
    /// Construct a traced payload through its reviewed provider initializer.
    ///
    /// # Safety
    /// Honor Scope::alloc_with's complete tracing and nonescaping-edge contract.
    pub unsafe fn allocate<'gc>(
        region: &mut CallRegion<'_, 'gc>,
        initialize: impl for<'init> FnOnce(Init<'init>) -> Result<P, GcError>,
    ) -> NativeResult<Gc<'gc, Self>> {
        // SAFETY: caller supplies the provider's initialized traced payload.
        unsafe {
            region.scope_mut().alloc_with(|init| {
                Ok(Self {
                    payload: Some(initialize(init)?),
                })
            })
        }
        .map_err(failure)
    }

    /// Mutate a present payload with shell-bound publication/accounting services.
    ///
    /// # Safety
    /// Honor Scope::with_object_mut's publication, scanner, accounting and unwind
    /// contract. Expose only reviewed provider operations, never arbitrary &mut P.
    pub unsafe fn mutate<'gc, R>(
        region: &mut CallRegion<'_, 'gc>,
        object: Gc<'gc, Self>,
        operation: impl for<'op> FnOnce(&'op mut P, &mut Mutation<'op, Self>) -> Result<R, GcError>,
    ) -> NativeResult<R> {
        // SAFETY: inherits the caller's complete provider mutation proof.
        unsafe {
            region
                .scope_mut()
                .with_object_mut(object, |shell, mutation| {
                    let payload = shell
                        .payload
                        .as_mut()
                        .ok_or(GcError::Storage("native payload was taken"))?;
                    operation(payload, mutation)
                })
        }
        .map_err(failure)
    }

    /// Load an internal edge while its admitted present payload stays retained.
    pub fn follow<'gc, U: 'static>(
        region: &CallRegion<'_, 'gc>,
        object: Gc<'gc, Self>,
        select: impl for<'a> Fn(&'a P) -> Option<&'a Edge<U>>,
    ) -> NativeResult<Option<Gc<'gc, U>>> {
        Self::read(region, object, |_| ())?;
        Ok(region
            .scope()
            .follow(object, |shell| shell.payload.as_ref().and_then(&select)))
    }

    pub fn read<'gc, R>(
        region: &CallRegion<'_, 'gc>,
        object: Gc<'gc, Self>,
        read: impl FnOnce(&P) -> R,
    ) -> NativeResult<R> {
        region
            .scope()
            .read(object, |shell| {
                shell
                    .payload
                    .as_ref()
                    .map(read)
                    .ok_or(GcError::Storage("native payload was taken"))
            })
            .map_err(failure)
    }
}

impl<T: EdgeFree> NativeObject<Leaf<T>> {
    pub fn alloc<'gc>(region: &mut CallRegion<'_, 'gc>, value: T) -> NativeResult<Gc<'gc, Self>> {
        region
            .scope_mut()
            .alloc_leaf(Self {
                payload: Some(Leaf(value)),
            })
            .map_err(failure)
    }

    pub fn edit<'gc, R>(
        region: &mut CallRegion<'_, 'gc>,
        object: Gc<'gc, Self>,
        edit: impl FnOnce(&mut T) -> R,
    ) -> NativeResult<R> {
        // SAFETY: only edge-free T is exposed; it cannot hide an edge, alter
        // scanner state or add accounted managed capacity. The Scope is exclusive.
        unsafe {
            region.scope_mut().with_object_mut(object, |shell, _| {
                let value = shell
                    .payload
                    .as_mut()
                    .ok_or(GcError::Storage("native payload was taken"))?;
                Ok(edit(&mut value.0))
            })
        }
        .map_err(failure)
    }

    pub fn take<'gc>(region: &mut CallRegion<'_, 'gc>, object: Gc<'gc, Self>) -> NativeResult<T> {
        // SAFETY: extracted T is recursively edge/root/code-free; shell identity
        // and descriptor stay unchanged and the provider kernel repairs scanning.
        unsafe {
            region.scope_mut().with_object_mut(object, |shell, _| {
                shell
                    .payload
                    .take()
                    .map(|leaf| leaf.0)
                    .ok_or(GcError::Storage("native payload was taken"))
            })
        }
        .map_err(failure)
    }
}

// SAFETY: payload is private; scanned/charged through the same P implementation.
// Taking is exposed only for Leaf<T>, which cannot contain managed dependencies.
unsafe impl<P: GcObject> GcObject for NativeObject<P> {
    type ScanState = Option<P::ScanState>;

    const SCAN_KIND: ScanKind = P::SCAN_KIND;

    fn begin_scan(&self) -> Self::ScanState {
        self.payload.as_ref().map(GcObject::begin_scan)
    }

    fn scan(
        &self,
        state: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        match (&self.payload, state) {
            (Some(payload), Some(state)) => payload.scan(state, budget, visitor),
            (None, _) => Ok(ScanStatus::Complete),
            _ => Err(GcError::Provider("native scanner state")),
        }
    }

    fn charge(&self) -> ObjectCharge {
        self.payload
            .as_ref()
            .map_or_else(ObjectCharge::default, GcObject::charge)
    }
}
