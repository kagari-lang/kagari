pub(crate) mod layouts;
pub(crate) mod native;
pub(crate) mod update_programs;
#[path = "../tests/support/update_publication.rs"]
pub(crate) mod update_publication;

// Shared raw-fixture preparation for unit and integration boundaries.
pub(crate) mod bytecode;

pub(crate) mod future;

use crate::Runtime;
use kagari_gc::heap::HeapStats;

impl Runtime {
    /// Finish managed collection before comparing live root registrations.
    /// Also inspects source cleanup after control-only quarantine; it does not
    /// reopen Runtime execution or clear a collector fault.
    pub(crate) fn collected_gc_stats(&mut self) -> HeapStats {
        self.memory.collect(&self.resources).unwrap();
        self.gc_stats()
    }
}
