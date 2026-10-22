//! Bounded Task waits and structured parent edges share one dependency graph.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::binding::NativeResult,
    resource::AsyncLimits,
    session::store::SessionId,
    task::TaskId,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    task::Waker,
};

#[derive(Debug)]
struct Edge {
    target: TaskId,
    serial: u64,
    wake: Waker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Node {
    Session(SessionId),
    Task(TaskId),
}

#[derive(Debug, Default)]
struct State {
    sessions: HashMap<TaskId, SessionId>,
    edges: HashMap<SessionId, Edge>,
    groups: HashMap<SessionId, [SessionId; 2]>,
    serial: u64,
}

impl State {
    fn check_edge(&self, from: Node, to: Node) -> NativeResult<()> {
        let bound = self
            .sessions
            .len()
            .saturating_mul(2)
            .saturating_add(self.edges.len().saturating_mul(2))
            .saturating_add(self.groups.len().saturating_mul(3))
            .saturating_add(2);
        let mut pending = Vec::new();
        let mut visited = Vec::new();
        pending
            .try_reserve(bound)
            .map_err(|_| RuntimeError::resource_limit("dependency traversal"))?;
        visited
            .try_reserve(bound)
            .map_err(|_| RuntimeError::resource_limit("dependency traversal"))?;
        pending.push(to);
        while let Some(node) = pending.pop() {
            if node == from {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "Task wait cycle",
                ));
            }
            if visited.contains(&node) {
                continue;
            }
            if visited.len() == bound {
                return Err(RuntimeError::resource_limit("dependency traversal"));
            }
            visited.push(node);
            match node {
                Node::Task(task) => {
                    if let Some(session) = self.sessions.get(&task) {
                        pending.push(Node::Session(*session));
                    }
                }
                Node::Session(session) => {
                    if let Some(edge) = self.edges.get(&session) {
                        pending.push(Node::Task(edge.target));
                    }
                    if let Some(children) = self.groups.get(&session) {
                        pending.extend(children.iter().copied().map(Node::Session));
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct Dependencies {
    state: Mutex<State>,
    limits: AsyncLimits,
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
    pub fn new(limits: AsyncLimits) -> Self {
        Self {
            state: Mutex::new(State::default()),
            limits,
        }
    }

    pub fn attach(&self, task: TaskId, session: SessionId) -> NativeResult<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.sessions.contains_key(&task) {
            return Err(RuntimeError::module_validation(
                "Task already has an execution",
            ));
        }
        state.check_edge(Node::Task(task), Node::Session(session))?;
        state
            .sessions
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("task execution registrations"))?;
        state.sessions.insert(task, session);
        Ok(())
    }

    /// Install both parent edges before claims; the caller removes them on rollback.
    pub fn attach_group(&self, parent: SessionId, children: [SessionId; 2]) -> NativeResult<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.groups.contains_key(&parent) || state.edges.contains_key(&parent) {
            return Err(RuntimeError::module_validation(
                "execution already has a dependency",
            ));
        }
        if state.groups.len() >= self.limits.max_groups.get() {
            return Err(RuntimeError::resource_limit("group dependencies"));
        }
        for child in children {
            state.check_edge(Node::Session(parent), Node::Session(child))?;
        }
        state
            .groups
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("group dependencies"))?;
        state.groups.insert(parent, children);
        Ok(())
    }

    pub fn detach_group(&self, parent: SessionId) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .groups
            .remove(&parent);
    }

    pub fn register(
        self: &Arc<Self>,
        session: SessionId,
        target: TaskId,
        wake: Waker,
    ) -> NativeResult<WaitLease> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.edges.contains_key(&session) || state.groups.contains_key(&session) {
            return Err(RuntimeError::module_validation(
                "execution already has a dependency",
            ));
        }
        state.check_edge(Node::Session(session), Node::Task(target))?;
        if state.edges.len() >= self.limits.max_task_waiters.get() {
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

    /// Remove registrations before waking; stale leases cannot remove later waits.
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
