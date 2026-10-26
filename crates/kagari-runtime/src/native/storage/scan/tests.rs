use super::{NativeScanKind, ScanBudget, ScanEntries, ScanStatus, ScanWorkspace};
use crate::{native::storage::NativePayload, resource::HeapCharge, value::Value};

#[derive(Debug)]
struct Edges(Vec<Value>);

#[derive(Clone, Copy)]
#[repr(align(64))]
struct Cursor {
    index: usize,
    end: usize,
}

// SAFETY: The Vec owns every edge; the aligned continuation contains indices only.
unsafe impl NativePayload for Edges {
    type ScanState = Cursor;
    const SCAN_KIND: NativeScanKind = NativeScanKind::Resumable;

    fn begin_scan(&self) -> Cursor {
        Cursor {
            index: 0,
            end: self.0.len(),
        }
    }

    fn scan<'a>(
        &'a self,
        cursor: &mut Cursor,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        assert_eq!((cursor as *const Cursor as usize) % 64, 0);
        while cursor.index < cursor.end {
            if !budget.take(1) {
                return ScanStatus::Pending;
            }
            visit(&self.0[cursor.index]);
            cursor.index += 1;
        }
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(self.0.len(), self.0.capacity() * size_of::<Value>())
    }
}

#[test]
fn erased_scanner_requires_admission_and_drains_an_aligned_finite_continuation() {
    let payload = Edges(vec![Value::Unit; 1025]);
    let entries = ScanEntries::new::<Edges>();
    let mut workspace = ScanWorkspace::default();
    assert!(
        entries
            .trace(&payload, &mut workspace, &mut |_| {})
            .is_err()
    );
    workspace.reserve(entries.layout).unwrap();
    let mut visited = 0;
    entries
        .trace(&payload, &mut workspace, &mut |_| visited += 1)
        .unwrap();
    assert_eq!(visited, 1025);
    let mut cursor = payload.begin_scan();
    let mut budget = ScanBudget::new(1);
    assert_eq!(
        payload.scan(&mut cursor, &mut budget, &mut |_| {}),
        ScanStatus::Pending
    );
    assert_eq!(budget.spent(), 1);
    let copied = cursor;
    assert_eq!((copied.index, copied.end), (1, 1025));
}

#[derive(Debug)]
struct InvalidProgress;

// Deliberate unsafe-provider fault injection: the driver must reject no progress.
unsafe impl NativePayload for InvalidProgress {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::Resumable;

    fn begin_scan(&self) {}

    fn scan<'a>(
        &'a self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        ScanStatus::Pending
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::default()
    }
}

#[test]
fn erased_scanner_rejects_pending_without_work() {
    let entries = ScanEntries::new::<InvalidProgress>();
    let mut workspace = ScanWorkspace::default();
    workspace.reserve(entries.layout).unwrap();
    assert!(
        entries
            .trace(&InvalidProgress, &mut workspace, &mut |_| {})
            .is_err()
    );
}
