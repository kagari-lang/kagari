//! Slot/generation root enumeration; lease upgrades last only for one field visit.
use crate::{
    error::RuntimeError,
    gc::{
        GcHeap,
        graph::objects::{EdgeVisitor, ScanEdge},
        roots::{RootId, lock_releases},
    },
    native::storage::scan::{ScanBudget, ScanStatus},
};

#[derive(Debug)]
pub(crate) struct RootCursor {
    next: usize,
    end: usize,
    releases: usize,
    active: Option<RootId>,
    field: usize,
    values: usize,
    metadata: usize,
}

impl GcHeap {
    pub(crate) fn begin_root_scan(&self) -> Result<RootCursor, RuntimeError> {
        let roots = self.roots.try_borrow().map_err(|_| invalid())?;
        let releases = lock_releases(&roots.released).pending.len();
        Ok(RootCursor {
            next: 0,
            end: roots.slots.len(),
            releases,
            active: None,
            field: 0,
            values: 0,
            metadata: 0,
        })
    }

    pub(crate) fn scan_root_slice(
        &self,
        cursor: &mut RootCursor,
        budget: &mut ScanBudget,
        visit: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, RuntimeError> {
        while cursor.releases != 0 {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            self.roots
                .try_borrow_mut()
                .map_err(|_| invalid())?
                .drain_released_limit(1);
            cursor.releases -= 1;
        }
        while cursor.next < cursor.end {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let roots = self.roots.try_borrow().map_err(|_| invalid())?;
            let slot = &roots.slots[cursor.next];
            let entry = slot
                .entry
                .as_ref()
                .and_then(|entry| entry.lease.upgrade().map(|lease| (entry, lease)));
            let Some((entry, _lease)) = entry else {
                cursor.next += 1;
                cursor.active = None;
                continue;
            };
            let id = RootId {
                owner: roots.owner,
                slot: cursor.next,
                generation: slot.generation,
            };
            if let Some(active) = cursor.active {
                if active != id {
                    // Reuse behind the cursor is protected by publication, not a restart.
                    cursor.next += 1;
                    cursor.active = None;
                    continue;
                }
            } else {
                cursor.active = Some(id);
                cursor.values = entry.values.len();
                cursor.metadata = entry.metadata.len();
                cursor.field = 0;
                continue;
            }
            if cursor.field < cursor.values {
                if let Some(value) = entry.values.get(cursor.field) {
                    visit(ScanEdge::Value(*value))?;
                }
            } else if cursor.field - cursor.values < cursor.metadata {
                if let Some(root) = entry.metadata.get(cursor.field - cursor.values) {
                    visit(ScanEdge::Metadata(root.edge()))?;
                }
            } else {
                cursor.next += 1;
                cursor.active = None;
                continue;
            }
            cursor.field += 1;
        }
        Ok(ScanStatus::Complete)
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("root storage borrowed during scan")
}
