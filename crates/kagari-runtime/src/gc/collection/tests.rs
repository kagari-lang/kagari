use super::*;
use crate::{
    Runtime, RuntimeConfig,
    gc::{GcHeapConfig, metrics::observation_timer_reads},
    test_support::layouts::allocation_owner,
};

fn runtime(threshold: Option<usize>) -> Runtime {
    let runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: threshold,
            retained_byte_threshold: usize::MAX,
        },
        ..RuntimeConfig::default()
    });
    runtime.gc().start_collection_observation().unwrap();
    runtime
}

fn garbage(runtime: &Runtime, count: usize) {
    for _ in 0..count {
        runtime.gc().alloc_string("unreachable".into()).unwrap();
    }
}

#[test]
fn observation_windows_reset_maxima_and_separate_a_carried_cycle() {
    let runtime = runtime(Some(1500));
    assert!(runtime.gc.finish_collection_observation().is_some());
    assert!(runtime.gc.collection_observation().is_none());
    runtime.gc.start_collection_observation().unwrap();
    garbage(&runtime, 2048);
    runtime.gc_safepoint().unwrap();
    let first = runtime.gc.collection_observation().unwrap();
    assert_eq!(first.normal.count, 1);
    assert_ne!(runtime.gc.graph.phase(), MarkPhase::Idle);
    runtime.gc.start_collection_observation().unwrap();
    let reset = runtime.gc.collection_observation().unwrap();
    assert!(reset.active_cycle_at_start);
    assert_eq!(reset.normal.count, 0);
    assert_eq!(reset.normal.max_time, Duration::ZERO);
    runtime.collect_garbage().unwrap();
    let finished = runtime.gc.finish_collection_observation().unwrap();
    assert_eq!(finished.normal.count, 0);
    assert_eq!(finished.explicit.count, 1);
    assert_eq!(finished.carried_cycles.count, 1);
    assert_eq!(finished.cycles.count, 1);
    assert_eq!(
        finished.carried_cycles.total_work + finished.cycles.total_work,
        first.normal.total_work + finished.explicit.total_work
    );
    assert!(runtime.gc.collection_observation().is_none());
    runtime.gc.start_collection_observation().unwrap();
    runtime.gc_safepoint().unwrap();
    let empty = runtime.gc.collection_observation().unwrap();
    assert!(!empty.active_cycle_at_start);
    assert_eq!(
        empty.normal.count, 0,
        "no-op safepoints are not normal collection samples"
    );
}

#[test]
fn observation_window_changes_are_excluded_during_an_admitted_invocation() {
    let runtime = runtime(None);
    let invocation = Invocation::new(&runtime.gc, None);
    assert!(runtime.gc.start_collection_observation().is_err());
    assert!(runtime.gc.finish_collection_observation().is_none());
    assert!(runtime.gc.collection_observation().is_some());
    invocation.finish(&runtime.gc, ServiceKind::Explicit, false);
    assert_eq!(
        runtime
            .gc
            .finish_collection_observation()
            .unwrap()
            .explicit
            .count,
        1
    );
}

#[test]
fn physical_drop_observations_count_individual_payloads_not_only_batch_maxima() {
    let runtime = runtime(None);
    runtime.gc.finish_collection_observation().unwrap();
    garbage(&runtime, 512);
    drop(runtime.root_value(Value::Unit).unwrap());
    runtime
        .gc
        .alloc_operation_group(runtime.modules(), vec![])
        .unwrap();
    let before = observation_timer_reads();
    let unobserved = runtime.collect_garbage().unwrap();
    assert_eq!(
        observation_timer_reads(),
        before,
        "disabled observations must not read any observation clock"
    );
    assert_eq!(unobserved.reclaimed_objects, 512);
    assert!(
        !unobserved.pause.is_zero(),
        "public invocation timing remains enabled"
    );
    assert_eq!(runtime.gc.stats().last_pause, unobserved.pause);
    assert!(runtime.gc.collection_observation().is_none());
    runtime.gc.start_collection_observation().unwrap();
    garbage(&runtime, 512);
    drop(runtime.root_value(Value::Unit).unwrap());
    runtime
        .gc
        .alloc_operation_group(runtime.modules(), vec![])
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(
        observation_timer_reads() >= before + 1024,
        "each of the 512 observed destructors reads start and end"
    );
    let observation = runtime.gc.collection_observation().unwrap();
    for kind in [
        IndivisibleKind::RootDrop,
        IndivisibleKind::MetadataDrop,
        IndivisibleKind::DescriptorDrop,
    ] {
        assert!(
            observation.indivisible[kind as usize].count > 0,
            "observed {kind:?}"
        );
    }
    let drops = observation.indivisible[IndivisibleKind::PayloadDrop as usize];
    assert_eq!(drops.count, 512);
    assert_eq!(drops.total_items, 512);
    assert!(drops.total_known_bytes > 0);
    assert_eq!(drops.unknown_size_count, 0);
    assert_eq!(drops.slowest.unwrap().owner, "String");
    assert!(drops.total_time >= drops.slowest.unwrap().time);
    assert!(drops.total_time <= observation.explicit.total_time);
    assert_eq!(observation.normal.count, 0);
    assert_eq!(observation.explicit.count, 1);
    assert_eq!(observation.cycles.count, 1);
    assert_eq!(
        observation.cycles.total_time,
        observation.explicit.total_time
    );
    assert_eq!(
        observation.indivisible[IndivisibleKind::LifecycleDrop as usize].count,
        1
    );
}

#[test]
fn automatic_budget_spans_mark_sweep_and_disposal_and_reports_only_new_retirements() {
    let runtime = runtime(Some(1500));
    // Exact directory capacity avoids the separately tested allocator-compaction exception.
    const OBJECTS: usize = 2048;
    garbage(&runtime, OBJECTS);
    runtime.gc_safepoint().unwrap();
    let first = runtime.gc().stats();
    assert_eq!(first.collections, 0);
    assert!(first.reclaimed_objects > 0);
    assert!(first.reclaimed_objects < OBJECTS);
    let automatic = runtime.gc.collection_observation().unwrap();
    assert_eq!(automatic.normal.count, 1);
    assert_eq!(automatic.explicit.count, 0);
    assert_eq!(automatic.pressure_assist.count, 0);
    assert!(
        runtime
            .gc
            .collection_observation()
            .unwrap()
            .normal
            .last_work
            <= NORMAL_WORK
    );
    // Explicit completion includes the remainder of the active cycle and a fresh
    // cycle, but cannot report the objects already reclaimed by the first step.
    let report = runtime.collect_garbage().unwrap();
    assert_eq!(report.reclaimed_objects, OBJECTS - first.reclaimed_objects);
    assert_eq!(report.live_objects, 0);
    assert_eq!(runtime.gc().stats().collections, 2);
    assert_eq!(runtime.gc().stats().reclaimed_objects, OBJECTS);
    assert_eq!(runtime.gc().stats().last_pause, report.pause);
    let finished = runtime.gc.collection_observation().unwrap();
    assert_eq!(
        finished.normal, automatic.normal,
        "explicit slices cannot contaminate normal maxima"
    );
    assert_eq!(finished.explicit.count, 1);
    assert_eq!(finished.explicit.last_time, report.pause);
    assert_eq!(finished.cycles.count, 2);
    assert_eq!(
        finished.cycles.total_work,
        finished.normal.total_work + finished.explicit.total_work
    );
    assert!(
        finished.cycles.total_time <= finished.normal.total_time + finished.explicit.total_time
    );
    assert!(finished.normal.max_work <= NORMAL_WORK);
    assert!(finished.slices.max_work > NORMAL_WORK);
    assert_eq!(
        finished.slices.count,
        automatic.slices.count + 2,
        "explicit drains the active remainder and fresh cycle without normal-service yields"
    );
}

#[test]
fn resumed_sweep_rechecks_later_store_borrows_before_any_further_detachment() {
    for explicit in [false, true] {
        let runtime = runtime(Some(1500));
        garbage(&runtime, 2048);
        runtime.gc_safepoint().unwrap();
        assert_eq!(runtime.gc.graph.phase(), MarkPhase::Sweep);
        assert!(runtime.gc().stats().reclaimed_objects > 0);
        assert!(runtime.gc().stats().allocated_objects > 0);
        // This newborn survives the active sweep. Its view is acquired only after
        // entry preflight, while physical owners still await the next service.
        let group = runtime
            .gc
            .alloc_operation_group(runtime.modules(), vec![])
            .unwrap();
        let view = runtime.gc.operation_group(group).unwrap();
        let before = runtime.gc().stats();
        let counters = runtime.resources().counters();
        if explicit {
            assert!(runtime.collect_garbage().is_err());
        } else {
            assert!(runtime.gc_safepoint().is_err());
        }
        assert_eq!(
            runtime.gc().stats(),
            before,
            "no earlier domain may retire before a borrowed later owner rejects admission"
        );
        assert_eq!(runtime.resources().counters(), counters);
        assert!(runtime.is_quarantined());
        let observation = runtime.gc.collection_observation().unwrap();
        assert_eq!(observation.explicit.failures, u64::from(explicit));
        assert_eq!(observation.normal.failures, u64::from(!explicit));
        assert!(runtime.gc.start_collection_observation().is_err());
        drop(view);
        assert!(runtime.gc.operation_group(group).is_some());
    }
}

#[test]
fn indivisible_directory_compaction_reports_its_work_and_duration() {
    let runtime = runtime(Some(1500));
    garbage(&runtime, 2000);
    runtime.collect_garbage().unwrap();
    let service = runtime.gc.collection_observation().unwrap();
    assert!(
        service.slices.max_work >= 2000,
        "copied directory slots are not hidden"
    );
    let compaction = service.indivisible[IndivisibleKind::DirectoryCompaction as usize];
    assert_eq!(compaction.count, 1);
    assert_eq!(compaction.total_work, 2000);
    assert!(compaction.slowest.unwrap().time > Duration::ZERO);
    assert!(service.cycles.last_work >= service.slices.max_work);
    assert!(service.cycles.last_time <= service.explicit.last_time);
}

#[test]
fn explicit_fresh_cycle_reclaims_a_root_lost_after_initial_discovery() {
    let runtime = runtime(Some(1));
    let value = runtime
        .gc()
        .alloc_string("root lost between slices".into())
        .unwrap();
    let root = runtime.root_value(value).unwrap();
    // Scalar roots keep enumeration active after the first root was discovered.
    let roots: Vec<_> = (0..800)
        .map(|_| runtime.root_value(Value::I32(0)).unwrap())
        .collect();
    runtime.gc_safepoint().unwrap();
    assert_eq!(runtime.gc.graph.phase(), MarkPhase::Roots);
    drop(root);
    let report = runtime.collect_garbage().unwrap();
    assert_eq!(report.reclaimed_objects, 1);
    assert_eq!(report.live_objects, 0);
    assert_eq!(runtime.gc().stats().collections, 2);
    drop(roots);
}

#[test]
fn disabled_automatic_policy_freezes_active_work_but_not_explicit_collection() {
    let mut runtime = runtime(Some(1500));
    garbage(&runtime, 2000);
    runtime.gc_safepoint().unwrap();
    assert_ne!(runtime.gc.graph.phase(), MarkPhase::Idle);
    runtime.gc.config.collection_threshold = None;
    let before = runtime.gc().stats();
    for _ in 0..3 {
        runtime.gc_safepoint().unwrap();
    }
    assert_eq!(runtime.gc().stats(), before);
    assert_ne!(runtime.gc.graph.phase(), MarkPhase::Idle);
    let report = runtime.collect_garbage().unwrap();
    assert_eq!(report.reclaimed_objects, 2000 - before.reclaimed_objects);
    assert_eq!(runtime.gc.graph.phase(), MarkPhase::Idle);
}

#[test]
fn active_cycle_continues_below_its_start_threshold() {
    let runtime = runtime(Some(1500));
    garbage(&runtime, 2000);
    // These 2000 strings carry 24000 heap units; crossing 1500 is not a 32-service
    // contract. Allow 64 services for bounded workspace/fragment work, but still
    // require crossing before cycle completion and continued automatic progress.
    for _ in 0..64 {
        runtime.gc_safepoint().unwrap();
        if runtime.gc().stats().current_heap_units < 1500 {
            break;
        }
    }
    assert!(runtime.gc().stats().current_heap_units < 1500);
    assert_ne!(runtime.gc.graph.phase(), MarkPhase::Idle);
    let before = runtime.gc.collection_observation().unwrap().normal.count;
    runtime.gc_safepoint().unwrap();
    assert!(runtime.gc.collection_observation().unwrap().normal.count > before);
    for _ in 0..128 {
        if runtime.gc.graph.phase() == MarkPhase::Idle {
            break;
        }
        runtime.gc_safepoint().unwrap();
    }
    assert_eq!(runtime.gc.graph.phase(), MarkPhase::Idle);
    assert_eq!(runtime.gc().stats().collections, 1);
    assert_eq!(runtime.gc().stats().allocated_objects, 0);
    let observation = runtime.gc.collection_observation().unwrap();

    assert_eq!(observation.cycles.count, 1);
    assert_eq!(observation.pressure_assist.count, 0);
    assert_eq!(observation.explicit.count, 0);
    assert_eq!(
        observation.cycles.total_time, observation.normal.total_time,
        "every complete service interval belongs to the cycle; no mutator gap or missing completion tail"
    );
    assert_eq!(observation.cycles.total_work, observation.normal.total_work);
}

#[test]
fn metadata_only_admission_and_backing_growth_each_trigger_their_captured_threshold() {
    let mut metadata = runtime(Some(1));
    garbage(&metadata, 2000);
    metadata.gc_safepoint().unwrap();
    let before = metadata.resources().committed_pressure();
    let _owner = allocation_owner(&mut metadata);
    assert_eq!(metadata.resources().committed_pressure(), before);
    metadata.gc_safepoint().unwrap();
    assert_eq!(metadata.gc.graph.phase(), MarkPhase::Idle);
    assert_eq!(
        metadata
            .gc
            .collection_observation()
            .unwrap()
            .pressure_assist
            .count,
        1
    );

    let mut backing = runtime(Some(1500));
    backing.gc.config.retained_byte_threshold = 1024;
    backing.gc.next_byte_collection.set(1024);
    garbage(&backing, 2000);
    backing.gc_safepoint().unwrap();
    let newborn = backing.gc().alloc_string("x".repeat(2048)).unwrap();
    backing.gc_safepoint().unwrap();
    assert_eq!(backing.gc.graph.phase(), MarkPhase::Idle);
    assert_eq!(
        backing
            .gc
            .collection_observation()
            .unwrap()
            .pressure_assist
            .count,
        1
    );
    assert!(backing.gc().validate_value(&newborn));
    assert_eq!(backing.collect_garbage().unwrap().reclaimed_objects, 1);
}

#[test]
fn committed_growth_escalates_even_when_released_before_the_next_boundary() {
    let runtime = runtime(Some(1500));
    garbage(&runtime, 2000);
    runtime.gc_safepoint().unwrap();
    let before = runtime.gc().stats().current_heap_units;
    let automatic = runtime.gc.collection_observation().unwrap().normal;
    let temporary = runtime.resources().reserve_temporary_heap(1500).unwrap();
    drop(temporary);
    assert_eq!(runtime.gc().stats().current_heap_units, before);
    runtime.gc_safepoint().unwrap();
    assert_eq!(runtime.gc.graph.phase(), MarkPhase::Idle);
    assert_eq!(
        runtime.gc().stats().collections,
        1,
        "assist finishes only the active cycle"
    );
    let finished = runtime.gc.collection_observation().unwrap();
    assert_eq!(finished.normal, automatic);
    assert!(finished.pressure_assist.last_work > NORMAL_WORK);
    assert!(finished.normal.max_work <= NORMAL_WORK);
    assert_eq!(
        finished.slices.count, 2,
        "one normal service and one assist drain, not repeated 1024-unit slices"
    );
    assert_eq!(
        finished.pressure_assist.total_time,
        finished.pressure_assist.last_time
    );
    assert_eq!(
        finished.cycles.total_time,
        finished.normal.total_time + finished.pressure_assist.total_time
    );
    assert_eq!(
        finished.cycles.total_work,
        finished.normal.total_work + finished.pressure_assist.total_work
    );
    assert_eq!(
        runtime
            .gc
            .collection_observation()
            .unwrap()
            .pressure_assist
            .count,
        1
    );
}
