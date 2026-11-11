use crate::{
    Runtime,
    error::RuntimeError,
    gc::roots::RootSet,
    host::{HostBorrowKind, HostCallGuard, HostFrameMark},
    publication::ActivationMark,
    session::{ExecutionEntry, ExecutionSession, SessionScope},
    value::{EphemeralValue, Value},
};
pub mod scoped;

/// Scoped leases into runtime-owned borrow and root tables.
/// The guard unregisters its identity and releases leases before leaving the session.
#[must_use]
pub struct HostResourceScope<'a> {
    runtime: &'a mut Runtime,
    activation: Option<ActivationMark>,
    borrows: Option<HostFrameMark>,
    roots: Vec<RootSet>,
    session: Option<SessionScope>,
}

impl<'a> HostResourceScope<'a> {
    pub(crate) fn new(runtime: &'a mut Runtime, values: &[Value]) -> Result<Self, RuntimeError> {
        runtime.resources().poll_execution()?;
        let session = match runtime.execution_root() {
            Some(root) => Some(runtime.admit_execution(
                &root,
                runtime.execution_options(),
                ExecutionEntry::Program,
            )?),
            None => None,
        };
        let activation = if session.is_none() {
            Some(runtime.resources().begin_synchronous_activation()?.detach())
        } else {
            None
        };
        let mut scope = Self {
            activation,
            runtime,
            borrows: None,
            roots: Vec::new(),
            session,
        };
        let mark = scope
            .runtime
            .host_borrows
            .enter_frame_in(Some(scope.runtime.resources()))?
            .into_mark();
        scope.borrows = Some(mark);
        if let Some(session) = &scope.session {
            let state = session.state(scope.runtime.resources());
            let mut scopes = state.host_scopes.borrow_mut();
            scopes
                .try_reserve(1)
                .map_err(|_| scope.runtime.resources().limit("host scope capacity"))?;
            scopes.insert(scope.borrows().frame_id());
        }
        scope.retain_values(values)?;
        Ok(scope)
    }

    pub fn runtime(&self) -> &Runtime {
        self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut Runtime {
        self.runtime
    }

    pub fn borrows(&self) -> HostCallGuard<'_> {
        HostCallGuard::borrow_mark(
            &self.runtime.host_borrows,
            self.runtime.resources(),
            self.borrows.as_ref().expect("live Host scope"),
        )
    }

    /// Keep values alive through subsequent host preparation and nested calls.
    /// Permanent host retention uses RootedValue instead.
    pub fn retain_values(&mut self, values: &[Value]) -> Result<(), RuntimeError> {
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
        let retained = &mut self.roots;
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
                    self.runtime.host().validate_view(heap, &view)?;
                }
                Value::Ephemeral(id) => match heap.ephemeral(id).ok_or_else(invalid)? {
                    EphemeralValue::HostRef(token) => self
                        .runtime
                        .validate_host_borrow(token, HostBorrowKind::Shared)?,
                    EphemeralValue::HostMut(token) => self
                        .runtime
                        .validate_host_borrow(token, HostBorrowKind::Unique)?,
                    EphemeralValue::Runtime(_) => {}
                    EphemeralValue::HostView(view) => {
                        self.runtime.host_borrows.validate_scoped(view)?
                    }
                },
                _ => {}
            }
        }
        Ok(())
    }
}

impl HostResourceScope<'_> {
    fn release(
        runtime: &Runtime,
        frame: &mut Option<HostFrameMark>,
        roots: &mut Vec<RootSet>,
        session: Option<&SessionScope>,
    ) {
        if let Some(mark) = frame.take() {
            if let Some(session) = session {
                session
                    .state(runtime.resources())
                    .host_scopes
                    .borrow_mut()
                    .remove(
                        &HostCallGuard::borrow_mark(
                            &runtime.host_borrows,
                            runtime.resources(),
                            &mark,
                        )
                        .frame_id(),
                    );
            }
            drop(HostCallGuard::from_mark(
                &runtime.host_borrows,
                runtime.resources(),
                mark,
            ));
        }
        roots.clear();
    }
}

impl Drop for HostResourceScope<'_> {
    fn drop(&mut self) {
        let activation = self.activation.take();
        if let Some(scope) = self.session.take() {
            // Restore a short cleanup guard before releasing Host records, so
            // their unwind cannot leak the surrounding session's admission.
            let session = ExecutionSession {
                runtime: self.runtime,
                scope,
            };
            Self::release(
                session.runtime(),
                &mut self.borrows,
                &mut self.roots,
                Some(&session.scope),
            );
        } else {
            let _activation = activation.map(|mark| mark.attach(self.runtime.resources()));
            Self::release(self.runtime, &mut self.borrows, &mut self.roots, None);
        }
    }
}
