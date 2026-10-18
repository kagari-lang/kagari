//! One stdlib bundle owns semantic publication and one-shot native preparation.
//!
//! Module recipes run once, on the first caller's thread. Only their explicitly
//! Send pending associations cross to the thread that first requests modules.
//! Declaration consumers retain `Semantic`, never the bundle or its pending work.
//! Factories consume the supplied sealed preparation view, not global declaration
//! entrypoints. They must not spawn and join work that initializes the same bundle.
//!
//! Production recipes and remaining raw seeds share one seal and this cache.
//! Declaration-only consumers never prepare adapters. SR05 removes the remaining
//! raw seed bridge and symbolic enum caches; no separate declaration producer runs.
mod composition;
mod initialization;
pub(crate) mod semantic;

#[cfg(test)]
mod tests;

use crate::{
    assembly::{
        initialization::{Phase, check_reentry, run},
        semantic::Semantic,
    },
    bindings,
};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        assembly::{ModuleAssembly, SendModuleAssembly},
        binding::NativeResult,
        module::NativeModule,
    },
};
use std::{
    ptr,
    sync::{Arc, Mutex, OnceLock},
};

/// A directly instantiable cache for one fixed set of stdlib contributors.
///
/// `F` is consumed once to author and seal a complete module set. A global bundle
/// requires `F: Send`, not `F: Sync`: its ownership is protected by the recipe
/// mutex. Recipes may create local non-Send authoring state; all retained pending
/// associations must pass runtime's `try_into_send` check before publication.
///
/// Both ordinary errors and unwinding panics are terminal cached failures. Panics
/// become phase-labelled metadata errors; no partial candidate is reused or retried.
/// Abort and double-panic process termination cannot be recovered. Native factory
/// side effects are not rolled back. Preparation never installs a runtime.
pub struct Bundle<F> {
    name: &'static str,
    recipe: Mutex<Option<F>>,
    collected: OnceLock<NativeResult<Collected>>,
    prepared: OnceLock<NativeResult<Arc<[NativeModule]>>>,
}

struct Collected {
    semantic: Arc<Semantic>,
    pending: Mutex<Option<SendModuleAssembly>>,
}

impl<F> Bundle<F> {
    pub const fn new(name: &'static str, recipe: F) -> Self {
        Self {
            name,
            recipe: Mutex::new(Some(recipe)),
            collected: OnceLock::new(),
            prepared: OnceLock::new(),
        }
    }
}

impl<F: FnOnce() -> NativeResult<ModuleAssembly>> Bundle<F> {
    /// Retain only immutable semantic records and symbolic enum handles.
    /// This never prepares native entries and remains usable after preparation fails.
    pub fn semantic(&self) -> NativeResult<Arc<Semantic>> {
        Ok(self.collect()?.semantic.clone())
    }

    /// Consume pending factories once and share either complete modules or failure.
    /// Concurrent callers wait for the same result; recursive initialization rejects
    /// before entering OnceLock. No recipe/pending mutex spans factory execution.
    pub fn modules(&self) -> NativeResult<Arc<[NativeModule]>> {
        let key = ptr::from_ref(self).cast();
        check_reentry(key, self.name, Phase::Preparation)?;
        let collected = self.collect()?;
        self.prepared
            .get_or_init(|| {
                run(key, self.name, Phase::Preparation, || {
                    let pending = take(&collected.pending, self.name, "pending associations")?;
                    pending.into_modules().map(Arc::from)
                })
            })
            .clone()
    }

    fn collect(&self) -> NativeResult<&Collected> {
        let key = ptr::from_ref(self).cast();
        check_reentry(key, self.name, Phase::Collection)?;
        self.collected
            .get_or_init(|| {
                run(key, self.name, Phase::Collection, || {
                    let recipe = take(&self.recipe, self.name, "recipe")?;
                    let pending = recipe()?.try_into_send()?;
                    let semantic = Arc::new(Semantic::new(pending.declarations())?);
                    Ok(Collected {
                        semantic,
                        pending: Mutex::new(Some(pending)),
                    })
                })
            })
            .as_ref()
            .map_err(Clone::clone)
    }
}

static STANDARD: Bundle<fn() -> NativeResult<ModuleAssembly>> =
    Bundle::new("standard", composition::collect);

pub(crate) fn semantic() -> NativeResult<Arc<Semantic>> {
    STANDARD.semantic()
}

pub(crate) fn modules() -> NativeResult<Vec<NativeModule>> {
    bindings::initialize(semantic()?.as_ref())?;
    STANDARD.modules().map(|modules| modules.to_vec())
}

fn take<T>(slot: &Mutex<Option<T>>, bundle: &str, item: &str) -> NativeResult<T> {
    slot.lock()
        .map_err(|_| RuntimeError::metadata_conflict(format!("stdlib {bundle}: poisoned {item}")))?
        .take()
        .ok_or_else(|| {
            RuntimeError::metadata_conflict(format!("stdlib {bundle}: already consumed {item}"))
        })
}
