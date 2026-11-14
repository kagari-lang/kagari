//! Failure preserves completed work and releases every temporary root/lease.
use super::*;
use kagari_runtime::{Runtime, module::entry::ProgramEntry};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use std::sync::atomic::Ordering;

fn failed_array(source: &str, expected: &[i32], calls: usize) {
    let probe = Probe::new();
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = Runtime::new(config);
    kagari_runtime::native::module::NativeModule::install_all(
        &crate::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    probe.module.install(&mut runtime).unwrap();
    let loaded = load_bytecode(
        &mut runtime,
        "failure",
        program(
            &format!("{source} {}", after_failure_source("i32", expected.len())),
            &[&probe.module],
        ),
    )
    .unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    assert!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .is_err()
    );
    assert_eq!(probe.calls.load(Ordering::SeqCst), calls);
    let snapshot = inspect_retained(&mut vm, &loaded, &probe);
    assert_eq!(array_i32(&mut vm, &loaded, &snapshot), expected);
    drop(snapshot);
    probe.retained.lock().unwrap().take();
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn retain_failure_keeps_completed_removals_and_stops_callbacks() {
    failed_array(
        include_str!("../../../../tests/fixtures/collections/retain-callback-failure.kgr"),
        &[2, 3, 4],
        3,
    );
}

const CONTAINER: &str =
    include_str!("../../../../tests/fixtures/collections/custom-list-receiver.kgr");

#[test]
fn custom_receiver_failures_preserve_completed_writes_and_removals() {
    for (operation, expected) in [
        ("reverse()", vec![2, 1, 2]),
        ("retain(|value| value < 0)", vec![1, 2]),
    ] {
        let source = format!(
            "{CONTAINER} fn main() {{ val values = Vec::from([3,1,2]); keep(values); val receiver: MutableList<i32> = Sequence {{ items: values }}; receiver.{operation}; }}"
        );
        failed_array(&source, &expected, 2);
    }
}

#[test]
fn list_edits_commit_before_invalidating_an_active_cursor() {
    for (operation, expected) in [
        ("sort()", vec![1, 1, 2, 3]),
        ("reverse()", vec![2, 1, 1, 3]),
        ("retain(|value| value != 1)", vec![3, 2]),
        ("dedup()", vec![3, 1, 2]),
    ] {
        let source = format!(
            "use test::probe::keep; fn main() {{ val values = Vec::from([3,1,1,2]); keep(values); for value in values {{ values.{operation}; }} }}"
        );
        failed_array(&source, &expected, 0);
    }
}

#[test]
fn cursor_reads_cannot_observe_a_buffer_detached_for_sorting() {
    failed_array(
        include_str!("../../../../tests/fixtures/collections/sort-cursor-read.kgr"),
        &[3, 1, 2],
        0,
    );
}
