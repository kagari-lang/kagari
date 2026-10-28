//! Full window identities and relative ordinals survive bank compaction.
use crate::{
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    frame::values::{ExecutionValues, FrameSlots},
    gc::graph::objects::{ScanEdge, ScanVisitor},
    native::storage::scan::{ScanBudget, ScanStatus},
};

#[derive(Debug)]
pub(crate) struct FrameCursor {
    next: usize,
    end: usize,
    active: Option<FrameSlots>,
    field: usize,
    values: usize,
}

impl ExecutionValues {
    pub(crate) fn begin_root_scan(&self) -> FrameCursor {
        FrameCursor {
            next: 0,
            end: self.windows.len(),
            active: None,
            field: 0,
            values: 0,
        }
    }

    pub(crate) fn scan_root_slice(
        &self,
        cursor: &mut FrameCursor,
        budget: &mut ScanBudget,
        visit: &mut ScanVisitor<'_>,
    ) -> Result<ScanStatus, RuntimeError> {
        while cursor.next < cursor.end {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let Some(window) = self.windows.get(cursor.next) else {
                cursor.next += 1;
                cursor.active = None;
                continue;
            };
            let id = FrameSlots {
                owner: self.owner,
                index: cursor.next,
                generation: window.generation,
            };
            if let Some(active) = cursor.active {
                if active != id {
                    cursor.next += 1;
                    cursor.active = None;
                    continue;
                }
            } else {
                cursor.active = Some(id);
                cursor.values = window.ranges.managed.len();
                cursor.field = 0;
                continue;
            }
            match cursor.field {
                0 => {
                    if visit(
                        ScanEdge::Metadata(MetadataEdge::Program(&window.program)),
                        budget,
                    )? == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                }
                1 => {
                    if let Some(environment) = &window.environment
                        && visit(
                            ScanEdge::Metadata(MetadataEdge::Environment(environment.id)),
                            budget,
                        )? == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                }
                2 => {
                    if let Some(application) = &window.native_application
                        && visit(
                            ScanEdge::Metadata(MetadataEdge::NativeApplication(application)),
                            budget,
                        )? == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                }
                3..=5 => {
                    if let Some(root) = window
                        .invocation
                        .and_then(|invocation| invocation.metadata_field(cursor.field - 3))
                        && visit(ScanEdge::Metadata(root.edge()), budget)? == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                }
                field if field - 6 < cursor.values => {
                    let index = window.ranges.managed.start + field - 6;
                    let value = self.values.get(index).ok_or_else(|| {
                        RuntimeError::module_validation("invalid frame scan range")
                    })?;
                    if visit(ScanEdge::Value(*value), budget)? == ScanStatus::Pending {
                        return Ok(ScanStatus::Pending);
                    }
                }
                _ => {
                    cursor.next += 1;
                    cursor.active = None;
                    continue;
                }
            }
            cursor.field += 1;
        }
        Ok(ScanStatus::Complete)
    }
}
