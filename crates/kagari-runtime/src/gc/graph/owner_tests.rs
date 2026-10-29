use super::{GraphIdentity, MarkPhase, objects::ScanEdge};
use crate::{
    Runtime,
    closure::{ClosureTarget, ClosureValueSnapshot},
    execution_metadata::{MetadataEdge, applications::MethodApplication},
    frame::types::{EnvironmentRecord, construction::ConstructionScope},
    gc::{HeapObjectId, interfaces::InterfaceResultBinding},
    module::state::tests::{program, ready},
    native::storage::{
        NativePayload, NativeStorage,
        scan::{NativeScanKind, ScanBudget, ScanStatus},
    },
    resource::HeapCharge,
    test_support::layouts::{allocation_owner, interface_value_with},
    value::Value,
};
use kagari_contract::ids::FunctionRef;
use kagari_types::{callable::Signature, scalar::BuiltinType, ty::Ty};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

fn drain_tiny(runtime: &Runtime) {
    for _ in 0..10000 {
        let mut budget = ScanBudget::new(5);
        let status = runtime
            .gc
            .scan_graph_slice(runtime.modules(), &mut budget)
            .unwrap();
        assert!(budget.spent() <= 5);
        if status == ScanStatus::Complete {
            return;
        }
        assert!(budget.spent() > 0, "finite owner slices must progress");
    }
    panic!("owner scanning did not finish");
}

#[test]
fn object_metadata_module_and_static_owners_close_one_joint_graph_in_tiny_slices() {
    let mut runtime = Runtime::default();
    let installed = runtime
        .load_program(
            "scan-static",
            program(Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))), true),
        )
        .unwrap();
    let owner = runtime.select_program(&installed).unwrap();
    let array = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
        .unwrap();
    ready(&runtime, &owner, 0, Value::Array(array));
    let mut parent = EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
    parent.set_constructions(ConstructionScope::new(&owner));
    let parent = runtime.alloc_environment(parent).unwrap();
    let group = runtime
        .gc
        .alloc_operation_group(runtime.modules(), vec![])
        .unwrap();
    let mut child = EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
    child.include(Some(parent.clone())).unwrap();
    child.add_receiver(&runtime.gc, group).unwrap();
    let child = runtime.alloc_environment(child).unwrap();
    let application = runtime
        .gc
        .alloc_method_application(
            runtime.modules(),
            MethodApplication {
                signature: Signature {
                    params: vec![],
                    result: Ty::Builtin(BuiltinType::Unit),
                },
                scoped_signature: None,
                environment: Some(child.clone()),
                result_adapter: Some(InterfaceResultBinding {
                    owner: owner.clone(),
                    table: 0,
                    arguments: vec![],
                    environment: Some(parent.clone()),
                }),
            },
        )
        .unwrap();
    let closure = runtime
        .gc
        .alloc_closure(
            runtime.modules(),
            ClosureValueSnapshot {
                implementation: owner.clone(),
                target: ClosureTarget::Script(FunctionRef::new(0)),
                captures: vec![Value::Array(array)],
                environment: Some(child.clone()),
            },
        )
        .unwrap();
    let interface = interface_value_with(
        &mut runtime,
        Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
        Value::Array(array),
    );
    let Value::Interface(interface_id) = interface else {
        panic!("interface");
    };
    let snapshot_id = runtime.gc.interface_snapshot_id(interface_id).unwrap();
    let description_id = runtime
        .gc
        .interface_snapshot(interface_id)
        .unwrap()
        .description
        .scan_id;
    let garbage = runtime.gc.alloc_string("not reachable".into()).unwrap();
    runtime.gc.graph.lock().begin_cycle().unwrap();
    for edge in [
        ScanEdge::Value(Value::Closure(closure)),
        ScanEdge::Value(interface),
        ScanEdge::Metadata(MetadataEdge::Application(application)),
    ] {
        runtime
            .gc
            .shade_graph_edge(runtime.modules(), edge)
            .unwrap();
    }
    // An empty work stack alone cannot certify unenumerated roots.
    assert!(runtime.gc.graph.lock().close_mark().is_err());
    runtime.gc.graph.lock().finish_roots().unwrap();
    drain_tiny(&runtime);
    runtime.gc.graph.lock().close_mark().unwrap();
    for id in [
        GraphIdentity::Object(array),
        GraphIdentity::Object(closure),
        GraphIdentity::Interface(snapshot_id),
        GraphIdentity::InterfaceDescription(description_id),
        GraphIdentity::Application(application),
        GraphIdentity::Environment(child.id),
        GraphIdentity::Environment(parent.id),
        GraphIdentity::Group(group),
    ] {
        assert!(
            runtime.gc.graph_survivor(runtime.modules(), id).unwrap(),
            "missing owner {id:?}"
        );
    }
    assert!(
        !runtime
            .gc
            .graph_survivor(
                runtime.modules(),
                GraphIdentity::Object(garbage.checked_id().unwrap())
            )
            .unwrap()
    );
    let static_id = runtime
        .modules()
        .bound_static_scan_identity(&owner, 0)
        .unwrap();
    assert!(
        runtime
            .gc
            .graph_survivor(runtime.modules(), static_id)
            .unwrap()
    );
    let program_id = runtime.modules().program_scan_identity(&owner).unwrap();
    assert!(
        runtime
            .gc
            .graph_survivor(runtime.modules(), program_id)
            .unwrap()
    );
    runtime.gc.graph.lock().finish_sweep().unwrap();
    // This test exercises mark closure only; the production collection entrypoint
    // also runs the synchronous retirement adapter.
    assert!(runtime.gc.validate_value(&garbage));
}

#[derive(Debug)]
struct Edges<const N: usize> {
    values: Vec<Value>,
    begins: Arc<AtomicUsize>,
    fault: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
#[repr(align(256))]
struct Cursor<const N: usize> {
    next: usize,
    end: usize,
    padding: [u8; N],
}

// SAFETY: Finite indexed strong edges and pointer-free Copy continuation. The
// counters observe scanner calls in tests; the fault flag injects an unsafe-provider violation.
unsafe impl<const N: usize> NativePayload for Edges<N> {
    type ScanState = Cursor<N>;
    const SCAN_KIND: NativeScanKind = NativeScanKind::Resumable;

    fn begin_scan(&self) -> Self::ScanState {
        self.begins.fetch_add(1, Ordering::Relaxed);
        Cursor {
            next: 0,
            end: self.values.len(),
            padding: [3; N],
        }
    }

    fn scan<'a>(
        &'a self,
        state: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        assert!(
            !self.fault.load(Ordering::Relaxed),
            "injected provider fault"
        );
        assert_eq!(state.padding, [3; N]);
        while state.next < state.end {
            if !budget.take(1) {
                return ScanStatus::Pending;
            }
            if let Some(value) = self.values.get(state.next) {
                visit(value);
            }
            state.next += 1;
        }
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(
            self.values.len(),
            self.values.capacity() * size_of::<Value>(),
        )
    }
}

#[test]
fn newborn_work_and_larger_payload_admission_preserve_the_active_object_job() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let children = (0..4)
        .map(|i| runtime.gc.alloc_string(i.to_string()).unwrap())
        .collect::<Vec<_>>();
    let begins = Arc::new(AtomicUsize::new(0));
    let object = NativeStorage::payload::<Edges<0>>()
        .prepare_payload(
            runtime.gc(),
            &Ty::Builtin(BuiltinType::Unit),
            Edges::<0> {
                values: children.clone(),
                begins: begins.clone(),
                fault: Arc::new(AtomicBool::new(false)),
            },
            &owner,
        )
        .unwrap();
    let id = runtime.gc.alloc_native(object).unwrap();
    runtime.gc.graph.lock().begin_cycle().unwrap();
    runtime
        .gc
        .shade_graph_edge(runtime.modules(), ScanEdge::Value(Value::GcHandle(id)))
        .unwrap();
    runtime.gc.graph.lock().finish_roots().unwrap();
    for _ in 0..7 {
        assert_eq!(
            runtime
                .gc
                .scan_graph_slice(runtime.modules(), &mut ScanBudget::new(1))
                .unwrap(),
            ScanStatus::Pending
        );
    }
    assert_eq!(
        runtime.gc.graph.lock().active,
        Some(GraphIdentity::Object(id))
    );
    let newborn_child = runtime.gc.alloc_string("newborn".into()).unwrap();
    let larger = NativeStorage::payload::<Edges<4096>>()
        .prepare_payload(
            runtime.gc(),
            &Ty::Builtin(BuiltinType::Unit),
            Edges::<4096> {
                values: vec![newborn_child],
                begins: Arc::default(),
                fault: Arc::default(),
            },
            &owner,
        )
        .unwrap();
    let newborn = runtime.gc.alloc_native(larger).unwrap();
    assert_eq!(
        runtime.gc.graph.lock().active,
        Some(GraphIdentity::Object(id))
    );
    drain_tiny(&runtime);
    runtime.gc.graph.lock().close_mark().unwrap();
    assert_eq!(
        begins.load(Ordering::Relaxed),
        2,
        "one admission scan and one nonrestarted collector scan"
    );
    for value in children
        .into_iter()
        .chain([newborn_child, Value::GcHandle(newborn)])
    {
        assert!(
            runtime
                .gc
                .graph_survivor(
                    runtime.modules(),
                    GraphIdentity::Object(value.checked_id().unwrap())
                )
                .unwrap()
        );
    }
}

#[test]
fn failed_admission_preserves_the_active_owner_and_publishes_no_credit_or_record() {
    let runtime = Runtime::default();
    let child = runtime.gc.alloc_string("child".into()).unwrap();
    runtime.gc.graph.lock().begin_cycle().unwrap();
    runtime
        .gc
        .shade_graph_edge(runtime.modules(), ScanEdge::Value(child))
        .unwrap();
    // This is mutator publication: its bitmap work is not deferred service debt.
    assert_eq!(
        runtime
            .gc
            .scan_graph_slice(runtime.modules(), &mut ScanBudget::new(1))
            .unwrap(),
        ScanStatus::Pending
    );
    let before = runtime.gc.stats();
    let count = runtime.gc.graph.lock().credits;
    runtime.gc.graph.lock().fail_reservation = true;
    assert!(runtime.gc.alloc_string("rejected".into()).is_err());
    assert_eq!(runtime.gc.stats(), before);
    runtime.gc.graph.lock().fail_reservation = true;
    assert!(
        runtime
            .gc
            .admit_interface_call(runtime.modules(), |_| panic!(
                "construction before reservation"
            ))
            .is_err()
    );
    {
        let graph = runtime.gc.graph.lock();
        assert_eq!(graph.credits, count);
        assert_eq!(
            graph.active,
            Some(GraphIdentity::Object(child.checked_id().unwrap()))
        );
    }
    // Resource-limit errors may terminate execution admission, but the already
    // admitted graph still has intact storage and reserved completion capacity.
    runtime.gc.graph.lock().finish_roots().unwrap();
    drain_tiny(&runtime);
    runtime.gc.graph.lock().close_mark().unwrap();
}

#[test]
fn idle_publication_checks_original_identity_and_kind_without_locking_work() {
    let runtime = Runtime::default();
    let foreign = Runtime::default();
    let value = runtime.gc.alloc_string("local".into()).unwrap();
    let other = foreign.gc.alloc_string("foreign".into()).unwrap();
    let id = value.checked_id().unwrap();
    let stale = HeapObjectId::new(u64::from(id.owner), id.index(), id.generation + 1);
    let guard = runtime.gc.graph.lock();
    runtime.gc.publish_value(value).unwrap();
    assert!(
        runtime
            .gc
            .heap_identity_accessible(GraphIdentity::Object(id))
    );
    assert!(runtime.gc.publish_value(other).is_err());
    assert!(runtime.gc.publish_value(Value::Str(stale)).is_err());
    assert!(runtime.gc.publish_value(Value::Tuple(id)).is_err());
    assert!(
        !runtime
            .gc
            .heap_identity_accessible(GraphIdentity::Object(stale))
    );
    assert_eq!(guard.phase(), MarkPhase::Idle);
}

#[test]
fn poisoned_graph_control_returns_a_trace_fault_without_panicking_during_cleanup() {
    for marking in [false, true] {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let value = runtime
            .gc
            .alloc_string("retained after fault".into())
            .unwrap();
        if marking {
            runtime.gc.graph.lock().begin_cycle().unwrap();
        }
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _guard = runtime.gc.graph.lock();
                panic!("injected graph control poison");
            }))
            .is_err()
        );
        assert_eq!(
            MarkPhase::load(&runtime.gc.graph.0.phase),
            MarkPhase::Poisoned
        );
        assert!(runtime.gc.publish_value(value).is_err());
        assert!(runtime.validate_executable_module(&owner).is_err());
        assert!(runtime.publish_executable_module(&owner).is_err());
        assert!(
            !runtime
                .gc
                .heap_identity_accessible(GraphIdentity::Object(value.checked_id().unwrap()))
        );
        assert!(
            runtime
                .gc
                .scan_graph_slice(runtime.modules(), &mut ScanBudget::new(1))
                .is_err()
        );
        assert!(runtime.is_quarantined());
        assert!(runtime.gc.graph.try_lock().is_err());
        assert!(runtime.gc.validate_value(&value));
    }
}

#[test]
fn owner_fault_preserves_gray_work_and_never_allows_mark_closure() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let fault = Arc::new(AtomicBool::new(false));
    let object = NativeStorage::payload::<Edges<0>>()
        .prepare_payload(
            runtime.gc(),
            &Ty::Builtin(BuiltinType::Unit),
            Edges::<0> {
                values: vec![Value::Unit],
                begins: Arc::default(),
                fault: fault.clone(),
            },
            &owner,
        )
        .unwrap();
    let id = runtime.gc.alloc_native(object).unwrap();
    let garbage = runtime.gc.alloc_string("must not sweep".into()).unwrap();
    runtime.gc.graph.lock().begin_cycle().unwrap();
    runtime
        .gc
        .shade_graph_edge(runtime.modules(), ScanEdge::Value(Value::GcHandle(id)))
        .unwrap();
    runtime.gc.graph.lock().finish_roots().unwrap();
    fault.store(true, Ordering::Relaxed);
    assert!(
        runtime
            .gc
            .scan_graph_slice(runtime.modules(), &mut ScanBudget::new(64))
            .is_err()
    );
    let mut graph = runtime.gc.graph.lock();
    assert_eq!(graph.phase(), MarkPhase::Faulted);
    assert_eq!(graph.active, Some(GraphIdentity::Object(id)));
    assert!(graph.close_mark().is_err());
    drop(graph);
    assert!(runtime.collect_garbage().is_err());
    assert!(runtime.gc.validate_value(&garbage));
}
