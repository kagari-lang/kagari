//! Frame publication and source retirement across incremental root cursors.
use super::{TestFrameMetadata, module, scalar_layout};
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    frame::{
        FrameDispatch, FrameEntry, ReturnDestination,
        arguments::FrameArguments,
        cursor::kernel::RegionExit,
        driver::{ExecutionAction, ReturnAction},
        transfer::ReturnValue,
        types::EnvironmentRecord,
        values::{ExecutionValues, FrameMetadata, scanning::FrameCursor},
    },
    gc::graph::{MarkPhase, objects::ScanEdge},
    module::{
        ExecutableModule, Program,
        execution::{OperandSlot, fields::FieldAccess},
    },
    native::storage::scan::{ScanBudget, ScanStatus},
    test_support::update_programs::bytecode,
    value::Value,
};
use kagari_bytecode::{
    instruction::{Slot, StructId},
    module::CallableTarget,
};

#[rstest::rstest]
#[case::representation(false)]
#[case::bounds(true)]
fn standalone_managed_write_preflight_does_not_quarantine_collection(#[case] out_of_bounds: bool) {
    let (runtime, _program, owner) = module();
    let original = runtime.gc.alloc_string("original".into()).unwrap();
    let replacement = runtime.gc.alloc_string("replacement".into()).unwrap();
    let mut values = ExecutionValues::default();
    let layout = scalar_layout();
    let valid_location = layout.locations[3];
    let mut rejected_location = valid_location;
    if out_of_bounds {
        rejected_location.operand = OperandSlot::new(layout.managed_count, true);
    }
    let slots = values
        .allocate_test(
            &runtime,
            4,
            &[3],
            &FrameArguments::plain(&[original]),
            TestFrameMetadata {
                program: owner,
                environment: None,
                invocation: None,
            },
            Some(layout),
        )
        .unwrap();
    let result = runtime.resources().collect_operation(|| {
        values.set_location(
            &runtime.gc,
            slots,
            rejected_location,
            if out_of_bounds {
                replacement
            } else {
                Value::I32(9)
            },
        )
    });
    assert!(
        !runtime.is_quarantined(),
        "preflight rejection must not attempt execution"
    );
    assert_eq!(result.unwrap(), None);
    assert_eq!(values.with_value(slots, 3, |value| *value), Some(original));
    assert!(runtime.gc.ensure_execution_allowed().is_ok());
    // Passing preflight still reaches the permission check before publication.
    let error = runtime
        .resources()
        .collect_operation(|| {
            assert!(
                values
                    .set_location(&runtime.gc, slots, valid_location, replacement)
                    .is_none()
            );
        })
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert_eq!(
        error.message(),
        "execution attempted during garbage collection"
    );
    assert!(runtime.is_quarantined());
    assert_eq!(values.with_value(slots, 3, |value| *value), Some(original));
    values.release(slots).unwrap();
}

#[test]
fn managed_writes_reject_foreign_kind_and_representation_before_publication() {
    let (runtime, _program, owner) = module();
    let foreign = Runtime::default();
    let original = runtime.gc.alloc_string("original".into()).unwrap();
    let tuple = runtime.gc.alloc_tuple(vec![Value::I32(1)]).unwrap();
    let mut values = ExecutionValues::default();
    let layout = scalar_layout();
    let location = layout.locations[3];
    let slots = values
        .allocate_test(
            &runtime,
            4,
            &[3],
            &FrameArguments::plain(&[original]),
            TestFrameMetadata {
                program: owner,
                environment: None,
                invocation: None,
            },
            Some(layout),
        )
        .unwrap();
    let foreign_string = foreign.gc.alloc_string("foreign".into()).unwrap();
    runtime.gc.graph.lock().begin_cycle().unwrap();
    runtime.gc.graph.lock().finish_roots().unwrap();
    for rejected in [
        foreign_string,
        Value::Str(tuple.direct_reference().unwrap()),
        tuple,
        Value::I32(9),
    ] {
        assert!(
            values
                .set_location(&runtime.gc, slots, location, rejected)
                .is_none()
        );
        let mut operands = values.borrow_operands(&runtime.gc, slots).unwrap();
        assert!(operands.write_location(location, rejected).is_none());
        assert_eq!(operands.read_location(location), Some(original));
    }
    runtime.gc.drain_graph_work(&runtime.modules).unwrap();
    runtime.gc.graph.lock().close_mark().unwrap();
    assert!(
        !runtime
            .gc
            .graph_survivor(&runtime.modules, tuple.heap_identity().unwrap().graph())
            .unwrap()
    );
    assert_eq!(values.with_value(slots, 3, |value| *value), Some(original));
    runtime.gc.graph.lock().finish_sweep().unwrap();
    // Unlike a closed cursor's non-reentrant interval, a new borrow or standalone
    // write must recheck permission after external execution has been quarantined.
    runtime
        .resources()
        .quarantine("injected write admission failure");
    assert!(values.borrow_operands(&runtime.gc, slots).is_none());
    assert!(
        values
            .set_location(&runtime.gc, slots, location, original)
            .is_none()
    );
    assert_eq!(values.with_value(slots, 3, |value| *value), Some(original));
    values.release(slots).unwrap();
}

#[test]
fn admitted_frame_publication_observes_phase_and_preserves_uncommitted_banks() {
    for phase in [
        MarkPhase::Roots,
        MarkPhase::Mark,
        MarkPhase::Sweep,
        MarkPhase::Faulted,
    ] {
        let (runtime, _program, owner) = module();
        let child = runtime.gc.alloc_string("argument".into()).unwrap();
        let identity = runtime.modules.program_scan_identity(&owner).unwrap();
        let mut values = ExecutionValues::default();
        let result = runtime.with_frame_admission(&owner, |program| {
            // Deliberately change phase after validation to prove the admission
            // token does not memoize Idle or substitute validation for shading.
            let mut graph = runtime.gc.graph.lock();
            graph.begin_cycle().unwrap();
            if phase != MarkPhase::Roots {
                graph.finish_roots().unwrap();
            }
            if phase == MarkPhase::Sweep {
                graph.close_mark().unwrap();
            } else if phase == MarkPhase::Faulted {
                graph.fault();
            }
            drop(graph);
            values.allocate(
                1,
                &[0],
                &FrameArguments::plain(&[child]),
                FrameMetadata {
                    program,
                    environment: None,
                    invocation: None,
                },
                None,
            )
        });
        if matches!(phase, MarkPhase::Roots | MarkPhase::Mark) {
            let slots = result.unwrap();
            assert_eq!(values.with_value(slots, 0, |value| *value), Some(child));
            if phase == MarkPhase::Roots {
                runtime.gc.graph.lock().finish_roots().unwrap();
            }
            runtime.gc.drain_graph_work(&runtime.modules).unwrap();
            runtime.gc.graph.lock().close_mark().unwrap();
            assert!(
                runtime
                    .gc
                    .graph_survivor(&runtime.modules, identity)
                    .unwrap()
            );
            assert!(
                runtime
                    .gc
                    .graph_survivor(&runtime.modules, child.heap_identity().unwrap().graph())
                    .unwrap()
            );
        } else {
            assert!(result.is_err());
            assert_eq!(values.active_windows(), 0);
            assert_eq!(values.next_generation, 0);
            assert!(values.values.is_empty());
            assert!(values.payloads.is_empty());
            assert!(values.initialized.is_empty());
        }
    }
}

#[test]
fn module_admission_still_precedes_environment_and_bank_failure_with_depth_rollback() {
    let (runtime, program, owner) = source_module("pub fn hold(value: i32) -> i32 { value }");
    let candidate = runtime
        .stage_reload_verified_program(&program, None, owner.verified_program().clone())
        .unwrap();
    let stale = candidate.module().clone();
    drop(candidate);
    let foreign = Runtime::default();
    let environment = foreign
        .alloc_environment(
            EnvironmentRecord::new(foreign.definition_context(), vec![], vec![]).unwrap(),
        )
        .unwrap();
    let stack = runtime.enter_execution_stack(&owner).unwrap();
    let entry = owner.bytecode.functions[0].id;
    runtime
        .resources()
        .frame_values
        .borrow_mut()
        .next_generation = u64::MAX;
    for (callee, expected) in [
        (stale, RuntimeErrorKind::StaleHandle),
        (owner.clone(), RuntimeErrorKind::ModuleValidation),
    ] {
        let error = stack
            .push_admitted_arguments(
                &runtime,
                callee,
                CallableTarget::Script(entry),
                FrameArguments::plain(&[Value::I32(1)]),
                None,
                FrameDispatch {
                    prepared: None,
                    entry: FrameEntry::Call,
                    invocation: None,
                    environment: Some(environment.clone()),
                },
            )
            .unwrap_err();
        assert_eq!(error.kind(), expected);
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(
            runtime.resources().frame_values.borrow().active_windows(),
            0
        );
        assert_eq!(
            runtime.resources().frame_values.borrow().next_generation,
            u64::MAX
        );
    }
}

#[test]
fn root_cursor_survives_compaction_reuse_and_direct_operand_publication() {
    let (runtime, _program, module) = module();
    let old = runtime.gc.alloc_string("old frame".into()).unwrap();
    let replacement = runtime.gc.alloc_string("direct write".into()).unwrap();
    let operand = runtime.gc.alloc_string("operand write".into()).unwrap();
    let reused_value = runtime.gc.alloc_string("reused window".into()).unwrap();
    let allocate = |values: &mut ExecutionValues, value| {
        values
            .allocate_test(
                &runtime,
                4,
                &[3],
                &FrameArguments::plain(&[value]),
                TestFrameMetadata {
                    program: module.clone(),
                    environment: None,
                    invocation: None,
                },
                Some(scalar_layout()),
            )
            .unwrap()
    };
    let (first, second, mut cursor) = {
        let mut values = runtime.resources().frame_values.borrow_mut();
        let first = allocate(&mut values, old);
        let second = allocate(&mut values, old);
        (first, second, values.begin_root_scan())
    };
    runtime.gc.graph.lock().begin_cycle().unwrap();
    let mut visit = |edge: ScanEdge<'_>, budget: &mut ScanBudget| {
        runtime
            .gc
            .shade_scanned_edge(&runtime.modules, edge, budget)
    };
    // Both windows have one managed slot. Advance past the second value, but
    // leave its full identity active while the preceding bank range is removed.
    assert_eq!(
        runtime
            .resources()
            .frame_values
            .borrow()
            .scan_root_slice(&mut cursor, &mut ScanBudget::new(25), &mut visit)
            .unwrap(),
        ScanStatus::Pending
    );
    let reused = {
        let mut values = runtime.resources().frame_values.borrow_mut();
        values.release(first).unwrap();
        let location = scalar_layout().location(3).unwrap();
        values
            .set_location(&runtime.gc, second, location, replacement)
            .unwrap();
        values
            .borrow_operands(&runtime.gc, second)
            .unwrap()
            .write_location(location, operand)
            .unwrap();
        allocate(&mut values, reused_value)
    };
    for _ in 0..32 {
        if runtime
            .resources()
            .frame_values
            .borrow()
            .scan_root_slice(&mut cursor, &mut ScanBudget::new(1), &mut visit)
            .unwrap()
            == ScanStatus::Complete
        {
            break;
        }
    }
    runtime.gc.graph.lock().finish_roots().unwrap();
    runtime.gc.drain_graph_work(&runtime.modules).unwrap();
    runtime.gc.graph.lock().close_mark().unwrap();
    for value in [old, replacement, operand, reused_value] {
        assert!(
            runtime
                .gc
                .graph_survivor(&runtime.modules, value.heap_identity().unwrap().graph())
                .unwrap()
        );
    }
    runtime.gc.graph.lock().finish_sweep().unwrap();
    let mut values = runtime.resources().frame_values.borrow_mut();
    values.release(second).unwrap();
    values.release(reused).unwrap();
}

pub(super) fn source_module(source: &str) -> (Runtime, Program, ExecutableModule) {
    let mut runtime = Runtime::default();
    let program = runtime
        .load_program("publication", bytecode(source, None))
        .unwrap();
    let owner = runtime.select_program(&program).unwrap();
    (runtime, program, owner)
}

fn scan_frames(runtime: &Runtime, cursor: &mut FrameCursor, work: usize) -> ScanStatus {
    runtime
        .resources()
        .frame_values
        .borrow()
        .scan_root_slice(cursor, &mut ScanBudget::new(work), &mut |edge, budget| {
            runtime
                .gc
                .shade_scanned_edge(&runtime.modules, edge, budget)
        })
        .unwrap()
}

fn close_frame_cycle(runtime: &Runtime, cursor: &mut FrameCursor, retained: Value) {
    assert!((0..1000).any(|_| scan_frames(runtime, cursor, 5) == ScanStatus::Complete));
    runtime.gc.graph.lock().finish_roots().unwrap();
    runtime.gc.drain_graph_work(&runtime.modules).unwrap();
    runtime.gc.graph.lock().close_mark().unwrap();
    assert!(
        runtime
            .gc
            .graph_survivor(&runtime.modules, retained.heap_identity().unwrap().graph())
            .unwrap(),
        "published frame value must survive"
    );
    runtime.gc.graph.lock().finish_sweep().unwrap();
}

#[test]
fn scoped_field_read_survives_removal_before_parent_scan() {
    let (runtime, _program, owner) = source_module(
        "pub struct Holder { var value: (i32, i32) } pub fn read(holder: Holder) -> (i32, i32) { holder.value }",
    );
    let entry = owner
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "read")
        .unwrap()
        .id;
    let child = runtime
        .gc
        .alloc_tuple(vec![Value::I32(1), Value::I32(2)])
        .unwrap();
    let replacement = runtime
        .gc
        .alloc_tuple(vec![Value::I32(3), Value::I32(4)])
        .unwrap();
    let layout = owner.struct_layout(StructId::new(0)).unwrap();
    let parent = runtime.alloc_struct(layout.clone(), vec![child]).unwrap();
    let stack = runtime.enter_execution_stack(&owner).unwrap();
    stack
        .push(
            &runtime,
            owner.slot(),
            entry,
            &[Value::Struct(parent)],
            None,
        )
        .unwrap();
    let (slots, destination) = {
        let frame = stack.current().unwrap();
        let operation = frame.prepared_field(0).unwrap();
        let FieldAccess::Read { dst } = operation.access else {
            panic!("field-read fixture");
        };
        // Supply the receiver operand normally installed by the preceding move.
        frame
            .slots
            .set_location(&runtime.gc, operation.base, Value::Struct(parent))
            .unwrap();
        (frame.slots, dst)
    };
    runtime.gc.graph.lock().begin_cycle().unwrap();
    let mut cursor = runtime.resources().frame_values.borrow().begin_root_scan();
    assert_eq!(
        scan_frames(&runtime, &mut cursor, usize::MAX),
        ScanStatus::Complete
    );
    // P is gray; C is white. Exercise the actual scoped transition, not a
    // synthetic bank store, then remove P's only edge before its first scan.
    assert!(matches!(
        stack.execute_scoped_field(&runtime, 0).unwrap(),
        RegionExit::Safepoint
    ));
    runtime
        .gc
        .struct_set_slot(parent, &layout, 0, replacement)
        .unwrap();
    assert_eq!(
        slots.with_location(&runtime.gc, destination, |value| *value),
        Some(child)
    );
    close_frame_cycle(&runtime, &mut cursor, child);
    drop(stack);
    assert_eq!(
        runtime.resources().frame_values.borrow().active_windows(),
        0
    );
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn return_transfer_survives_retirement_ahead_of_root_cursor() {
    for prepared in [false, true] {
        let (runtime, _program, owner) =
            source_module("pub fn hold(value: (i32, i32)) -> (i32, i32) { value }");
        let entry = owner.bytecode.functions[0].id;
        let before = runtime
            .gc
            .alloc_tuple(vec![Value::I32(1), Value::I32(2)])
            .unwrap();
        let child = runtime
            .gc
            .alloc_tuple(vec![Value::I32(3), Value::I32(4)])
            .unwrap();
        let stack = runtime.enter_execution_stack(&owner).unwrap();
        stack
            .push(&runtime, owner.slot(), entry, &[before], None)
            .unwrap();
        let caller = stack.current().unwrap().slots;
        let (destination, work) = {
            let values = runtime.resources().frame_values.borrow();
            let window = values.window(caller).unwrap();
            (window.location(0).unwrap(), 8 + window.ranges.managed.len())
        };
        stack
            .push(&runtime, owner.slot(), entry, &[child], Some(Slot::new(0)))
            .unwrap();
        if prepared {
            stack.current_mut().unwrap().return_to = ReturnDestination::Prepared(Some(destination));
        }
        runtime.gc.graph.lock().begin_cycle().unwrap();
        let mut cursor = runtime.resources().frame_values.borrow().begin_root_scan();
        assert_eq!(
            scan_frames(&runtime, &mut cursor, work),
            ScanStatus::Pending
        );
        assert_eq!(
            stack
                .finish_return(&runtime, ReturnValue::general(child))
                .unwrap(),
            ReturnAction::Continue(ExecutionAction::Script)
        );
        assert_eq!(caller.get(&runtime.gc, 0), Some(child));
        assert_eq!(
            runtime.resources().frame_values.borrow().active_windows(),
            1
        );
        assert_eq!(runtime.resources().counters().current_call_depth, 1);
        close_frame_cycle(&runtime, &mut cursor, child);
        drop(stack);
        assert_eq!(
            runtime.resources().frame_values.borrow().active_windows(),
            0
        );
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
    }
}

#[test]
fn rejected_return_publication_preserves_callee_and_caller_until_cleanup() {
    for fail_barrier in [false, true] {
        let (runtime, _program, owner) =
            source_module("pub fn hold(value: (i32, i32)) -> (i32, i32) { value }");
        let entry = owner.bytecode.functions[0].id;
        let before = runtime
            .gc
            .alloc_tuple(vec![Value::I32(1), Value::I32(2)])
            .unwrap();
        let child = runtime
            .gc
            .alloc_tuple(vec![Value::I32(3), Value::I32(4)])
            .unwrap();
        let stack = runtime.enter_execution_stack(&owner).unwrap();
        stack
            .push(&runtime, owner.slot(), entry, &[before], None)
            .unwrap();
        let caller = stack.current().unwrap().slots;
        stack
            .push(&runtime, owner.slot(), entry, &[child], Some(Slot::new(0)))
            .unwrap();
        let callee = stack.current().unwrap().slots;
        // Idle publication intentionally needs no graph lock. During marking a
        // conflicting borrow fails shading; a wrong representation fails admission.
        // Neither may overwrite the caller or retire the source root.
        if fail_barrier {
            runtime.gc.graph.lock().begin_cycle().unwrap();
        }
        let graph = fail_barrier.then(|| runtime.gc.graph.lock());
        let value = if fail_barrier { child } else { Value::I32(1) };
        assert!(
            stack
                .finish_return(&runtime, ReturnValue::general(value))
                .is_err()
        );
        drop(graph);
        let values = runtime.resources().frame_values.borrow();
        assert_eq!(values.with_value(caller, 0, |value| *value), Some(before));
        assert_eq!(values.with_value(callee, 0, |value| *value), Some(child));
        assert_eq!(values.active_windows(), 2);
        drop(values);
        assert_eq!(runtime.resources().counters().current_call_depth, 2);
        drop(stack);
        assert_eq!(
            runtime.resources().frame_values.borrow().active_windows(),
            0
        );
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
    }
}
