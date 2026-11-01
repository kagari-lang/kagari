//! Observations and idle maintenance for reclaimable collector metadata.

use crate::{
    error::GcError,
    heap::{Heap, Phase},
};
use std::time::{Duration, Instant};

/// Requested storage for a stable-ID directory, excluding allocator overhead.
/// Partially occupied blocks retain empty slots. Bytes include radix branches.
#[derive(Debug, Clone, Copy)]
pub struct TableStorage {
    pub entries: usize,
    pub slots: usize,
    pub bytes: usize,
}

impl TableStorage {
    fn new(entries: usize, (slots, bytes): (usize, usize)) -> Self {
        Self {
            entries,
            slots,
            bytes,
        }
    }
}

/// Collector table/buffer capacity, separate from managed object backing and RSS.
/// Excludes individual Arc allocations, root-pack targets, descriptor-owned
/// layouts, source-owned frames and allocator bookkeeping. HashMap exposes entry
/// capacity rather than its implementation-specific byte allocation.
#[derive(Debug, Clone, Copy)]
pub struct MetadataStats {
    pub pages: TableStorage,
    pub roots: TableStorage,
    pub descriptors: TableStorage,
    pub root_active_bytes: usize,
    pub descriptor_active_bytes: usize,
    pub release_queue_bytes: usize,
    pub mark_queue_bytes: usize,
    pub scan_workspace_bytes: usize,
    pub type_map_capacity: usize,
}

impl Heap {
    /// Inspect allocated metadata blocks, not historical IDs. This diagnostic
    /// walks the directories and locks the release port; it is not a hot-path counter.
    pub fn metadata_stats(&self) -> MetadataStats {
        let (root_active_bytes, release_queue_bytes) = self.roots.buffer_storage();
        let (slots, bytes, descriptor_active_bytes, type_map_capacity) =
            self.physical.descriptors.storage();
        MetadataStats {
            pages: TableStorage::new(
                self.physical.page_count(),
                self.physical.allocations.storage(),
            ),
            roots: TableStorage::new(self.roots.count, self.roots.slots.storage()),
            descriptors: TableStorage::new(self.physical.descriptors.len(), (slots, bytes)),
            root_active_bytes,
            descriptor_active_bytes,
            release_queue_bytes,
            mark_queue_bytes: self.marking.buffer_bytes(),
            scan_workspace_bytes: self.workspace.bytes(),
            type_map_capacity,
        }
    }

    /// Shrink excess auxiliary capacity at an idle boundary, with hysteresis.
    /// Returns None during a cycle, and errors for a faulted/closed heap. Empty
    /// directory blocks already release during ordinary retirement. This optional
    /// buffer maintenance can copy live entries and is not work- or time-bounded.
    /// Full collect and completing automatic safepoints call it automatically;
    /// step-only drivers choose when to call it after reaching Idle. No GC, object
    /// hooks or root-source scans run here. Allocation-free root Drop credits and
    /// registered scanner workspace requirements remain reserved.
    pub fn trim_metadata(&mut self) -> Result<Option<Duration>, GcError> {
        self.marking.check()?;
        if self.marking.phase != Phase::Idle {
            return Ok(None);
        }
        assert!(self.active.is_none());
        let start = Instant::now();
        self.physical.allocations.trim();
        self.roots.trim();
        self.marking.trim(self.physical.page_count());
        let needed = self.physical.descriptors.trim();
        self.workspace.trim(needed);
        Ok(Some(start.elapsed()))
    }
}
