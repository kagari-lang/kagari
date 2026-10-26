//! Four retained closure generations keep object graphs while calls use current helpers.
use crate::{
    families::prepare,
    observations::{self, Record},
};
use kagari::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::native::value_handle::ScriptValue;
use std::{array, time::Instant};

const SOURCE: &str = r#"
struct Item { val value: i32 }
fn bias() -> i32 { __VERSION__ }
pub fn make(n: i32) -> fn() -> i32 {
    val values: Vec<Item> = Vec::new();
    var i = 0;
    while i < n { values.push(Item { value: 1 }); i += 1; }
    || { values.len() as i32 + values[0].value + __CAPTURED_BODY__ + bias() }
}
pub fn invoke(callback: fn() -> i32) -> i32 { callback() }
"#;

fn lifecycle(engine: &KagariEngine, programs: &[PreparedProgram; 4]) -> Record {
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let mut record = Record::default();
    let start = Instant::now();
    let loaded = runtime
        .load_program(&programs[0], Default::default())
        .unwrap();
    let baseline = runtime.runtime().collect_garbage().unwrap().live_objects;
    record.capture("setup", &runtime, start);
    let start = Instant::now();
    let mut closures: [Option<ScriptValue>; 4] = array::from_fn(|_| None);
    let mut keys = Vec::with_capacity(4);
    for (index, program) in programs.iter().enumerate() {
        if index != 0 {
            runtime
                .reload_program(&loaded, program, Default::default())
                .unwrap();
        }
        keys.push(runtime.runtime().select_program(&loaded).unwrap().key());
        closures[index] = Some(
            runtime
                .execute_typed(&loaded, "make", (256_i32,), &context)
                .unwrap(),
        );
    }
    record.capture("construct_reload", &runtime, start);
    let start = Instant::now();
    runtime.runtime().collect_garbage().unwrap();
    record.capture("retained_gc", &runtime, start);
    for (index, callback) in closures.iter().enumerate() {
        assert!(runtime.runtime().modules().loaded(keys[index]).is_some());
        let result: i32 = runtime
            .execute_typed(
                &loaded,
                "invoke",
                (callback.as_ref().unwrap().clone(),),
                &context,
            )
            .unwrap();
        // Retained closure body differs per generation; named helper is current.
        assert_eq!(result, 256 + 1 + index as i32 * 10 + 3);
    }
    for callback in &mut closures[..3] {
        *callback = None;
    }
    let start = Instant::now();
    runtime.runtime().collect_garbage().unwrap();
    record.capture("release_old_gc", &runtime, start);
    for key in &keys[..3] {
        assert!(runtime.runtime().modules().loaded(*key).is_none());
    }
    assert!(runtime.runtime().modules().loaded(keys[3]).is_some());
    drop(closures);
    let start = Instant::now();
    assert_eq!(
        runtime.runtime().collect_garbage().unwrap().live_objects,
        baseline
    );
    record.capture("release_all_gc", &runtime, start);
    record
}

pub(super) fn run(engine: &KagariEngine) {
    let programs: [_; 4] = array::from_fn(|version| {
        let source = SOURCE
            .replace("__VERSION__", &version.to_string())
            .replace("__CAPTURED_BODY__", &(version * 10).to_string());
        prepare(engine, "storage_reload.kgr", &source)
    });
    observations::run("retained_reload_4x256", || lifecycle(engine, &programs));
}
