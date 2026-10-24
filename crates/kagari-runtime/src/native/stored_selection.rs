//! Selected executable evidence stored as graph edges, without host root leases.
use crate::{
    Runtime,
    execution_metadata::MetadataEdge,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        context::{CallableOwner, LinkedCallable},
    },
};

#[derive(Debug, Clone)]
pub(crate) struct StoredSelection {
    owner: ExecutableModule,
    callable: LinkedCallable,
}

impl StoredSelection {
    pub(crate) fn new(caller: &ExecutableModule, selected: &LinkedCallable) -> NativeResult<Self> {
        let owner = selected.owner(caller)?;
        let mut callable = selected.clone();
        // The native object's metadata traversal retains this program and its
        // environment. An internal owning root would leak module/data cycles.
        callable.owner = CallableOwner::Program(owner.slot());
        Ok(Self { owner, callable })
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
