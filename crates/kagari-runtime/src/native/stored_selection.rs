//! Selected executable evidence stored as graph edges, without host root leases.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        context::{CallableOwner, LinkedCallable, LinkedOperation},
        conversion::{KagariType, arguments::KagariArguments},
        function_handle::Function,
    },
};

use kagari_contract::standard::RuntimePrimitive;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct StoredSelection {
    owner: ExecutableModule,
    callable: LinkedCallable,
}

impl StoredSelection {
    pub(crate) fn primitive(&self) -> Option<RuntimePrimitive> {
        // This evidence comes from a verified builtin protocol adapter. Generated
        // adapters use Script targets, but their dispatch routes are body-owned;
        // ordinary reloadable script implementations never receive this evidence.
        self.callable.primitive
    }

    pub(crate) fn slots(
        owner: &ExecutableModule,
        selected: &[LinkedOperation],
        slots: &[usize],
    ) -> NativeResult<Arc<[Self]>> {
        if slots.is_empty() {
            return Ok(Arc::default());
        }
        slots
            .iter()
            .map(|slot| {
                Self::new(
                    owner,
                    selected
                        .get(*slot)
                        .and_then(LinkedOperation::ready)
                        .ok_or_else(|| RuntimeError::module_validation("selected callable slot"))?,
                )
            })
            .collect()
    }

    pub(crate) fn new(caller: &ExecutableModule, selected: &LinkedCallable) -> NativeResult<Self> {
        let owner = selected.owner(caller)?;
        let mut callable = selected.clone();
        // The native object's metadata traversal retains this program and its
        // environment. An internal owning root would leak module/data cycles.
        callable.owner = CallableOwner::Program(owner.slot());
        Ok(Self { owner, callable })
    }

    pub(crate) fn function<A: KagariArguments, R: KagariType>(
        &self,
        runtime: &Runtime,
    ) -> NativeResult<Function<A, R>> {
        Function::from_selected(runtime, &self.owner, &self.callable)
    }

    pub(crate) fn scan_field(&self, field: usize) -> Option<MetadataEdge<'_>> {
        self.callable.scan_field(&self.owner, field)
    }

    pub(crate) fn trace<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        self.callable.trace(&self.owner, pending);
    }

    /// Select the next computation from a currently rooted object's checked edge.
    /// The object retains relationship evidence, not an ordinary callback's body.
    pub(crate) fn retain(&self, runtime: &Runtime) -> NativeResult<LinkedCallable> {
        self.callable.select_current(runtime, &self.owner)
    }
}
