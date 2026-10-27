use super::{NativeScanKind, ScanBudget, ScanEntries, ScanStatus, ScanWorkspace, WorkspaceState};
use crate::{native::storage::NativePayload, resource::HeapCharge, value::Value};
use std::{
    alloc::Layout,
    panic::{AssertUnwindSafe, catch_unwind},
};

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
    entries.begin(&payload, &mut workspace).unwrap();
    let mut service = ScanBudget::new(usize::MAX);
    let mut visited = 0;
    for (index, work) in [256, 256, 256, 256, 1].into_iter().enumerate() {
        let before = service.spent();
        let status = entries
            .resume(&payload, &mut workspace, &mut service, &mut |_| {
                visited += 1
            })
            .unwrap();
        assert_eq!(service.spent() - before, work);
        assert_eq!(
            status,
            if index == 4 {
                ScanStatus::Complete
            } else {
                ScanStatus::Pending
            }
        );
    }
    assert_eq!(visited, 1025);
    assert_eq!(service.spent(), 1025);
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
    assert_eq!(workspace.state, WorkspaceState::Faulted);
    assert!(entries.inspect(&InvalidProgress, &mut |_| {}).is_err());
}

#[test]
fn active_cursor_survives_growth_failure_and_scoped_inspection_without_restart() {
    let payload = Edges((0..5).map(Value::I32).collect());
    let entries = ScanEntries::new::<Edges>();
    let mut workspace = ScanWorkspace::default();
    workspace.reserve(entries.layout).unwrap();
    entries.begin(&payload, &mut workspace).unwrap();
    let mut values = Vec::new();
    assert_eq!(
        entries
            .resume(
                &payload,
                &mut workspace,
                &mut ScanBudget::new(1),
                &mut |v| values.push(*v)
            )
            .unwrap(),
        ScanStatus::Pending
    );
    let old_allocation = workspace.allocation;
    let old_state = workspace.state;
    let larger = Layout::from_size_align(4096, 256).unwrap();
    workspace.fail_next_growth = true;
    assert!(workspace.reserve(larger).is_err());
    assert_eq!(workspace.allocation, old_allocation);
    assert_eq!(workspace.state, old_state);
    workspace.reserve(larger).unwrap();
    assert_eq!(workspace.state, old_state);
    let (allocation, layout) = workspace.allocation.unwrap();
    assert_eq!(allocation.as_ptr() as usize % 256, 0);
    assert!(layout.size() >= 4096);
    // A different finite extent must not overwrite the suspended state.
    let mut inspected = 0;
    entries
        .inspect(&Edges(vec![Value::Unit; 1025]), &mut |_| inspected += 1)
        .unwrap();
    assert_eq!(inspected, 1025);
    // Beginning a second collector job must not discard the active job either.
    assert!(entries.begin(&payload, &mut workspace).is_err());
    loop {
        let mut budget = ScanBudget::new(1);
        let status = entries
            .resume(&payload, &mut workspace, &mut budget, &mut |v| {
                values.push(*v)
            })
            .unwrap();
        assert_eq!(budget.spent(), 1);
        if status == ScanStatus::Complete {
            break;
        }
    }
    assert_eq!(values, payload.0);
    assert_eq!(workspace.state, WorkspaceState::Idle);
}

#[test]
fn exhausted_slice_leaves_the_active_cursor_unchanged() {
    let payload = Edges(vec![Value::I32(1), Value::I32(2)]);
    let entries = ScanEntries::new::<Edges>();
    let mut workspace = ScanWorkspace::default();
    workspace.reserve(entries.layout).unwrap();
    entries.begin(&payload, &mut workspace).unwrap();
    let mut budget = ScanBudget::new(0);
    assert_eq!(
        entries
            .resume(&payload, &mut workspace, &mut budget, &mut |_| panic!(
                "no work"
            ))
            .unwrap(),
        ScanStatus::Pending
    );
    assert_eq!(budget.spent(), 0);
    let mut values = Vec::new();
    assert_eq!(
        entries
            .resume(
                &payload,
                &mut workspace,
                &mut ScanBudget::new(2),
                &mut |v| values.push(*v)
            )
            .unwrap(),
        ScanStatus::Complete
    );
    assert_eq!(values, payload.0);
}

#[derive(Debug)]
struct PanicScanner {
    during_begin: bool,
}

// Deliberate unsafe-provider fault injection, at both continuation boundaries.
unsafe impl NativePayload for PanicScanner {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::Resumable;

    fn begin_scan(&self) {
        assert!(!self.during_begin, "provider begin panic");
    }

    fn scan<'a>(
        &'a self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        panic!("provider step panic");
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::default()
    }
}

#[test]
fn provider_unwind_faults_the_workspace_and_cannot_retry_or_start_another_job() {
    for during_begin in [false, true] {
        let entries = ScanEntries::new::<PanicScanner>();
        let mut workspace = ScanWorkspace::default();
        workspace.reserve(entries.layout).unwrap();
        let payload = PanicScanner { during_begin };
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                entries
                    .trace(&payload, &mut workspace, &mut |_| {})
                    .unwrap();
            }))
            .is_err()
        );
        assert_eq!(workspace.state, WorkspaceState::Faulted);
        assert!(entries.begin(&payload, &mut workspace).is_err());
        assert!(
            entries
                .resume(
                    &payload,
                    &mut workspace,
                    &mut ScanBudget::new(1),
                    &mut |_| {}
                )
                .is_err()
        );
        workspace
            .reserve(Layout::from_size_align(4096, 256).unwrap())
            .unwrap();
        assert_eq!(workspace.state, WorkspaceState::Faulted);
    }
}

#[test]
fn mismatched_erased_state_is_rejected_before_casting_or_invoking_the_provider() {
    let payload = Edges(vec![Value::Unit]);
    let entries = ScanEntries::new::<Edges>();
    let foreign = ScanEntries::new::<InvalidProgress>();
    let mut workspace = ScanWorkspace::default();
    workspace.reserve(entries.layout).unwrap();
    entries.begin(&payload, &mut workspace).unwrap();
    assert!(
        foreign
            .resume(
                &InvalidProgress,
                &mut workspace,
                &mut ScanBudget::new(1),
                &mut |_| {}
            )
            .is_err()
    );
    assert_eq!(workspace.state, WorkspaceState::Faulted);
}
