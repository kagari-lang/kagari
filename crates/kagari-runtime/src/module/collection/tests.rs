use crate::{
    Runtime, RuntimeConfig,
    gc::graph::GraphIdentity,
    module::{
        ModuleStore,
        scanning::StaticScanId,
        state::tests::{program, ready},
    },
    native::storage::scan::{NORMAL_WORK, ScanBudget, ScanStatus},
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use rstest::rstest;
use std::{
    sync::{Arc, atomic::Ordering},
    thread,
};

fn runtime() -> Runtime {
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = None;
    Runtime::new(config)
}

fn close_mark(runtime: &Runtime) {
    let mut roots = runtime
        .gc
        .begin_initial_roots(&runtime.modules, &runtime.host)
        .unwrap();
    // Root setup needs room for lazy bitmap initialization as well as inspection.
    // The sweep operations under test below retain their tiny, exact allowances.
    while runtime
        .gc
        .scan_initial_root_slice(
            &runtime.modules,
            &runtime.host,
            &mut roots,
            &mut ScanBudget::new(NORMAL_WORK),
        )
        .unwrap()
        == ScanStatus::Pending
    {}
    runtime.gc.drain_graph_work(&runtime.modules).unwrap();
    runtime.gc.graph.lock().close_mark().unwrap();
}

#[rstest]
#[case::abort(false)]
#[case::complete(true)]
fn abandonment_ticket_preserves_concurrent_new_request(#[case] finish: bool) {
    let mut runtime = runtime();
    let code = program(Ty::Builtin(BuiltinType::I32), true);
    let installed = runtime.load_program("ticket", code.clone()).unwrap();
    let first = runtime
        .stage_reload_program(&installed, None, code.clone())
        .unwrap();
    drop(first);
    assert!(runtime.modules.abandonment_pending());
    let ticket = runtime.modules.begin_collection();
    assert!(!runtime.modules.abandonment_pending());
    let second = runtime
        .stage_reload_program(&installed, None, code)
        .unwrap();
    thread::spawn(move || drop(second)).join().unwrap();
    assert!(runtime.modules.abandonment_pending());
    if finish {
        ticket.finish();
    } else {
        drop(ticket);
    }
    assert!(runtime.modules.abandonment_pending());
}

#[test]
fn abandonment_ticket_restores_only_consumed_request_and_outlives_store() {
    let store = ModuleStore::default();
    drop(store.begin_collection());
    assert!(!store.abandonment_pending());
    store.abandoned.store(true, Ordering::Release);
    drop(store.begin_collection());
    assert!(store.abandonment_pending());
    let ticket = store.begin_collection();
    let signal = store.abandoned.clone();
    drop(store);
    assert!(!signal.load(Ordering::Acquire));
    drop(ticket);
    assert!(signal.load(Ordering::Acquire));
}

#[test]
fn published_candidate_release_during_ticket_does_not_request_collection() {
    let mut runtime = runtime();
    let code = program(Ty::Builtin(BuiltinType::I32), true);
    let installed = runtime.load_program("published", code.clone()).unwrap();
    let staged = runtime
        .stage_reload_program(&installed, None, code)
        .unwrap();
    let last = staged.program.lease.clone();
    runtime.publish_staged_reload(staged).unwrap();
    let ticket = runtime.modules.begin_collection();
    thread::spawn(move || drop(last)).join().unwrap();
    ticket.finish();
    assert!(!runtime.modules.abandonment_pending());
}

#[rstest]
#[case::vacant(false)]
#[case::reused_and_extended(true)]
fn module_sweep_charges_captured_slots_and_preserves_newborns(#[case] reuse: bool) {
    let mut runtime = runtime();
    let code = program(Ty::Builtin(BuiltinType::I32), true);
    let installed = runtime.load_program("slots", code.clone()).unwrap();
    let owner = runtime.select_program(&installed).unwrap();
    ready(&runtime, &owner, 0, Value::I32(7));
    let abandoned = runtime
        .stage_reload_program(&installed, None, code.clone())
        .unwrap();
    let old = runtime
        .modules
        .program_scan_identity(abandoned.module())
        .unwrap();
    drop(abandoned);
    runtime.collect_garbage().unwrap();
    close_mark(&runtime);
    let mut cursor = runtime.modules.begin_sweep().unwrap();
    assert_eq!(cursor.end, 2);
    let mut report = Vec::new();
    let mut retired = Vec::new();
    let status = runtime
        .modules
        .sweep_programs(
            &mut cursor,
            &mut ScanBudget::new(0),
            32,
            Some(&mut report),
            &mut retired,
        )
        .unwrap();
    assert_eq!(status, ScanStatus::Pending);
    assert!(retired.is_empty());
    assert_eq!(retired.capacity(), 0);
    assert_eq!(report.capacity(), 0);
    assert_eq!(cursor.next, 0);
    let mut budget = ScanBudget::new(1);
    let status = runtime
        .modules
        .sweep_programs(
            &mut cursor,
            &mut budget,
            32,
            Some(&mut report),
            &mut retired,
        )
        .unwrap();
    assert_eq!(status, ScanStatus::Pending);
    assert_eq!(budget.spent(), 1);
    assert!(retired.is_empty());
    assert_eq!(retired.capacity(), 0);
    assert_eq!(report.capacity(), 0);
    let mut newborns = Vec::new();
    if reuse {
        for _ in 0..2 {
            newborns.push(
                runtime
                    .stage_reload_program(&installed, None, code.clone())
                    .unwrap(),
            );
        }
        let new = runtime
            .modules
            .program_scan_identity(newborns[0].module())
            .unwrap();
        let (GraphIdentity::Program(old), GraphIdentity::Program(new)) = (old, new) else {
            panic!("program identities")
        };
        assert_eq!(old.slot, new.slot);
        assert_ne!(old.generation, new.generation);
        assert_eq!(runtime.modules.inner.borrow().records.scan_extent(), 3);
    }
    let mut budget = ScanBudget::new(1);
    let status = runtime
        .modules
        .sweep_programs(
            &mut cursor,
            &mut budget,
            32,
            Some(&mut report),
            &mut retired,
        )
        .unwrap();
    assert_eq!(status, ScanStatus::Complete);
    assert_eq!(budget.spent(), 1);
    assert!(retired.is_empty());
    assert_eq!(retired.capacity(), 0);
    assert_eq!(report.capacity(), 0);
    assert_eq!(cursor.next, 2);
    for newborn in &newborns {
        assert!(runtime.modules.contains_module(newborn.module()).unwrap());
    }
    let mut budget = ScanBudget::new(10);
    let mut retired = Vec::new();
    let status = runtime
        .modules
        .sweep_cells(&mut cursor, &mut budget, 32, &mut retired)
        .unwrap();
    assert_eq!(status, ScanStatus::Complete);
    assert_eq!(
        budget.spent(),
        1,
        "prepended newborn cells are outside the capture"
    );
    assert!(retired.is_empty());
    assert_eq!(retired.capacity(), 0, "live cells need no disposal backing");
    assert_eq!(
        runtime.modules.inner.borrow().cells.len(),
        1 + newborns.len()
    );
    let mut exhausted = Vec::new();
    let mut budget = ScanBudget::new(0);
    assert_eq!(
        runtime
            .modules
            .sweep_cells(&mut cursor, &mut budget, 32, &mut exhausted)
            .unwrap(),
        ScanStatus::Complete
    );
    assert_eq!(exhausted.capacity(), 0);
    assert_eq!(budget.spent(), 0);
    runtime.gc.graph.lock().finish_sweep().unwrap();
}

#[derive(Clone, Copy)]
enum SweepBoundary {
    Valid,
    Foreign,
    Borrowed,
    Stale,
    Idle,
}

#[rstest]
#[case::valid(SweepBoundary::Valid)]
#[case::foreign_owner(SweepBoundary::Foreign)]
#[case::borrowed_store(SweepBoundary::Borrowed)]
#[case::stale_cursor(SweepBoundary::Stale)]
#[case::wrong_phase(SweepBoundary::Idle)]
fn exhausted_sweep_validates_without_reserving_outputs(#[case] boundary: SweepBoundary) {
    let runtime = runtime();
    close_mark(&runtime);
    let mut cursor = runtime.modules.begin_sweep().unwrap();
    let mut borrowed = None;
    match boundary {
        SweepBoundary::Valid => {}
        SweepBoundary::Foreign => cursor.owner = Arc::default(),
        SweepBoundary::Borrowed => borrowed = Some(runtime.modules.inner.borrow()),
        SweepBoundary::Stale => drop(runtime.modules.begin_sweep().unwrap()),
        SweepBoundary::Idle => {
            runtime.gc.graph.lock().finish_sweep().unwrap();
        }
    }
    let mut budget = ScanBudget::new(0);
    let mut report = Vec::new();
    let mut modules = Vec::new();
    let mut cells = Vec::new();
    let programs = runtime.modules.sweep_programs(
        &mut cursor,
        &mut budget,
        32,
        Some(&mut report),
        &mut modules,
    );
    let statics = runtime
        .modules
        .sweep_cells(&mut cursor, &mut budget, 32, &mut cells);
    if matches!(boundary, SweepBoundary::Valid) {
        assert_eq!(programs.unwrap(), ScanStatus::Complete);
        assert_eq!(statics.unwrap(), ScanStatus::Complete);
    } else {
        assert!(programs.is_err());
        assert!(statics.is_err());
    }
    assert_eq!(budget.spent(), 0);
    assert_eq!(report.capacity(), 0);
    assert_eq!(modules.capacity(), 0);
    assert_eq!(cells.capacity(), 0);
    drop(borrowed);
    if !matches!(boundary, SweepBoundary::Idle) {
        runtime.gc.graph.lock().finish_sweep().unwrap();
    }
}

#[rstest]
#[case::same_slice(false)]
#[case::after_yield(true)]
fn sweep_tail_reuses_capacity_bounded_by_remaining_slots_and_occupied_cells(
    #[case] split_live_prefix: bool,
) {
    let mut runtime = runtime();
    let code = program(Ty::Builtin(BuiltinType::I32), true);
    let installed = runtime.load_program("capacity", code.clone()).unwrap();
    let owner = runtime.select_program(&installed).unwrap();
    ready(&runtime, &owner, 0, Value::I32(7));
    let staged = runtime
        .stage_reload_program(&installed, None, code)
        .unwrap();
    let key = staged.module().key();
    drop(staged);
    close_mark(&runtime);
    let mut cursor = runtime.modules.begin_sweep().unwrap();
    assert_eq!(cursor.end, 2);
    if split_live_prefix {
        let mut first = ScanBudget::new(1);
        let mut modules = Vec::new();
        assert_eq!(
            runtime
                .modules
                .sweep_programs(&mut cursor, &mut first, 1, None, &mut modules)
                .unwrap(),
            ScanStatus::Pending
        );
        assert_eq!(first.spent(), 1);
        assert_eq!(cursor.next, 1);
        assert_eq!(modules.capacity(), 0);
    }

    let mut modules = Vec::with_capacity(1);
    // Earlier reported retirements are caller-owned and not part of the 32-owner cap.
    let mut report = Vec::with_capacity(41);
    report.extend([owner.key(); 40]);
    let work = if split_live_prefix { 1 } else { 2 };
    let mut tail = ScanBudget::new(work);
    assert_eq!(
        runtime
            .modules
            .sweep_programs(&mut cursor, &mut tail, 32, Some(&mut report), &mut modules)
            .unwrap(),
        ScanStatus::Complete
    );
    assert_eq!(tail.spent(), work);
    assert_eq!(modules.len(), 1);
    assert_eq!(&report[..40], &[owner.key(); 40]);
    assert_eq!(&report[40..], &[key]);
    assert_eq!(modules.capacity(), 1);
    assert_eq!(report.capacity(), 41);

    let occupied = runtime.modules.inner.borrow().cells.len();
    assert_eq!(occupied, 2);
    let mut cells = Vec::with_capacity(occupied);
    let mut budget = ScanBudget::new(occupied);
    assert_eq!(
        runtime
            .modules
            .sweep_cells(&mut cursor, &mut budget, 32, &mut cells)
            .unwrap(),
        ScanStatus::Complete
    );
    assert_eq!(budget.spent(), occupied);
    assert_eq!(cells.len(), 1);
    assert_eq!(cells.capacity(), occupied);
    assert!(runtime.modules.inner.try_borrow_mut().is_ok());
    drop((modules, cells));
    runtime.gc.graph.lock().finish_sweep().unwrap();
}

#[test]
fn module_batches_report_exact_keys_and_full_batch_is_not_completion() {
    let mut runtime = runtime();
    let code = program(Ty::Builtin(BuiltinType::I32), true);
    let installed = runtime.load_program("batches", code.clone()).unwrap();
    let owner = runtime.select_program(&installed).unwrap();
    ready(&runtime, &owner, 0, Value::I32(7));
    let mut expected = Vec::new();
    for _ in 0..3 {
        let staged = runtime
            .stage_reload_program(&installed, None, code.clone())
            .unwrap();
        expected.push(staged.module().key());
        drop(staged);
    }
    close_mark(&runtime);
    let mut cursor = runtime.modules.begin_sweep().unwrap();
    let mut report = Vec::new();
    let mut budget = ScanBudget::new(10);
    let mut retired = Vec::new();
    let status = runtime
        .modules
        .sweep_programs(&mut cursor, &mut budget, 0, Some(&mut report), &mut retired)
        .unwrap();
    assert_eq!(status, ScanStatus::Pending);
    assert!(retired.is_empty());
    let mut retired = Vec::new();
    let status = runtime
        .modules
        .sweep_cells(&mut cursor, &mut budget, 0, &mut retired)
        .unwrap();
    assert_eq!(status, ScanStatus::Pending);
    assert!(retired.is_empty());
    assert_eq!(budget.spent(), 0);
    assert!(report.is_empty());
    let mut retired = Vec::new();
    for (index, key) in expected.iter().enumerate() {
        let mut budget = ScanBudget::new(10);
        let status = runtime
            .modules
            .sweep_programs(&mut cursor, &mut budget, 1, Some(&mut report), &mut retired)
            .unwrap();
        assert_eq!(
            status,
            if index == 2 {
                ScanStatus::Complete
            } else {
                ScanStatus::Pending
            }
        );
        assert_eq!(budget.spent(), if index == 0 { 2 } else { 1 });
        assert_eq!(retired.len(), index + 1);
        assert_eq!(report, expected[..=index]);
        assert!(!runtime.modules.inner.borrow().records.contains_key(key));
    }
    // The detached backing neither borrows the store nor needs it to drop.
    drop(retired);
    let mut retired = Vec::new();
    loop {
        let mut budget = ScanBudget::new(1);
        let status = runtime
            .modules
            .sweep_cells(&mut cursor, &mut budget, 1, &mut retired)
            .unwrap();
        assert!(budget.spent() <= 1);
        if status == ScanStatus::Complete {
            break;
        }
    }
    assert_eq!(retired.len(), 3);
    drop(retired);
    assert_eq!(runtime.modules.inner.borrow().cells.len(), 1);
    runtime.gc.graph.lock().finish_sweep().unwrap();
}

#[test]
fn static_cursor_repairs_removals_excludes_prepend_and_rejects_stale_cursor() {
    let mut runtime = runtime();
    let mut ids = Vec::new();
    for name in ["first", "middle", "last"] {
        let installed = runtime
            .load_program(name, program(Ty::Builtin(BuiltinType::I32), true))
            .unwrap();
        let owner = runtime.select_program(&installed).unwrap();
        ids.push(runtime.modules.inner.borrow().records[&owner.key()].bindings[0]);
    }
    let cursor = runtime
        .modules
        .inner
        .borrow_mut()
        .cells
        .begin_sweep()
        .unwrap();
    assert_eq!(
        runtime
            .modules
            .inner
            .borrow()
            .cells
            .sweep_next(&cursor)
            .unwrap(),
        Some(ids[2])
    );
    // Remove a future node first, then the next node; both repairs are required.
    for id in [ids[1], ids[2]] {
        let removed = {
            let mut store = runtime.modules.inner.borrow_mut();
            store
                .cells
                .remove(&id, &mut runtime.gc.graph.lock())
                .unwrap()
                .unwrap()
        };
        drop(removed);
    }
    runtime
        .load_program("newborn", program(Ty::Builtin(BuiltinType::I32), true))
        .unwrap();
    let mut store = runtime.modules.inner.borrow_mut();
    assert_eq!(store.cells.sweep_next(&cursor).unwrap(), Some(ids[0]));
    store.cells.advance_sweep(&cursor, ids[0]).unwrap();
    assert_eq!(store.cells.sweep_next(&cursor).unwrap(), None);
    let next = store.cells.begin_sweep().unwrap();
    assert!(store.cells.sweep_next(&cursor).is_err());
    assert_ne!(store.cells.sweep_next(&next).unwrap(), Some(ids[0]));
    assert_eq!(store.cells.len(), 2);
}

#[test]
fn gray_static_retirement_rejects_without_changing_continuation() {
    let mut runtime = runtime();
    let installed = runtime
        .load_program("gray", program(Ty::Builtin(BuiltinType::I32), true))
        .unwrap();
    let owner = runtime.select_program(&installed).unwrap();
    let id = runtime.modules.inner.borrow().records[&owner.key()].bindings[0];
    let cursor = runtime
        .modules
        .inner
        .borrow_mut()
        .cells
        .begin_sweep()
        .unwrap();
    runtime.gc.graph.lock().begin_cycle().unwrap();
    runtime
        .gc
        .shade_graph_identity(
            &runtime.modules,
            GraphIdentity::Static(StaticScanId {
                owner: owner.registry_owner,
                cell: id,
            }),
        )
        .unwrap();
    {
        let mut store = runtime.modules.inner.borrow_mut();
        assert!(
            store
                .cells
                .remove(&id, &mut runtime.gc.graph.lock())
                .is_err()
        );
        assert_eq!(store.cells.len(), 1);
        assert_eq!(store.cells.sweep_next(&cursor).unwrap(), Some(id));
    }
    runtime.gc.graph.lock().finish_roots().unwrap();
    runtime.gc.drain_graph_work(&runtime.modules).unwrap();
    runtime.gc.graph.lock().close_mark().unwrap();
    runtime.gc.graph.lock().finish_sweep().unwrap();
}

#[rstest]
#[case::before_detachment(false)]
#[case::with_accumulated_outputs(true)]
fn sweep_fault_preserves_caller_outputs_and_report(#[case] detach_first: bool) {
    let mut runtime = runtime();
    let code = program(Ty::Builtin(BuiltinType::I32), true);
    let installed = runtime.load_program("fault", code.clone()).unwrap();
    let staged = runtime
        .stage_reload_program(&installed, None, code)
        .unwrap();
    let key = staged.module().key();
    drop(staged);
    close_mark(&runtime);
    let mut cursor = runtime.modules.begin_sweep().unwrap();
    let mut retired_modules = Vec::new();
    let mut retired_cells = Vec::new();
    let mut report = Vec::new();
    if detach_first {
        runtime
            .modules
            .sweep_programs(
                &mut cursor,
                &mut ScanBudget::new(10),
                1,
                Some(&mut report),
                &mut retired_modules,
            )
            .unwrap();
        runtime
            .modules
            .sweep_cells(&mut cursor, &mut ScanBudget::new(10), 1, &mut retired_cells)
            .unwrap();
        assert_eq!(retired_modules.len(), 1);
        assert_eq!(retired_cells.len(), 1);
        assert_eq!(report, [key]);
    }
    let cell_count = runtime.modules.inner.borrow().cells.len();
    let position = cursor.next;
    let reported = report.clone();
    runtime.gc.graph.lock().fault();
    let mut budget = ScanBudget::new(10);
    assert!(
        runtime
            .modules
            .sweep_programs(
                &mut cursor,
                &mut budget,
                1,
                Some(&mut report),
                &mut retired_modules
            )
            .is_err()
    );
    assert!(
        runtime
            .modules
            .sweep_cells(&mut cursor, &mut budget, 1, &mut retired_cells)
            .is_err()
    );
    assert_eq!(budget.spent(), 0);
    assert_eq!(report, reported);
    assert_eq!(retired_modules.len(), usize::from(detach_first));
    assert_eq!(retired_cells.len(), usize::from(detach_first));
    assert_eq!(cursor.next, position);
    assert_eq!(
        runtime.modules.inner.borrow().records.contains_key(&key),
        !detach_first
    );
    drop((retired_modules, retired_cells));
    assert_eq!(runtime.modules.inner.borrow().cells.len(), cell_count);
}
