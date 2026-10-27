//! Checked owner lookup is the publication proof; graph control stores no ID registry.
use crate::{
    error::RuntimeError,
    gc::{
        GcHeap,
        graph::{
            GraphIdentity,
            marks::{Domain, MarkRef, WorkGroup},
        },
    },
    module::ModuleStore,
    value::Value,
};

impl GcHeap {
    pub(crate) fn next_group_owner(
        &self,
        modules: &ModuleStore,
        group: WorkGroup,
        epoch: u64,
    ) -> Result<(Option<GraphIdentity>, bool), RuntimeError> {
        macro_rules! metadata {
            ($store:ident) => {
                self.$store
                    .try_borrow()
                    .map_err(|_| invalid())?
                    .next_gray(group.block, epoch)
            };
        }
        macro_rules! typed {
            ($store:ident, $variant:ident) => {{
                let (id, more) = metadata!($store);
                (id.map(GraphIdentity::$variant), more)
            }};
        }
        Ok(match group.domain {
            Domain::Object => {
                let (id, more) = self.objects.try_borrow().map_err(|_| invalid())?.next_gray(
                    self.owner,
                    group.block,
                    epoch,
                );
                (id.map(GraphIdentity::Object), more)
            }
            Domain::Moved => typed!(moved, Moved),
            Domain::Environment => metadata!(environments),
            Domain::Group => metadata!(operation_groups),
            Domain::Application => metadata!(applications),
            Domain::Interface => metadata!(interfaces),
            Domain::NativeApplication => typed!(native_applications, NativeApplication),
            Domain::InterfaceCall => typed!(interface_calls, InterfaceCall),
            Domain::InterfaceDescription => typed!(interface_descriptions, InterfaceDescription),
            Domain::Program => modules.next_group_owner(group.block, epoch)?,
        })
    }

    /// Access does not create retention; only successful mark closure condemns.
    /// Collector/disposal resolution uses with_heap_mark directly instead.
    pub(crate) fn heap_identity_accessible(&self, id: GraphIdentity) -> bool {
        self.with_heap_mark(id, |mark| self.graph.accessible(mark))
            .unwrap_or(false)
    }

    /// Protect an already admitted strong target before its publication. Scalars
    /// have no graph edge; moved identities are collector-only and are rejected.
    #[inline]
    pub(crate) fn publish_value(&self, value: Value) -> Result<(), RuntimeError> {
        let Some(id) = value.object_id() else {
            return Ok(());
        };
        let objects = self.objects.try_borrow().map_err(|_| invalid())?;
        self.validate_heap_value(&objects, &value)
            .ok_or_else(invalid)?;
        self.publish_resolved_value(&objects, id)
    }

    pub(crate) fn publish_heap_identity(&self, id: GraphIdentity) -> Result<(), RuntimeError> {
        self.with_heap_mark(id, |mark| self.graph.shade(id, mark))
    }

    pub(crate) fn with_heap_mark<R>(
        &self,
        id: GraphIdentity,
        action: impl FnOnce(MarkRef<'_>) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        macro_rules! slot {
            ($store:ident, $id:expr) => {{
                let store = self.$store.try_borrow().map_err(|_| invalid())?;
                action(store.mark($id).ok_or_else(invalid)?)
            }};
        }
        match id {
            GraphIdentity::Object(id) => {
                let objects = self.objects.try_borrow().map_err(|_| invalid())?;
                self.object_location(&objects, id).ok_or_else(invalid)?;
                action(objects.mark_for_directory(id.index()))
            }
            GraphIdentity::Moved(id) => {
                let moved = self.moved.try_borrow().map_err(|_| invalid())?;
                action(moved.get(&id).ok_or_else(invalid)?)
            }
            GraphIdentity::Environment(id) => slot!(environments, id),
            GraphIdentity::Group(id) => slot!(operation_groups, id),
            GraphIdentity::Application(id) => slot!(applications, id),
            GraphIdentity::Interface(id) => slot!(interfaces, id),
            GraphIdentity::NativeApplication(id) => slot!(native_applications, id),
            GraphIdentity::InterfaceCall(id) => slot!(interface_calls, id),
            GraphIdentity::InterfaceDescription(id) => slot!(interface_descriptions, id),
            GraphIdentity::Program(_) | GraphIdentity::Static(_) => Err(invalid()),
        }
    }

    pub(crate) fn with_graph_mark<R>(
        &self,
        modules: &ModuleStore,
        id: GraphIdentity,
        action: impl FnOnce(MarkRef<'_>) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        match id {
            GraphIdentity::Program(_) | GraphIdentity::Static(_) => {
                modules.with_graph_mark(id, action)
            }
            _ => self.with_heap_mark(id, action),
        }
    }

    pub(crate) fn shade_graph_identity(
        &self,
        modules: &ModuleStore,
        id: GraphIdentity,
    ) -> Result<(), RuntimeError> {
        self.with_graph_mark(modules, id, |mark| self.graph.shade(id, mark))
    }

    #[cfg(test)]
    pub(crate) fn graph_survivor(
        &self,
        modules: &ModuleStore,
        id: GraphIdentity,
    ) -> Result<bool, RuntimeError> {
        self.with_graph_mark(modules, id, |mark| self.graph.try_lock()?.is_survivor(mark))
    }
}

#[cold]
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid or borrowed graph mark owner")
}
