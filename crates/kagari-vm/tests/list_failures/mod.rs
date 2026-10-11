//! Failure preserves completed work and releases every temporary root/lease.
use super::*;
use std::sync::atomic::Ordering;

fn failed_array(source: &str, expected: &[i32], calls: usize) {
    let probe = Probe::new();
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = Runtime::new(config);
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    probe.module.install(&mut runtime).unwrap();
    let loaded = runtime
        .load_program("failure", program(source, &[&probe.module]))
        .unwrap();
    let vm = Vm::new(runtime);
    assert!(vm.execute(&loaded, "main").is_err());
    assert_eq!(probe.calls.load(Ordering::SeqCst), calls);
    let Value::GcHandle(array) = probe
        .retained
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .value(vm.runtime().gc())
        .unwrap()
    else {
        panic!("array")
    };
    assert_eq!(
        vm.runtime().gc().sequence_snapshot(array).unwrap(),
        expected
            .iter()
            .map(|value| Value::I32(*value))
            .collect::<Vec<_>>()
    );
    vm.runtime()
        .gc()
        .sequence_push(array, Value::I32(99))
        .unwrap();
    probe.retained.lock().unwrap().take();
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn retain_failure_keeps_completed_removals_and_stops_callbacks() {
    failed_array(
        r#"
use test::probe::{keep, tick};
fn main() {
    val values = Vec::from([1,2,3,4]); keep(values);
    values.retain(|value| {
        val count = tick();
        if count == 3usize { val fail = 1 / 0; }
        value != 1
    });
}
"#,
        &[2, 3, 4],
        3,
    );
}

const CONTAINER: &str = r#"use std::collections::{List, MutableList};
use std::iter::{CollectionCursor};
use std::ops::{Index};

use test::probe::{keep, tick};
struct Sequence { val items: Vec<i32> }
impl Index<usize> for Sequence {
    type Output = i32;
    fn index(self, index: usize) -> i32 { self.items[index] }
}
impl Iterable for Sequence {
    type Item = i32;
    type Iter = CollectionCursor<i32>;
    fn iter(self) -> CollectionCursor<i32> { self.items.iter() }
}
impl List<i32> for Sequence {
    fn len(self) -> usize { self.items.len() }
    fn is_empty(self) -> bool { self.items.is_empty() }
    fn get(self, index: usize) -> Option<i32> { self.items.get(index) }
}
impl MutableList<i32> for Sequence {
    fn push(self, value: i32) { self.items.push(value); }
    fn pop(self) -> Option<i32> { self.items.pop() }
    fn insert(self, index: usize, value: i32) { self.items.insert(index,value); }
    fn clear(self) { self.items.clear(); }
    fn set(self, index: usize, value: i32) {
        if tick() == 2usize { val fail = 1 / 0; }
        self.items[index] = value;
    }
    fn remove(self, index: usize) -> Option<i32> {
        if tick() == 2usize { val fail = 1 / 0; }
        self.items.remove(index)
    }
}
"#;

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
fn active_iteration_rejects_list_edits_and_releases_the_guard_on_failure() {
    for operation in ["sort()", "reverse()", "retain(|value| true)", "dedup()"] {
        let source = format!(
            "use test::probe::keep; fn main() {{ val values = Vec::from([3,1,2]); keep(values); for value in values {{ values.{operation}; }} }}"
        );
        failed_array(&source, &[3, 1, 2], 0);
    }
}
