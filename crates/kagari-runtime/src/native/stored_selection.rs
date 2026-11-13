//! Selected executable evidence stored as graph edges, without host root leases.
use crate::{
    Runtime,
    execution::roots::SessionId,
    execution_metadata::{MetadataEdge, managed::sources::MetadataSource},
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        context::{CallSelection, CallableOwner, LinkedCallable},
    },
};

#[derive(Debug, Clone)]
pub(crate) struct StoredSelection {
    owner: ExecutableModule,
    callable: LinkedCallable,
}

impl StoredSelection {
    pub(crate) fn owner(&self) -> &ExecutableModule {
        &self.owner
    }

    pub(crate) fn new(caller: &ExecutableModule, selected: &LinkedCallable) -> NativeResult<Self> {
        let owner = selected.owner(caller)?;
        let mut callable = selected.clone();
        // The native object's metadata traversal retains this program and its
        // environment. An internal owning root would leak module/data cycles.
        callable.owner = CallableOwner::Program(owner.slot());
        Ok(Self { owner, callable })
    }

    pub(crate) fn trace<'a, E, F>(&'a self, visit: &mut F) -> Result<(), E>
    where
        F: FnMut(MetadataEdge<'a>) -> Result<(), E> + ?Sized,
    {
        self.callable.trace(&self.owner, visit)
    }

    /// Select the next computation from a currently rooted object's checked edge.
    /// The object retains relationship evidence, not an ordinary callback's body.
    pub(crate) fn retain(
        &self,
        runtime: &mut Runtime,
        execution: SessionId,
        protected: &[MetadataSource<'_>],
    ) -> NativeResult<CallSelection> {
        self.callable
            .select_current(runtime, &self.owner, execution, protected)
    }
}
