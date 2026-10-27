//! Failure preserves completed work and releases every temporary root/lease.
use super::*;
use kagari_runtime::module::entry::ProgramEntry;
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
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = Vm::new(runtime);
    assert!(vm.execute(&loaded, ProgramEntry::Main).is_err());
    assert_eq!(probe.calls.load(Ordering::SeqCst), calls);
    let retained = probe.retained.lock().unwrap().as_ref().unwrap().clone();
    assert_eq!(
        ConversionContext::new(vm.runtime(), &loaded)
            .unwrap()
            .decode::<VecValues<i32>>(&retained)
            .unwrap()
            .0,
        expected
    );
    let vector: ScriptVec<i32> = ConversionContext::new(vm.runtime(), &loaded)
        .unwrap()
        .decode(&retained)
        .unwrap();
    vector.push(&mut vm.context(&loaded).unwrap(), 99).unwrap();
    drop(vector);
    drop(retained);
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
        if count == 3u64 { val fail = 1 / 0; }
        value != 1
    });
}
"#,
        &[2, 3, 4],
        3,
    );
}

const CONTAINER: &str = r#"use std::collections::{List, MutableList};
use alloc::vec::VecCursor;
use std::ops::{Index};

use test::probe::{keep, tick};
struct Sequence { val items: Vec<i32> }
impl Index<u64> for Sequence {
    type Output = i32;
    fn index(self, index: u64) -> i32 { self.items[index] }
}
impl Iterable for Sequence {
    type Item = i32;
    type Iter = VecCursor<i32>;
    fn iter(self) -> VecCursor<i32> { self.items.iter() }
}
impl List<i32> for Sequence {
    fn len(self) -> u64 { self.items.len() }
    fn is_empty(self) -> bool { self.items.is_empty() }
    fn get(self, index: u64) -> Option<i32> { self.items.get(index) }
}
impl MutableList<i32> for Sequence {
    fn push(self, value: i32) { self.items.push(value); }
    fn pop(self) -> Option<i32> { self.items.pop() }
    fn insert(self, index: u64, value: i32) { self.items.insert(index,value); }
    fn clear(self) { self.items.clear(); }
    fn set(self, index: u64, value: i32) {
        if tick() == 2u64 { val fail = 1 / 0; }
        self.items[index] = value;
    }
    fn remove(self, index: u64) -> Option<i32> {
        if tick() == 2u64 { val fail = 1 / 0; }
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
        r#"use test::probe::keep;
        fn main() {
            val values=Vec::from([3,1,2]); keep(values);
            val cursor=values.iter();
            values.sort_by(|a,b| { cursor.next(); a.cmp(b) });
        }"#,
        &[3, 1, 2],
        0,
    );
}
