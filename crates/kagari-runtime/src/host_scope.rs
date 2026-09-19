use std::{cell::RefCell, rc::Rc};

use crate::{ExecutionSession, HostCallGuard, Runtime, RuntimeError, gc::RootSet, value::Value};

#[derive(Debug)]
pub(crate) struct HostScopeState {
    borrows: HostCallGuard,
    roots: RefCell<Vec<RootSet>>,
}

/// Temporary host resources. Active script sessions own the registered state;
/// this guard removes it and releases its borrows/roots before leaving the session.
#[must_use]
pub struct HostResourceScope<'a> {
    runtime: &'a Runtime,
    state: Rc<HostScopeState>,
    session: Option<ExecutionSession>,
}

impl<'a> HostResourceScope<'a> {
    pub(crate) fn new(runtime: &'a Runtime, values: &[Value]) -> Result<Self, RuntimeError> {
        runtime.resources.poll_execution()?;
        let session = runtime
            .execution_root()
            .map(|root| runtime.begin_execution(&root, runtime.execution_options()))
            .transpose()?;
        let state = Rc::new(HostScopeState {
            borrows: runtime.host_borrows.enter_frame()?,
            roots: RefCell::new(Vec::new()),
        });
        if let Some(session) = &session {
            let mut scopes = session.state.host_scopes.borrow_mut();
            scopes
                .try_reserve(1)
                .map_err(|_| runtime.resources.limit("host scope capacity"))?;
            scopes.insert(state.borrows.frame_id(), state.clone());
        }
        let scope = Self {
            runtime,
            state,
            session,
        };
        scope.retain_values(values)?;
        Ok(scope)
    }

    pub fn runtime(&self) -> &'a Runtime {
        self.runtime
    }
    pub fn borrows(&self) -> &HostCallGuard {
        &self.state.borrows
    }

    /// Keep values alive through subsequent host preparation and nested calls.
    /// Permanent host retention uses RootedValue instead.
    pub fn retain_values(&self, values: &[Value]) -> Result<(), RuntimeError> {
        self.runtime.resources.ensure_execution_allowed()?;
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
        let mut retained = self.state.roots.borrow_mut();
        retained
            .try_reserve(1)
            .map_err(|_| self.runtime.resources.limit("host temporary root capacity"))?;
        retained.push(roots);
        Ok(())
    }

    fn validate_borrows(&self, values: &[Value]) -> Result<(), RuntimeError> {
        let mut pending = values.iter().collect::<Vec<_>>();
        while let Some(value) = pending.pop() {
            match value {
                Value::Tuple(elements) => pending.extend(elements),
                Value::HostRoot(root) if !self.runtime.host().matches_root(*root) => {
                    return Err(RuntimeError::host_call_failure(
                        "host root belongs to another registry or is not registered",
                    ));
                }
                Value::HostPathView(view) if !self.runtime.host().matches_root(view.root()) => {
                    return Err(RuntimeError::host_call_failure(
                        "host path view belongs to another registry or has an unregistered root",
                    ));
                }
                Value::Ephemeral(crate::value::EphemeralValue::HostRef(token)) => self
                    .runtime
                    .host_borrows
                    .validate(*token, crate::HostBorrowKind::Shared)?,
                Value::Ephemeral(crate::value::EphemeralValue::HostMut(token)) => self
                    .runtime
                    .host_borrows
                    .validate(*token, crate::HostBorrowKind::Unique)?,
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
                .state
                .host_scopes
                .borrow_mut()
                .remove(&self.state.borrows.frame_id());
        }
    }
}
