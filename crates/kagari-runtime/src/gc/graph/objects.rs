//! Resumable heap-owner slices; packed structs use their scoped physical scan owner.
use crate::{
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    gc::{GcHeap, graph::GraphIdentity, reference::HeapObjectIdentity, storage::HeapObject},
    host::HostPathViewHandle,
    native::storage::scan::{ScanBudget, ScanStatus},
    value::Value,
};
use std::sync::Arc;

/// A view exists only during the visitor call; no borrowed edge is queued.
pub(crate) enum ScanEdge<'a> {
    Identity(GraphIdentity),
    Value(Value),
    Metadata(MetadataEdge<'a>),
}

pub(crate) type EdgeVisitor<'a> =
    dyn for<'edge> FnMut(ScanEdge<'edge>) -> Result<(), RuntimeError> + 'a;

/// Collector-only visitor. Pending must leave the enclosing edge cursor in place.
/// Mutator publication uses EdgeVisitor and never borrows a collector budget.
pub(crate) type ScanVisitor<'a> =
    dyn for<'edge> FnMut(ScanEdge<'edge>, &mut ScanBudget) -> Result<ScanStatus, RuntimeError> + 'a;

/// Numeric extents for mutable payloads. The optional host-path Arc pins
/// immutable backing while walking its chain, never serves as graph identity and
/// never owns a heap/root lease. The active collector retains the nonowning identity; no payload loan or borrowed iterator survives a yield.
#[derive(Debug)]
pub(crate) struct ObjectCursor {
    id: HeapObjectIdentity,
    section: u8,
    index: usize,
    end: usize,
    host_path: Option<Arc<HostPathViewHandle>>,
}

impl GcHeap {
    pub(crate) fn begin_object_scan(
        &self,
        id: HeapObjectIdentity,
    ) -> Result<ObjectCursor, RuntimeError> {
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| invalid("borrowed object scan storage"))?;
        let object = self
            .identity_ref(&objects, id)
            .ok_or_else(|| invalid("invalid object scan identity"))?;
        let (end, host_path) = match object {
            HeapObject::Tuple(value) => (value.members.len(), None),
            HeapObject::Enum(value, _) => (value.fields.len(), None),
            HeapObject::Struct { .. } => {
                return Err(invalid("struct requires physical scan owner"));
            }
            HeapObject::Closure { snapshot } => (snapshot.captures.len(), None),
            HeapObject::Native(value) => (value.selected.len(), None),
            HeapObject::HostPath(path) => (0, Some(path.clone())),
            _ => (0, None),
        };
        Ok(ObjectCursor {
            id,
            section: 0,
            index: 0,
            end,
            host_path,
        })
    }

    /// Classify the currently resolved owner, not a cached representation in the
    /// cursor. The driver cannot mutate or call out between this check and scan.
    /// Native providers and HostPath Arc advancement require unlocked control.
    pub(super) fn object_scan_is_internal(
        &self,
        cursor: &ObjectCursor,
    ) -> Result<bool, RuntimeError> {
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| invalid("borrowed object scan storage"))?;
        let object = self
            .identity_ref(&objects, cursor.id)
            .ok_or_else(|| invalid("retired object scan identity"))?;
        Ok(match object {
            HeapObject::Native(_) | HeapObject::HostPath(_) => false,
            HeapObject::Tuple(_)
            | HeapObject::Enum(..)
            | HeapObject::Struct { .. }
            | HeapObject::Closure { .. }
            | HeapObject::Cell { .. }
            | HeapObject::Interface { .. }
            | HeapObject::String(_)
            | HeapObject::Range(_)
            | HeapObject::HostRoot(_)
            | HeapObject::Ephemeral(_) => true,
        })
    }

    /// Publication must shade insertions and moves; this cursor never restarts on
    /// mutation. Native providers capture their own initialized extents at begin.
    pub(crate) fn scan_object_slice(
        &self,
        cursor: &mut ObjectCursor,
        budget: &mut ScanBudget,
        visit: &mut ScanVisitor<'_>,
    ) -> Result<ScanStatus, RuntimeError> {
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| invalid("borrowed object scan storage"))?;
        let object = self
            .identity_ref(&objects, cursor.id)
            .ok_or_else(|| invalid("retired object scan identity"))?;
        loop {
            match object {
                HeapObject::Native(object) => {
                    if cursor.section == 0 {
                        if cursor.index < cursor.end.saturating_mul(2) {
                            if !budget.take(1) {
                                return Ok(ScanStatus::Pending);
                            }
                            let selection = object
                                .selected
                                .get(cursor.index / 2)
                                .ok_or_else(|| invalid("changed native selection extent"))?;
                            if let Some(edge) = selection.scan_field(cursor.index % 2)
                                && visit(ScanEdge::Metadata(edge), budget)? == ScanStatus::Pending
                            {
                                return Ok(ScanStatus::Pending);
                            }
                            cursor.index += 1;
                            continue;
                        }
                        if !budget.take(1) {
                            return Ok(ScanStatus::Pending);
                        }
                        let mut workspace = self
                            .native_scan_workspace
                            .try_borrow_mut()
                            .map_err(|_| invalid("borrowed native scan workspace"))?;
                        object.begin_scan(&mut workspace)?;
                        cursor.section = 1;
                    }
                    let mut workspace = self
                        .native_scan_workspace
                        .try_borrow_mut()
                        .map_err(|_| invalid("borrowed native scan workspace"))?;
                    return object.scan_slice(
                        self,
                        &mut workspace,
                        budget,
                        &mut |value, budget| visit(ScanEdge::Value(*value), budget),
                    );
                }
                HeapObject::HostPath(_) => {
                    let Some(path) = &cursor.host_path else {
                        return Ok(ScanStatus::Complete);
                    };
                    if !budget.take(1) {
                        return Ok(ScanStatus::Pending);
                    }
                    if let Some(argument) = path.dynamic_args().as_slice().get(cursor.index) {
                        if visit(ScanEdge::Value(argument.value), budget)? == ScanStatus::Pending {
                            return Ok(ScanStatus::Pending);
                        }
                        cursor.index += 1;
                    } else {
                        cursor.host_path = path.scan_base();
                        cursor.index = 0;
                    }
                }
                HeapObject::Closure { snapshot } if cursor.section < 2 => {
                    if !budget.take(1) {
                        return Ok(ScanStatus::Pending);
                    }
                    match cursor.section {
                        0 if snapshot.call_route().is_none() => {
                            if visit(
                                ScanEdge::Metadata(MetadataEdge::Program(&snapshot.implementation)),
                                budget,
                            )? == ScanStatus::Pending
                            {
                                return Ok(ScanStatus::Pending);
                            }
                        }
                        1 => {
                            if let Some(environment) = &snapshot.environment
                                && visit(
                                    ScanEdge::Metadata(MetadataEdge::Environment(environment.id)),
                                    budget,
                                )? == ScanStatus::Pending
                            {
                                return Ok(ScanStatus::Pending);
                            }
                        }
                        _ => {}
                    }
                    cursor.section += 1;
                }
                HeapObject::Tuple(_) | HeapObject::Enum(..) | HeapObject::Closure { .. } => {
                    if cursor.index == cursor.end {
                        return Ok(ScanStatus::Complete);
                    }
                    if !budget.take(1) {
                        return Ok(ScanStatus::Pending);
                    }
                    let value = match object {
                        HeapObject::Tuple(value) => value.members.get(cursor.index).copied(),
                        HeapObject::Enum(value, _) => value.fields.get(cursor.index).copied(),
                        HeapObject::Closure { snapshot } => {
                            snapshot.captures.get(cursor.index).copied()
                        }
                        _ => unreachable!("indexed object branch"),
                    }
                    .ok_or_else(|| invalid("invalid indexed object scan field"))?;
                    if visit(ScanEdge::Value(value), budget)? == ScanStatus::Pending {
                        return Ok(ScanStatus::Pending);
                    }
                    cursor.index += 1;
                }
                HeapObject::Cell { value, .. } => {
                    if !budget.take(1) {
                        return Ok(ScanStatus::Pending);
                    }
                    if visit(ScanEdge::Value(*value), budget)? == ScanStatus::Pending {
                        return Ok(ScanStatus::Pending);
                    }
                    return Ok(ScanStatus::Complete);
                }
                HeapObject::Interface { snapshot } => {
                    if !budget.take(1) {
                        return Ok(ScanStatus::Pending);
                    }
                    if visit(
                        ScanEdge::Metadata(MetadataEdge::Interface(snapshot)),
                        budget,
                    )? == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                    return Ok(ScanStatus::Complete);
                }
                HeapObject::String(_)
                | HeapObject::Range(_)
                | HeapObject::HostRoot(_)
                | HeapObject::Ephemeral(_) => {
                    return Ok(if budget.take(1) {
                        ScanStatus::Complete
                    } else {
                        ScanStatus::Pending
                    });
                }
                HeapObject::Struct { .. } => {
                    return Err(invalid("struct requires physical scan owner"));
                }
            }
        }
    }
}

fn invalid(message: &'static str) -> RuntimeError {
    RuntimeError::module_validation(message)
}
