//! Selected executable evidence stored as graph edges, without host root leases.
use crate::{
    Runtime,
    execution_metadata::{MetadataEdge, MetadataRoot},
    module::LoadedModule,
    native::{
        binding::NativeResult,
        context::{CallableOwner, LinkedCallable},
    },
};

#[derive(Debug, Clone)]
pub(crate) struct StoredSelection {
    owner: LoadedModule,
    callable: LinkedCallable,
}

impl StoredSelection {
    pub(crate) fn new(caller: &LoadedModule, selected: &LinkedCallable) -> NativeResult<Self> {
        let owner = selected.owner(caller)?;
        let mut callable = selected.clone();
        // The native object's metadata traversal retains this program and its
        // environment. An internal owning root would leak module/data cycles.
        callable.owner = CallableOwner::Program(owner.slot());
        Ok(Self { owner, callable })
    }

    pub(crate) fn trace<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        pending.push(MetadataEdge::Program(&self.owner));
        if let Some(environment) = &self.callable.environment {
            pending.push(MetadataEdge::Environment(environment.id));
        }
    }

    /// Promote an edge of a currently rooted object to a host-owned call handle.
    pub(crate) fn retain(&self, runtime: &Runtime) -> NativeResult<LinkedCallable> {
        let mut metadata = vec![MetadataRoot::Program(self.owner.clone())];
        metadata.extend(
            self.callable
                .environment
                .iter()
                .map(|environment| MetadataRoot::Environment(environment.id)),
        );
        let roots = runtime.root_metadata(metadata)?;
        let mut callable = self.callable.clone();
        callable.owner = CallableOwner::Pinned(self.owner.clone(), roots);
        Ok(callable)
    }
}
