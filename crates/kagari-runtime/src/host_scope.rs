use crate::{
    Runtime,
    error::RuntimeError,
    gc::roots::RootSet,
    host::{HostBorrowKind, HostCallGuard},
    session::ExecutionSession,
    value::{EphemeralValue, Value},
};
use std::cell::RefCell;

/// Scoped leases into runtime-owned borrow and root tables.
/// The guard unregisters its identity and releases leases before leaving the session.
#[must_use]
pub struct HostResourceScope<'a> {
    runtime: &'a Runtime,
    borrows: HostCallGuard<'a>,
    roots: RefCell<Vec<RootSet>>,
    session: Option<ExecutionSession<'a>>,
}

impl<'a> HostResourceScope<'a> {
    pub(crate) fn new(runtime: &'a Runtime, values: &[Value]) -> Result<Self, RuntimeError> {
        runtime.resources().poll_execution()?;
        let session = runtime
            .execution_root()
            .map(|root| runtime.begin_execution(&root, runtime.execution_options()))
            .transpose()?;
        let borrows = runtime
            .host_borrows
            .enter_frame_in(Some(runtime.resources()))?;
        if let Some(session) = &session {
            let session = session.state();
            let mut scopes = session.host_scopes.borrow_mut();
            scopes
                .try_reserve(1)
                .map_err(|_| runtime.resources().limit("host scope capacity"))?;
            scopes.insert(borrows.frame_id());
        }
        let scope = Self {
            runtime,
            borrows,
            roots: RefCell::new(Vec::new()),
            session,
        };
        scope.retain_values(values)?;
        Ok(scope)
    }

    pub fn runtime(&self) -> &'a Runtime {
        self.runtime
    }

    pub fn borrows(&self) -> &HostCallGuard<'_> {
        &self.borrows
    }

    /// Keep values alive through subsequent host preparation and nested calls.
    /// Permanent host retention uses RootedValue instead.
    pub fn retain_values(&self, values: &[Value]) -> Result<(), RuntimeError> {
        self.runtime.resources().ensure_execution_allowed()?;
        self.validate_borrows(values)?;
        if values.is_empty() {
            return Ok(());
        }
        let roots = self
            .runtime
            .gc
            .root_execution_values(values.to_vec())
            .ok_or_else(|| {
                RuntimeError::host_call_failure("invalid heap reference in host temporaries")
            })?;
        let mut retained = self.roots.borrow_mut();
        retained.try_reserve(1).map_err(|_| {
            self.runtime
                .resources()
                .limit("host temporary root capacity")
        })?;
        retained.push(roots);
        Ok(())
    }

    fn validate_borrows(&self, values: &[Value]) -> Result<(), RuntimeError> {
        let heap = self.runtime.gc();
        let invalid = || RuntimeError::host_call_failure("invalid scoped host value");
        let mut pending = values.to_vec();
        while let Some(value) = pending.pop() {
            match value {
                Value::Tuple(id) => {
                    pending.extend(heap.tuple(id).ok_or_else(invalid)?.iter().copied())
                }
                Value::HostRoot(id) => {
                    let root = heap.host_root(id).ok_or_else(invalid)?;
                    if !self.runtime.host().matches_root(root) {
                        return Err(RuntimeError::host_call_failure(
                            "host root belongs to another registry or is not registered",
                        ));
                    }
                }
                Value::HostPathView(id) => {
                    let view = heap.host_path(id).ok_or_else(invalid)?;
                    if !self.runtime.host().matches_root(view.root()) {
                        return Err(RuntimeError::host_call_failure(
                            "host path view belongs to another registry or has an unregistered root",
                        ));
                    }
                }
                Value::Ephemeral(id) => match heap.ephemeral(id).ok_or_else(invalid)? {
                    EphemeralValue::HostRef(token) => self
                        .runtime
                        .validate_host_borrow(token, HostBorrowKind::Shared)?,
                    EphemeralValue::HostMut(token) => self
                        .runtime
                        .validate_host_borrow(token, HostBorrowKind::Unique)?,
                    EphemeralValue::Runtime(_) => {}
                },
                _ => {}
            }
        }
        Ok(())
    }
}

impl Drop for HostResourceScope<'_> {
    fn drop(&mut self) {
        if let Some(session) = &self.session {
            session
                .state()
                .host_scopes
                .borrow_mut()
                .remove(&self.borrows.frame_id());
        }
    }
}
