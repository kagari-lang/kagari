//! Runtime-owned observation. No observer storage is shared with an execution driver.
use std::{
    any::Any,
    cell::{Ref, RefMut},
    mem,
};

use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    session::{ExecutionEvent, ExecutionObserver},
};

impl Runtime {
    /// Replace the exclusively owned observer while no execution session is active.
    /// Its state may be Send without Sync, and moves and drops with this runtime.
    pub fn set_execution_observer(
        &self,
        observer: impl ExecutionObserver,
    ) -> Result<(), RuntimeError> {
        self.replace_execution_observer(Some(Box::new(observer)))
    }

    pub fn clear_execution_observer(&self) -> Result<(), RuntimeError> {
        self.replace_execution_observer(None)
    }

    fn replace_execution_observer(
        &self,
        observer: Option<Box<dyn ExecutionObserver>>,
    ) -> Result<(), RuntimeError> {
        self.resources().ensure_execution_allowed()?;
        if self.resources().active_session().is_some() {
            return Err(RuntimeError::module_validation(
                "cannot replace the observer during an execution session",
            ));
        }
        let previous = {
            let mut slot = self
                .observer
                .try_borrow_mut()
                .map_err(|_| RuntimeError::module_validation("execution observer is borrowed"))?;
            mem::replace(&mut *slot, observer)
        };
        drop(previous);
        Ok(())
    }

    /// Borrow the installed observer's concrete state without sharing ownership.
    pub fn execution_observer<T: ExecutionObserver>(&self) -> Option<Ref<'_, T>> {
        Ref::filter_map(self.observer.try_borrow().ok()?, |observer| {
            (observer.as_deref()? as &dyn Any).downcast_ref()
        })
        .ok()
    }

    pub fn execution_observer_mut<T: ExecutionObserver>(&mut self) -> Option<RefMut<'_, T>> {
        RefMut::filter_map(self.observer.try_borrow_mut().ok()?, |observer| {
            (observer.as_deref_mut()? as &mut dyn Any).downcast_mut()
        })
        .ok()
    }

    /// Activate the installed observer once for the root. Nested drivers inherit it.
    pub fn attach_execution_observer(&self) -> Result<bool, RuntimeError> {
        self.resources().ensure_execution_allowed()?;
        let session = self.resources().active_session().ok_or_else(|| {
            RuntimeError::module_validation("execution observer requires an active session")
        })?;
        if session.observer_attached.get() {
            return Ok(false);
        }
        let frames = self
            .resources()
            .sessions
            .frames(session.id)
            .ok_or_else(|| {
                self.resources()
                    .quarantine("observer installation encountered a borrowed stack")
            })?;
        if !frames.is_empty() {
            return Err(RuntimeError::module_validation(
                "cannot attach an observer during frame execution",
            ));
        }
        let mut observer = self
            .observer
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("execution observer is borrowed"))?;
        let Some(observer) = observer.as_mut() else {
            return Ok(false);
        };
        let root = session.root.clone();
        drop(session);
        observer.begin(self, &root)?;
        drop(frames);
        let session = self.resources().active_session().ok_or_else(|| {
            self.resources()
                .quarantine("observer initialization lost its session")
        })?;
        session.observer_attached.set(true);
        Ok(true)
    }

    pub fn observe_execution(&self, event: ExecutionEvent) -> Result<(), RuntimeError> {
        let Some(session) = self.resources().active_session() else {
            return Ok(());
        };
        if !session.observer_attached.get() {
            return Ok(());
        }
        let id = session.id;
        drop(session);
        let frames = self.resources().sessions.frames(id).ok_or_else(|| {
            self.resources()
                .quarantine("observer encountered a borrowed execution stack")
        })?;
        let mut observer = self.observer.try_borrow_mut().map_err(|_| {
            self.resources()
                .quarantine("execution observer borrowed across execution")
        })?;
        let observer = observer.as_mut().ok_or_else(|| {
            self.resources()
                .quarantine("active execution observer is missing")
        })?;
        let result = observer.observe(self, event, &frames);
        if result
            .as_ref()
            .is_err_and(|error| error.kind() == RuntimeErrorKind::EngineFault)
        {
            self.resources()
                .quarantine("execution observer encountered an engine fault");
        }
        result
    }
}
