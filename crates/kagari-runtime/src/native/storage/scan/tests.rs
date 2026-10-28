use super::{
    MAX_BOUNDED_WORK, NORMAL_WORK, NativeScanKind, ScanBudget, ScanEntries, ScanStatus,
    ScanWorkspace, WorkspaceState,
};
use crate::{
    Runtime,
    gc::{
        graph::{marks::initialized_words, objects::ScanEdge},
        metrics::IndivisibleKind,
    },
    native::storage::{NativePayload, NativeStorage},
    resource::HeapCharge,
    test_support::layouts::allocation_owner,
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    alloc::Layout,
    any::TypeId,
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
struct IndivisibleEdges(Edges);

// SAFETY: All strong edges are visited in one explicitly indivisible operation;
// the unit cursor owns no pointers, loans or roots.
unsafe impl NativePayload for IndivisibleEdges {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::Indivisible;

    fn begin_scan(&self) {}

    fn scan<'a>(
        &'a self,
        _: &mut (),
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        budget.charge(self.0.0.len());
        for value in &self.0.0 {
            visit(value);
        }
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        self.0.charge()
    }
}

#[test]
fn collection_observes_indivisible_provider_type_work_and_extent_not_admission() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    runtime.gc.start_collection_observation().unwrap();
    let object = NativeStorage::payload::<IndivisibleEdges>()
        .prepare_payload(
            runtime.gc(),
            &Ty::Builtin(BuiltinType::Unit),
            IndivisibleEdges(Edges(vec![Value::Unit; 3])),
            &owner,
        )
        .unwrap();
    let id = runtime.gc.alloc_native(object).unwrap();
    let root = runtime.root_value(Value::GcHandle(id)).unwrap();
    assert_eq!(
        runtime.gc.collection_observation().unwrap().indivisible
            [IndivisibleKind::ProviderScan as usize]
            .count,
        0
    );
    runtime.collect_garbage().unwrap();
    let observed = runtime.gc.collection_observation().unwrap();
    let providers = observed.indivisible[IndivisibleKind::ProviderScan as usize];
    assert_eq!(providers.count, 1);
    assert_eq!(providers.total_items, 3);
    assert_eq!(providers.total_work, 3);
    let sample = providers.slowest.unwrap();
    assert_eq!(sample.owner, "NativePayload");
    assert_eq!(sample.rust_type, Some(TypeId::of::<IndivisibleEdges>()));
    assert_eq!(sample.known_bytes, Some(size_of::<IndivisibleEdges>()));
    assert!(!sample.time.is_zero());
    assert!(sample.time <= observed.explicit.total_time);
    assert!(root.value(runtime.gc()).is_some());
}

#[test]
fn traced_provider_reserves_actual_bitmap_resets_before_advancing_opaque_state() {
    let runtime = Runtime::default();
    let values = (0..513)
        .filter_map(|index| {
            let value = runtime.gc.alloc_string(index.to_string()).unwrap();
            (index % 128 == 0).then_some(value)
        })
        .collect();
    let payload = Edges(values);
    let entries = ScanEntries::new::<Edges>();
    let mut workspace = ScanWorkspace::default();
    workspace.reserve(entries.layout).unwrap();
    entries.begin(&payload, &mut workspace).unwrap();
    runtime.gc.graph.lock().begin_cycle().unwrap();
    let mut visited = Vec::new();
    let mut visit = |value: &Value, budget: &mut ScanBudget| {
        let status =
            runtime
                .gc
                .shade_scanned_edge(&runtime.modules, ScanEdge::Value(*value), budget)?;
        if status == ScanStatus::Complete {
            visited.push(*value);
        }
        Ok(status)
    };
    let before = initialized_words();
    let mut short = ScanBudget::new(4);
    assert_eq!(
        entries
            .resume_traced(
                &runtime.gc,
                &payload,
                &mut workspace,
                &mut short,
                &mut visit
            )
            .unwrap(),
        ScanStatus::Pending
    );
    assert_eq!(short.spent(), 0);
    assert_eq!(initialized_words(), before);
    let mut first = ScanBudget::new(16);
    assert_eq!(
        entries
            .resume_traced(
                &runtime.gc,
                &payload,
                &mut workspace,
                &mut first,
                &mut visit
            )
            .unwrap(),
        ScanStatus::Pending
    );
    assert_eq!(first.spent(), 15);
    assert_eq!(
        initialized_words() - before,
        12,
        "three prepaid edges, not a later work debt"
    );
    let mut second = ScanBudget::new(10);
    assert_eq!(
        entries
            .resume_traced(
                &runtime.gc,
                &payload,
                &mut workspace,
                &mut second,
                &mut visit
            )
            .unwrap(),
        ScanStatus::Complete
    );
    assert_eq!(second.spent(), 10);
    assert_eq!(initialized_words() - before, 20);
    assert_eq!(
        visited, payload.0,
        "opaque continuation neither skips nor repeats an edge"
    );
    runtime.gc.graph.lock().finish_roots().unwrap();
    runtime.gc.drain_graph_work(&runtime.modules).unwrap();
    runtime.gc.graph.lock().close_mark().unwrap();
    runtime.gc.graph.lock().finish_sweep().unwrap();

    // The same edge workload may overrun only under an explicit indivisible
    // declaration, and both provider slots and bitmap stores remain reported.
    let payload = IndivisibleEdges(payload);
    let entries = ScanEntries::new::<IndivisibleEdges>();
    entries.begin(&payload, &mut workspace).unwrap();
    runtime.gc.graph.lock().begin_cycle().unwrap();
    let before = initialized_words();
    let mut budget = ScanBudget::new(1);
    assert_eq!(
        entries
            .resume_traced(
                &runtime.gc,
                &payload,
                &mut workspace,
                &mut budget,
                &mut |value, budget| {
                    runtime
                        .gc
                        .shade_scanned_edge(&runtime.modules, ScanEdge::Value(*value), budget)
                }
            )
            .unwrap(),
        ScanStatus::Complete
    );
    assert_eq!(initialized_words() - before, 20);
    assert_eq!(budget.spent(), 25);

    runtime.gc.graph.lock().finish_roots().unwrap();
    runtime.gc.drain_graph_work(&runtime.modules).unwrap();
    runtime.gc.graph.lock().close_mark().unwrap();
    runtime.gc.graph.lock().finish_sweep().unwrap();
}

#[derive(Debug)]
struct Whole<const WORK: usize>;

// SAFETY: No references or borrowed state; the declared bound is exact.
unsafe impl<const WORK: usize> NativePayload for Whole<WORK> {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::Bounded { max_work: WORK };

    fn begin_scan(&self) {
        assert!(
            WORK <= MAX_BOUNDED_WORK,
            "invalid provider must be rejected before entry"
        );
    }

    fn scan<'a>(
        &'a self,
        _: &mut (),
        budget: &mut ScanBudget,
        _: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        assert!(budget.take(WORK));
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::default()
    }
}

#[test]
fn bounded_scans_fit_one_normal_step_and_reject_oversized_admission_before_provider_entry() {
    let runtime = Runtime::default();
    let entries = ScanEntries::new::<Whole<204>>();
    let mut workspace = ScanWorkspace::default();
    workspace.reserve(entries.layout).unwrap();
    entries.begin(&Whole::<204>, &mut workspace).unwrap();
    let mut short = ScanBudget::new(1019);
    assert_eq!(
        entries
            .resume_traced(
                &runtime.gc,
                &Whole::<204>,
                &mut workspace,
                &mut short,
                &mut |_, _| Ok(ScanStatus::Complete)
            )
            .unwrap(),
        ScanStatus::Pending
    );
    assert_eq!(short.spent(), 0);
    let mut full = ScanBudget::new(NORMAL_WORK);
    assert_eq!(
        entries
            .resume_traced(
                &runtime.gc,
                &Whole::<204>,
                &mut workspace,
                &mut full,
                &mut |_, _| Ok(ScanStatus::Complete)
            )
            .unwrap(),
        ScanStatus::Complete
    );
    assert_eq!(full.spent(), 204, "unused edge reservation is returned");

    let oversized = ScanEntries::new::<Whole<205>>();
    assert!(oversized.inspect(&Whole::<205>, &mut |_| {}).is_err());
    assert!(oversized.begin(&Whole::<205>, &mut workspace).is_err());
    assert_eq!(workspace.state, WorkspaceState::Idle);
    let storage = NativeStorage::new(|_| Ok(Whole::<205>));
    let before = runtime.gc().stats();
    assert!(
        runtime
            .gc()
            .scan_native_payload(&storage, &Whole::<205>, &mut |_| {})
            .is_err()
    );
    assert_eq!(runtime.gc().stats(), before);
    assert!(!runtime.resources().is_quarantined());
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
