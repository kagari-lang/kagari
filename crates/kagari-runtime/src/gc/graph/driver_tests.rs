use super::{GraphIdentity, GraphState, MarkPhase, objects::ScanEdge};
use crate::{
    Runtime,
    execution_metadata::MetadataEdge,
    frame::types::EnvironmentRecord,
    module::state::tests::{program, ready},
    native::storage::{
        NativePayload, NativeStorage,
        scan::{NativeScanKind, ScanBudget, ScanStatus},
    },
    resource::HeapCharge,
    test_support::layouts::allocation_owner,
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

thread_local! {
    static ACQUISITIONS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn record_acquisition() {
    ACQUISITIONS.set(ACQUISITIONS.get() + 1);
}

fn scan(runtime: &Runtime) -> (usize, usize) {
    let before = ACQUISITIONS.get();
    let mut budget = ScanBudget::new(usize::MAX);
    assert_eq!(
        runtime
            .gc
            .scan_graph_slice(runtime.modules(), &mut budget)
            .unwrap(),
        ScanStatus::Complete,
    );
    (ACQUISITIONS.get() - before, budget.spent())
}

#[test]
fn internal_tuple_scan_control_acquisitions_and_work() {
    for count in [0, 8, 64] {
        let runtime = Runtime::default();
        let children: Vec<_> = (0..count)
            .map(|index| runtime.gc.alloc_string(index.to_string()).unwrap())
            .collect();
        let parent = runtime.gc.alloc_tuple(children.clone()).unwrap();
        runtime.gc.graph.lock().begin_cycle().unwrap();
        runtime.gc.publish_value(parent).unwrap();
        runtime.gc.graph.lock().finish_roots().unwrap();
        let (acquisitions, work) = scan(&runtime);
        assert_eq!(acquisitions, 1, "one control borrow across all pure owners");
        assert_eq!(
            work,
            count * 4 + 2,
            "selection, begin, fields and leaf scans"
        );
        runtime.gc.graph.lock().close_mark().unwrap();
        for value in children.into_iter().chain([parent]) {
            assert!(
                runtime
                    .gc
                    .graph_survivor(
                        runtime.modules(),
                        GraphIdentity::Object(value.object_id().unwrap())
                    )
                    .unwrap()
            );
        }
    }
}

#[test]
fn metadata_module_and_static_scan_control_acquisitions_and_work() {
    let mut runtime = Runtime::default();
    let installed = runtime
        .load_program("scan-control", program(Ty::Builtin(BuiltinType::I32), true))
        .unwrap();
    let owner = runtime.select_program(&installed).unwrap();
    ready(&runtime, &owner, 0, Value::I32(42));
    let parent = runtime
        .alloc_environment(
            EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap(),
        )
        .unwrap();
    let mut child = EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
    child.include(Some(parent.clone())).unwrap();
    let child = runtime.alloc_environment(child).unwrap();
    runtime.gc.graph.lock().begin_cycle().unwrap();
    for edge in [
        MetadataEdge::Program(&owner),
        MetadataEdge::Environment(child.id),
    ] {
        runtime
            .gc
            .shade_graph_edge(runtime.modules(), ScanEdge::Metadata(edge))
            .unwrap();
    }
    runtime.gc.graph.lock().finish_roots().unwrap();
    let (acquisitions, work) = scan(&runtime);
    assert_eq!(acquisitions, 2, "release only at the descriptor suffix");
    assert_eq!(work, 25, "unchanged module/static/metadata scan work");
    runtime.gc.graph.lock().close_mark().unwrap();
    for id in [
        runtime.modules().program_scan_identity(&owner).unwrap(),
        runtime
            .modules()
            .bound_static_scan_identity(&owner, 0)
            .unwrap(),
        GraphIdentity::Environment(child.id),
        GraphIdentity::Environment(parent.id),
    ] {
        assert!(runtime.gc.graph_survivor(runtime.modules(), id).unwrap());
    }
}

#[derive(Debug)]
struct UnlockedProvider {
    graph: GraphState,
    child: Value,
    fail: Arc<AtomicBool>,
    fail_begin: bool,
}

// SAFETY: One immutable strong edge; the continuation is pointer-free. Panic
// injection tests quarantine, not a successful provider contract.
unsafe impl NativePayload for UnlockedProvider {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::Indivisible;

    fn begin_scan(&self) {
        assert!(
            self.graph.try_lock().is_ok(),
            "provider begin must not borrow control"
        );
        assert!(
            !(self.fail_begin && self.fail.load(Ordering::Relaxed)),
            "begin fault"
        );
    }

    fn scan<'a>(
        &'a self,
        _: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        assert!(
            self.graph.try_lock().is_ok(),
            "provider scan must not borrow control"
        );
        assert!(!self.fail.load(Ordering::Relaxed), "scan fault");
        budget.charge(1);
        visit(&self.child);
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(1, 0)
    }
}

#[test]
fn provider_boundaries_release_control_and_unwind_faults_without_poison() {
    for fail_begin in [false, true] {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let child = runtime.gc.alloc_string("provider edge".into()).unwrap();
        let fail = Arc::new(AtomicBool::new(false));
        let object = NativeStorage::payload::<UnlockedProvider>()
            .prepare_payload(
                runtime.gc(),
                &Ty::Builtin(BuiltinType::Unit),
                UnlockedProvider {
                    graph: runtime.gc.graph.clone(),
                    child,
                    fail: fail.clone(),
                    fail_begin,
                },
                &owner,
            )
            .unwrap();
        let id = runtime.gc.alloc_native(object).unwrap();
        runtime.gc.graph.lock().begin_cycle().unwrap();
        runtime.gc.publish_value(Value::GcHandle(id)).unwrap();
        runtime.gc.graph.lock().finish_roots().unwrap();
        scan(&runtime);
        runtime.gc.graph.lock().close_mark().unwrap();
        assert!(
            runtime
                .gc
                .graph_survivor(
                    runtime.modules(),
                    GraphIdentity::Object(child.object_id().unwrap())
                )
                .unwrap()
        );
        runtime.gc.graph.lock().finish_sweep().unwrap();
        fail.store(true, Ordering::Relaxed);
        runtime.gc.graph.lock().begin_cycle().unwrap();
        runtime.gc.publish_value(Value::GcHandle(id)).unwrap();
        runtime.gc.graph.lock().finish_roots().unwrap();
        assert!(
            runtime
                .gc
                .scan_graph_slice(runtime.modules(), &mut ScanBudget::new(1024))
                .is_err()
        );
        assert_eq!(runtime.gc.graph.phase(), MarkPhase::Faulted);
        assert!(runtime.gc.graph.try_lock().is_ok());
        assert!(runtime.gc.graph.lock().close_mark().is_err());
        assert!(runtime.resources().is_quarantined());
    }
}
