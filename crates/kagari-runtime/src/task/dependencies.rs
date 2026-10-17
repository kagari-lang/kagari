//! Bounded wait edges own wake registrations, never script values or executions.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::binding::NativeResult,
    session::store::SessionId,
    task::TaskId,
};
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
    task::Waker,
};

#[derive(Debug)]
struct Edge {
    target: TaskId,
    serial: u64,
    wake: Waker,
}

#[derive(Debug, Default)]
struct State {
    sessions: HashMap<TaskId, SessionId>,
    edges: HashMap<SessionId, Edge>,
    serial: u64,
}

#[derive(Debug)]
pub(crate) struct Dependencies {
    state: Mutex<State>,
    limit: usize,
}

#[derive(Debug)]
pub(crate) struct WaitLease {
    registry: Arc<Dependencies>,
    session: SessionId,
    serial: u64,
}

impl Drop for WaitLease {
    fn drop(&mut self) {
        let discarded = {
            let mut state = self
                .registry
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if state
                .edges
                .get(&self.session)
                .is_some_and(|edge| edge.serial == self.serial)
            {
                state.edges.remove(&self.session)
            } else {
                None
            }
        };
        drop(discarded);
    }
}

impl Dependencies {
    pub fn new(limit: NonZeroUsize) -> Self {
        Self {
            state: Mutex::new(State::default()),
            limit: limit.get(),
        }
    }

    pub fn attach(&self, task: TaskId, session: SessionId) -> NativeResult<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.sessions.contains_key(&task) {
            return Err(RuntimeError::module_validation(
                "Task already has an execution",
            ));
        }
        state
            .sessions
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("task execution registrations"))?;
        state.sessions.insert(task, session);
        Ok(())
    }

    pub fn register(
        self: &Arc<Self>,
        session: SessionId,
        target: TaskId,
        wake: Waker,
    ) -> NativeResult<WaitLease> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.edges.contains_key(&session) {
            return Err(RuntimeError::module_validation(
                "execution already waits on a Task",
            ));
        }
        let mut next = target;
        let mut remaining = state.sessions.len();
        while let Some(target_session) = state.sessions.get(&next) {
            if remaining == 0 {
                return Err(RuntimeError::module_validation(
                    "cyclic Task dependency registry",
                ));
            }
            remaining -= 1;
            if *target_session == session {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "Task wait cycle",
                ));
            }
            let Some(edge) = state.edges.get(target_session) else {
                break;
            };
            next = edge.target;
        }
        if state.edges.len() >= self.limit {
            return Err(RuntimeError::resource_limit("task waiters"));
        }
        let serial = state
            .serial
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("task waiter identities"))?;
        state
            .edges
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("task waiters"))?;
        state.serial = serial;
        state.edges.insert(
            session,
            Edge {
                target,
                serial,
                wake,
            },
        );
        Ok(WaitLease {
            registry: self.clone(),
            session,
            serial,
        })
    }

    /// Detach before invoking any wake. Leases left in parked waiters become
    /// inert, and a later registration in the same session gets a new serial.
    pub fn complete(&self, task: TaskId) -> Vec<Waker> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let outgoing = state
            .sessions
            .remove(&task)
            .and_then(|session| state.edges.remove(&session));
        let wakes = state
            .edges
            .extract_if(|_, edge| edge.target == task)
            .map(|(_, edge)| edge.wake)
            .collect();
        drop(state);
        drop(outgoing);
        wakes
    }
}
