//! Dirty-record roots use a captured prefix and clear revision, never a snapshot.
use crate::{
    error::RuntimeError,
    gc::{
        GcHeap,
        graph::objects::{ScanEdge, ScanVisitor},
    },
    host::{HostPathMutationRecord, HostPathViewHandle, HostRegistry},
    native::storage::scan::{ScanBudget, ScanStatus},
};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct HostRootCursor {
    revision: u64,
    next: usize,
    end: usize,
    field: usize,
    path: Option<Arc<HostPathViewHandle>>,
    path_field: usize,
}

impl HostRegistry {
    pub(crate) fn begin_root_scan(&self) -> Result<HostRootCursor, RuntimeError> {
        let revision = self.dirty_revision.get();
        if revision == u64::MAX {
            return Err(invalid());
        }
        let end = self.dirty_paths.try_borrow().map_err(|_| invalid())?.len();
        Ok(HostRootCursor {
            revision,
            next: 0,
            end,
            field: 0,
            path: None,
            path_field: 0,
        })
    }

    pub(crate) fn scan_root_slice(
        &self,
        cursor: &mut HostRootCursor,
        budget: &mut ScanBudget,
        visit: &mut ScanVisitor<'_>,
    ) -> Result<ScanStatus, RuntimeError> {
        if cursor.revision != self.dirty_revision.get() {
            cursor.path = None;
            return Ok(ScanStatus::Complete);
        }
        while cursor.next < cursor.end {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let records = self.dirty_paths.try_borrow().map_err(|_| invalid())?;
            let record = records.get(cursor.next).ok_or_else(invalid)?;
            match cursor.field {
                0 => {
                    if let Some(value) = record.old_value
                        && visit(ScanEdge::Value(value), budget)? == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                }
                1 => {
                    if visit(ScanEdge::Value(record.new_value), budget)? == ScanStatus::Pending {
                        return Ok(ScanStatus::Pending);
                    }
                }
                field if field - 2 < record.dynamic_args.len() => {
                    if visit(
                        ScanEdge::Value(record.dynamic_args.as_slice()[field - 2].value),
                        budget,
                    )? == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                }
                field if field - 2 == record.dynamic_args.len() => {
                    cursor.path = record.base_view.clone();
                    cursor.path_field = 0;
                }
                _ => {
                    if let Some(path) = &cursor.path {
                        if let Some(argument) =
                            path.dynamic_args().as_slice().get(cursor.path_field)
                        {
                            if visit(ScanEdge::Value(argument.value), budget)?
                                == ScanStatus::Pending
                            {
                                return Ok(ScanStatus::Pending);
                            }
                            cursor.path_field += 1;
                        } else {
                            cursor.path = path.scan_base();
                            cursor.path_field = 0;
                        }
                        continue;
                    }
                    cursor.next += 1;
                    cursor.field = 0;
                    continue;
                }
            }
            cursor.field += 1;
        }
        Ok(ScanStatus::Complete)
    }
}

impl HostPathMutationRecord {
    /// Must finish before prepared host state is irreversibly committed.
    pub(super) fn publish_edges(&self, heap: &GcHeap) -> Result<(), RuntimeError> {
        if let Some(value) = self.old_value {
            heap.publish_value(value)?;
        }
        heap.publish_value(self.new_value)?;
        for argument in self.dynamic_args.as_slice() {
            heap.publish_value(argument.value)?;
        }
        if let Some(path) = &self.base_view {
            for value in path.retained_values() {
                heap.publish_value(*value)?;
            }
        }
        Ok(())
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid dirty-root revision or borrowed storage")
}
