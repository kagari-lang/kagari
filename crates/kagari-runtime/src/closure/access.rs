//! Closure observation reads actual providers and returns immutable facts only.
use crate::{
    Runtime,
    closure::ClosureRecord,
    error::{RuntimeError, trap::TrapReason},
    execution::roots::ExecutionRoots,
    value::{closures::ClosureObject, kind::ObjectKind, view::ValueView},
};
use kagari_gc::error::GcError;
use std::sync::Arc;

impl Runtime {
    /// Read immutable call facts from a protected closure. The returned record
    /// owns neither captures nor executable state; keep the original source live
    /// through admission or explicitly retain the closure for Host ownership.
    pub fn resolve_closure(
        &mut self,
        value: &ValueView<'_>,
    ) -> Result<Arc<ClosureRecord>, RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.ensure_execution_allowed()?;
        if value.kind() != Some(ObjectKind::Closure) {
            return Err(TrapReason::ExpectedClosure.into());
        }
        self.memory
            .scope(&self.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let value = value.get(slots)?;
                    let object = slots.scope().downcast::<ClosureObject>(
                        value.object().ok_or(GcError::Representation)?,
                    )?;
                    Ok(slots.scope().read(object, |object| object.record().clone()))
                })
            })
            .map_err(|error| match error {
                GcError::Index | GcError::Representation | GcError::InvalidRoot => {
                    TrapReason::InvalidClosureHandle.into()
                }
                GcError::ForeignHeap => RuntimeError::module_validation("foreign closure source"),
                error => self.managed_memory_error(error),
            })
    }
}
