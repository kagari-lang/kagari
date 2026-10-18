//! Detect same-bundle recursion before OnceLock can block its initializer thread.
use kagari_runtime::{error::RuntimeError, native::binding::NativeResult};
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Collection,
    Preparation,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Collection => "collection",
            Self::Preparation => "preparation",
        }
    }
}

struct Active {
    // This address identifies only a currently borrowed cache, never a declaration.
    key: *const (),
    phase: Phase,
    reentry: Option<RuntimeError>,
}

thread_local! {
    static ACTIVE: RefCell<Vec<Active>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn check_reentry(key: *const (), name: &str, requested: Phase) -> NativeResult<()> {
    ACTIVE.with_borrow_mut(|active| {
        let Some(initializer) = active.iter_mut().find(|initializer| {
            initializer.key == key
                && (initializer.phase == Phase::Collection || requested == Phase::Preparation)
        }) else {
            return Ok(());
        };
        let error = initializer.reentry.get_or_insert_with(|| {
            RuntimeError::metadata_conflict(format!(
                "stdlib {name} {}: recursive bundle initialization while requesting {}",
                initializer.phase.name(),
                requested.name(),
            ))
        });
        Err(error.clone())
    })
}

pub(super) fn run<T>(
    key: *const (),
    name: &str,
    phase: Phase,
    initialize: impl FnOnce() -> NativeResult<T>,
) -> NativeResult<T> {
    let guard = Guard::enter(key, phase);
    // The candidate is owned by this attempt, discarded on failure and never
    // retried. No mutex guard crosses this unwind boundary. AssertUnwindSafe does
    // not promise rollback of effects performed by a recipe or native factory.
    let result = catch_unwind(AssertUnwindSafe(initialize)).unwrap_or_else(|_| {
        Err(RuntimeError::metadata_conflict(format!(
            "stdlib {name} {}: initializer panicked; retry disabled",
            phase.name(),
        )))
    });
    // A factory cannot turn recursive initialization into a cached success merely
    // by swallowing the error. Work it performs after swallowing it is not undone.
    if let Some(error) = guard.reentry() {
        return Err(error);
    }
    result
}

struct Guard;

impl Guard {
    fn enter(key: *const (), phase: Phase) -> Self {
        ACTIVE.with_borrow_mut(|active| {
            active.push(Active {
                key,
                phase,
                reentry: None,
            });
        });
        Self
    }

    fn reentry(&self) -> Option<RuntimeError> {
        ACTIVE.with_borrow(|active| active.last().and_then(|active| active.reentry.clone()))
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        ACTIVE.with_borrow_mut(|active| {
            active.pop();
        });
    }
}
