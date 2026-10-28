//! Phase-local public observations; raw service pauses retain nested indivisible time.
use kagari::runtime::Runtime;
use kagari_runtime::gc::metrics::{CollectionObservation, IndivisibleKind, WorkMeasurements};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scope {
    Automatic,
    Explicit,
}

pub(super) struct Phase {
    label: &'static str,
    scope: Scope,
    elapsed_ns: u128,
    objects_before: usize,
    objects_after: usize,
    observation: CollectionObservation,
}

pub(super) struct Record {
    phases: Vec<Phase>,
}

impl Record {
    pub(super) fn new() -> Self {
        Self {
            phases: Vec::with_capacity(20),
        }
    }

    pub(super) fn measure<T>(
        &mut self,
        label: &'static str,
        scope: Scope,
        runtime: &mut Runtime,
        action: impl FnOnce(&mut Runtime) -> T,
    ) -> T {
        // The first start allocates observation backing; later starts reuse it.
        // Both start/reset and the snapshot copy are outside the action timer.
        runtime
            .runtime()
            .gc()
            .start_collection_observation()
            .unwrap();
        let before = runtime.runtime().gc().stats();
        let start = Instant::now();
        let result = action(runtime);
        let elapsed_ns = start.elapsed().as_nanos();
        let observation = runtime.runtime().gc().collection_observation().unwrap();
        let after = runtime.runtime().gc().stats();
        validate(&observation, scope, elapsed_ns);
        assert_eq!(
            observation.cycles.count + observation.carried_cycles.count,
            after.collections - before.collections,
            "{label}: every completed cycle must be reported"
        );
        self.phases.push(Phase {
            label,
            scope,
            elapsed_ns,
            objects_before: before.allocated_objects,
            objects_after: after.allocated_objects,
            observation,
        });
        result
    }

    pub(super) fn last_observation(&self) -> CollectionObservation {
        self.phases
            .last()
            .expect("completed observation phase")
            .observation
    }

    pub(super) fn finish(&self, runtime: &Runtime) {
        let final_snapshot = runtime
            .runtime()
            .gc()
            .finish_collection_observation()
            .unwrap();
        assert_eq!(
            final_snapshot,
            self.phases.last().unwrap().observation,
            "no unreported collection between final phase and observation release"
        );
        assert!(runtime.runtime().gc().collection_observation().is_none());
    }

    fn normal_count(&self) -> u64 {
        self.phases
            .iter()
            .map(|phase| phase.observation.normal.count)
            .sum()
    }
}

fn validate(observation: &CollectionObservation, scope: Scope, elapsed_ns: u128) {
    for work in [
        observation.normal,
        observation.pressure_assist,
        observation.explicit,
        observation.cycles,
        observation.carried_cycles,
    ] {
        assert_eq!(work.failures, 0);
        assert!(work.last_work <= work.max_work);
        assert!(work.max_work as u128 <= work.total_work);
        assert!(work.last_time <= work.max_time);
        assert!(work.max_time <= work.total_time);
    }
    let services = [
        observation.normal,
        observation.pressure_assist,
        observation.explicit,
    ];
    assert!(
        services
            .iter()
            .map(|s| s.total_time.as_nanos())
            .sum::<u128>()
            <= elapsed_ns
    );
    assert_eq!(
        observation.slices.total_work,
        services.iter().map(|s| s.total_work).sum::<u128>()
    );
    assert!(observation.carried_cycles.count <= 1);
    assert!(observation.active_cycle_at_start || observation.carried_cycles.count == 0);
    match scope {
        Scope::Automatic => assert_eq!(observation.explicit.count, 0),
        Scope::Explicit => {
            assert_eq!(observation.explicit.count, 1);
            assert_eq!(observation.normal.count, 0);
            assert_eq!(observation.pressure_assist.count, 0);
        }
    }
    for operation in observation.indivisible {
        assert_eq!(operation.count == 0, operation.slowest.is_none());
        assert!(operation.unknown_size_count <= operation.count);
        if let Some(slowest) = operation.slowest {
            assert!(slowest.time <= operation.total_time);
            assert!(slowest.items as u128 <= operation.total_items);
            assert!(slowest.work as u128 <= operation.total_work);
        }
    }
}

fn work_row(prefix: &str, kind: &str, work: WorkMeasurements) {
    println!(
        "auto_work,{prefix},kind={kind},count={},failures={},total_work={},last_work={},max_work={},total_ns={},last_ns={},max_ns={}",
        work.count,
        work.failures,
        work.total_work,
        work.last_work,
        work.max_work,
        work.total_time.as_nanos(),
        work.last_time.as_nanos(),
        work.max_time.as_nanos(),
    );
}

pub(super) fn run(
    policy: &str,
    family: &str,
    check_only: bool,
    require_activity: bool,
    action: impl Fn() -> Record,
) {
    let warmups = if check_only { 0 } else { 3 };
    let samples = if check_only { 1 } else { 11 };
    for _ in 0..warmups {
        let record = action();
        if require_activity {
            assert!(
                record.normal_count() > 0,
                "{family}: no automatic normal work"
            );
        }
    }
    let records: Vec<_> = (0..samples).map(|_| action()).collect();
    for (sample, record) in records.iter().enumerate() {
        if require_activity {
            assert!(
                record.normal_count() > 0,
                "{family}: no automatic normal work"
            );
        }
        for phase in &record.phases {
            let prefix = format!("{policy},{family},phase={},sample={sample}", phase.label);
            let observed = phase.observation;
            println!(
                "auto_phase,{prefix},scope={:?},elapsed_ns={},objects_before={},objects_after={},active_cycle_at_start={}",
                phase.scope,
                phase.elapsed_ns,
                phase.objects_before,
                phase.objects_after,
                observed.active_cycle_at_start,
            );
            for (kind, work) in [
                ("normal", observed.normal),
                ("pressure_assist", observed.pressure_assist),
                ("explicit", observed.explicit),
                ("cycles", observed.cycles),
                ("carried_cycles", observed.carried_cycles),
            ] {
                work_row(&prefix, kind, work);
            }
            println!(
                "auto_slices,{prefix},count={},total_work={},last_work={},max_work={}",
                observed.slices.count,
                observed.slices.total_work,
                observed.slices.last_work,
                observed.slices.max_work,
            );
            for kind in IndivisibleKind::ALL {
                let operation = observed.indivisible[kind as usize];
                println!(
                    "auto_indivisible,{prefix},kind={kind:?},count={},items={},work={},known_bytes={},unknown_size_count={},total_ns={},max_ns={},slowest={:?}",
                    operation.count,
                    operation.total_items,
                    operation.total_work,
                    operation.total_known_bytes,
                    operation.unknown_size_count,
                    operation.total_time.as_nanos(),
                    operation.slowest.map_or(0, |sample| sample.time.as_nanos()),
                    operation.slowest,
                );
            }
        }
    }
    let phase_count = records[0].phases.len();
    assert!(
        records
            .iter()
            .all(|record| record.phases.len() == phase_count)
    );
    for index in 0..phase_count {
        let phase = &records[0].phases[index];
        let maxima: Vec<_> = records
            .iter()
            .map(|record| {
                assert_eq!(record.phases[index].label, phase.label);
                record.phases[index].observation.normal.max_time.as_nanos()
            })
            .collect();
        let counts: Vec<_> = records
            .iter()
            .map(|record| record.phases[index].observation.normal.count)
            .collect();
        println!(
            "auto_summary,{policy},{family},phase={},scope={:?},samples={samples},warmups={warmups},normal_count={},normal_max_ns={},sample_counts={counts:?},sample_maxima_ns={maxima:?}",
            phase.label,
            phase.scope,
            counts.iter().sum::<u64>(),
            maxima.iter().max().unwrap(),
        );
    }
    let normal_count: u64 = records.iter().map(Record::normal_count).sum();
    println!(
        "auto_check,{policy},{family},mode={},samples={samples},warmups={warmups},normal_count={normal_count},active={},required={require_activity},semantic_checks=passed,observation_checks=passed",
        if check_only { "check" } else { "measurement" },
        normal_count > 0,
    );
}
