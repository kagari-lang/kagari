//! Owned metadata cursors over admitted records; inline views never become jobs.
use crate::{
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    gc::{
        GcHeap,
        graph::{
            GraphIdentity,
            objects::{EdgeVisitor, ScanEdge},
        },
    },
    native::storage::scan::{ScanBudget, ScanStatus},
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct MetadataCursor {
    identity: GraphIdentity,
    next: usize,
    end: usize,
}

impl GcHeap {
    /// Resolve borrowed validation views while their owner is borrowed. Candidate
    /// and inline variants must be scanned by an enclosing owner, not queued.
    pub(crate) fn metadata_scan_identity(
        &self,
        edge: MetadataEdge<'_>,
    ) -> Result<GraphIdentity, RuntimeError> {
        let id = match edge {
            MetadataEdge::NativeApplication(value) => {
                self.native_applications
                    .try_borrow()
                    .map_err(|_| invalid())?
                    .get(value.scan_id)
                    .ok_or_else(invalid)?;
                GraphIdentity::NativeApplication(value.scan_id)
            }
            MetadataEdge::Environment(id) => {
                self.environments
                    .try_borrow()
                    .map_err(|_| invalid())?
                    .get(id)
                    .ok_or_else(invalid)?;
                GraphIdentity::Environment(id)
            }
            MetadataEdge::Operation(id) => {
                self.operation_groups
                    .try_borrow()
                    .map_err(|_| invalid())?
                    .operation(id)
                    .ok_or_else(invalid)?;
                GraphIdentity::Group(id.group)
            }
            MetadataEdge::Group(id) => {
                self.operation_groups
                    .try_borrow()
                    .map_err(|_| invalid())?
                    .get(id)
                    .ok_or_else(invalid)?;
                GraphIdentity::Group(id)
            }
            MetadataEdge::Application(id) => {
                self.applications
                    .try_borrow()
                    .map_err(|_| invalid())?
                    .record(id)
                    .ok_or_else(invalid)?;
                GraphIdentity::Application(id)
            }
            MetadataEdge::Interface(id) => {
                self.interfaces
                    .try_borrow()
                    .map_err(|_| invalid())?
                    .get(id)
                    .ok_or_else(invalid)?;
                GraphIdentity::Interface(id)
            }
            MetadataEdge::InterfaceCall(value) => {
                self.interface_calls
                    .try_borrow()
                    .map_err(|_| invalid())?
                    .get(value.scan_id)
                    .ok_or_else(invalid)?;
                GraphIdentity::InterfaceCall(value.scan_id)
            }
            MetadataEdge::InterfaceDescription(value) => {
                self.interface_descriptions
                    .try_borrow()
                    .map_err(|_| invalid())?
                    .get(value.scan_id)
                    .ok_or_else(invalid)?;
                GraphIdentity::InterfaceDescription(value.scan_id)
            }
            MetadataEdge::Program(_) => {
                return Err(RuntimeError::module_validation(
                    "program scan identity requires ModuleStore",
                ));
            }
            MetadataEdge::EnvironmentView(_)
            | MetadataEdge::InterfaceView(_)
            | MetadataEdge::Binding(_)
            | MetadataEdge::Closure(_)
            | MetadataEdge::Selections(_)
            | MetadataEdge::Operations(_) => {
                return Err(RuntimeError::module_validation(
                    "inline metadata requires its owning scan cursor",
                ));
            }
        };
        Ok(id)
    }

    pub(crate) fn begin_metadata_scan(
        &self,
        identity: GraphIdentity,
    ) -> Result<MetadataCursor, RuntimeError> {
        let end = match identity {
            GraphIdentity::Environment(id) => self
                .environments
                .try_borrow()
                .map_err(|_| invalid())?
                .get(id)
                .ok_or_else(invalid)?
                .scan_len(),
            GraphIdentity::Group(id) => self
                .operation_groups
                .try_borrow()
                .map_err(|_| invalid())?
                .get(id)
                .ok_or_else(invalid)?
                .scan_len(),
            GraphIdentity::Application(id) => {
                self.applications
                    .try_borrow()
                    .map_err(|_| invalid())?
                    .record(id)
                    .ok_or_else(invalid)?;
                3
            }
            GraphIdentity::Interface(id) => {
                2 + self
                    .interfaces
                    .try_borrow()
                    .map_err(|_| invalid())?
                    .get(id)
                    .ok_or_else(invalid)?
                    .parent_views
                    .len()
            }
            GraphIdentity::NativeApplication(id) => self
                .native_applications
                .try_borrow()
                .map_err(|_| invalid())?
                .get(id)
                .ok_or_else(invalid)?
                .scan_len(),
            GraphIdentity::InterfaceCall(id) => self
                .interface_calls
                .try_borrow()
                .map_err(|_| invalid())?
                .get(id)
                .ok_or_else(invalid)?
                .scan_len(),
            GraphIdentity::InterfaceDescription(id) => self
                .interface_descriptions
                .try_borrow()
                .map_err(|_| invalid())?
                .get(id)
                .ok_or_else(invalid)?
                .scan_len(),
            GraphIdentity::Object(_)
            | GraphIdentity::Moved(_)
            | GraphIdentity::Program(_)
            | GraphIdentity::Static(_) => return Err(invalid()),
        };
        Ok(MetadataCursor {
            identity,
            next: 0,
            end,
        })
    }

    pub(crate) fn scan_metadata_slice(
        &self,
        cursor: &mut MetadataCursor,
        budget: &mut ScanBudget,
        visit: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, RuntimeError> {
        while cursor.next < cursor.end {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            self.scan_metadata_field(cursor.identity, cursor.next, visit)?;
            cursor.next += 1;
        }
        Ok(ScanStatus::Complete)
    }

    fn scan_metadata_field(
        &self,
        identity: GraphIdentity,
        field: usize,
        visit: &mut EdgeVisitor<'_>,
    ) -> Result<(), RuntimeError> {
        // Each borrow ends before the next call. The visitor consumes only this
        // field and shades checked IDs; it cannot retain the borrowed view.
        macro_rules! field {
            ($store:ident, $id:expr, $get:ident) => {{
                let store = self.$store.try_borrow().map_err(|_| invalid())?;
                let record = store.$get($id).ok_or_else(invalid)?;
                if let Some(edge) = record.scan_field(field) {
                    visit(ScanEdge::Metadata(edge))?;
                }
            }};
        }
        match identity {
            GraphIdentity::Environment(id) => field!(environments, id, get),
            GraphIdentity::Group(id) => field!(operation_groups, id, get),
            GraphIdentity::Application(id) => field!(applications, id, record),
            GraphIdentity::NativeApplication(id) => field!(native_applications, id, get),
            GraphIdentity::InterfaceCall(id) => field!(interface_calls, id, get),
            GraphIdentity::InterfaceDescription(id) => field!(interface_descriptions, id, get),
            GraphIdentity::Interface(id) => {
                let store = self.interfaces.try_borrow().map_err(|_| invalid())?;
                let record = store.get(id).ok_or_else(invalid)?;
                match field {
                    0 => visit(ScanEdge::Value(record.data))?,
                    1 => visit(ScanEdge::Metadata(MetadataEdge::InterfaceDescription(
                        &record.description,
                    )))?,
                    _ => {
                        let parent = record.parent_views.get(field - 2).ok_or_else(invalid)?;
                        if let Some(id) = parent.get() {
                            visit(ScanEdge::Metadata(MetadataEdge::Interface(*id)))?;
                        }
                    }
                }
            }
            GraphIdentity::Object(_)
            | GraphIdentity::Moved(_)
            | GraphIdentity::Program(_)
            | GraphIdentity::Static(_) => return Err(invalid()),
        }
        Ok(())
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid or borrowed metadata scan owner")
}
