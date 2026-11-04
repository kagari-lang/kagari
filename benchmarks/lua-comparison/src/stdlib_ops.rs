//! Current public operations, including their actual transport and wrapper costs.
use crate::Options;
#[cfg(feature = "diagnostics")]
use crate::diagnostics;
use crate::profile;
use kagari::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, declarations::FunctionDecl,
    typed::NativeContext, types::Type, value_handle::ScriptValue,
};
use kagari_source::source::SourceFile;
use mlua::{Function, Lua};
use std::{hint::black_box, time::Instant};

const N: i64 = 5_000;
const WIDTH: i64 = 64;
const FOLD: i64 = 2_048;
const SOURCE: &str = r#"
use bench::stdlib::one;
use core::iter::Iterable;
struct Item { val value: i64 }
pub fn numbers(n: i64) -> Vec<i64> {
    val v: Vec<i64> = Vec::new(); var i = 0i64;
    while i < n { v.push(i); i += 1i64; } v
}
pub fn references(n: i64) -> Vec<Item> {
    val v: Vec<Item> = Vec::new(); var i = 0i64;
    while i < n { v.push(Item { value: i }); i += 1i64; } v
}
pub fn loop_control(v: Vec<i64>) -> i64 {
    var sum = 0i64; var i = 0i64;
    while i < 5000i64 { sum += 1i64; i += 1i64; } sum
}
pub fn native_zero(v: Vec<i64>) -> i64 {
    var sum = 0i64; var i = 0i64;
    while i < 5000i64 { sum += one(); i += 1i64; } sum
}
pub fn vec_len(v: Vec<i64>) -> i64 {
    var sum = 0i64; var i = 0i64;
    while i < 5000i64 { sum += v.len() as i64; i += 1i64; } sum
}
pub fn vec_get(v: Vec<i64>) -> i64 {
    var sum = 0i64; var i = 0i64;
    while i < 5000i64 { sum += v[(i % 64i64) as u64]; i += 1i64; } sum
}
pub fn vec_set_get(v: Vec<i64>) -> i64 {
    var sum = 0i64; var i = 0i64;
    while i < 5000i64 {
        val index = (i % 64i64) as u64; v[index] = i % 97i64;
        sum += v[index]; i += 1i64;
    } sum
}
pub fn vec_pop_push(v: Vec<i64>) -> i64 {
    var sum = 0i64; var i = 0i64;
    while i < 5000i64 {
        val item = match v.pop() { Some(value) => value, None => -1i64 };
        sum += item; v.push(item); i += 1i64;
    } sum
}
pub fn fold_scalar(v: Vec<i64>) -> i64 { v.fold(0i64, |a, b| a + b) }
pub fn fold_references(v: Vec<Item>) -> i64 { v.fold(0i64, |a, b| a + b.value) }
pub fn verify(v: Vec<i64>) -> i64 {
    var i = 0u64; var sum = 0i64;
    while i < 64u64 { sum += (i as i64 + 1i64) * v[i]; i += 1u64; }
    sum + v.len() as i64
}
"#;

fn one() -> i64 {
    black_box(1)
}

fn native_one(_: &mut NativeContext<'_>, (): ()) -> NativeResult<i64> {
    Ok(one())
}

fn lua_source(name: &str) -> String {
    let action = match name {
        "loop_control" => "sum = sum + 1",
        "native_zero" => "sum = sum + one()",
        "vec_len" => "sum = sum + #v",
        "vec_get" => "sum = sum + v[i % 64 + 1]",
        "vec_set_get" => "local index = i % 64 + 1; v[index] = i % 97; sum = sum + v[index]",
        "vec_pop_push" => "local item = remove(v); sum = sum + item; insert(v, item)",
        "fold_scalar" => "sum = reduce(sum, v[i + 1])",
        "fold_references" => "sum = reduce(sum, v[i + 1])",
        _ => unreachable!(),
    };
    let size = if name.starts_with("fold_") { FOLD } else { N };
    let setup = if name == "fold_references" {
        "local function reduce(a, b) return a + b.value end"
    } else if name == "fold_scalar" {
        "local function reduce(a, b) return a + b end"
    } else if name == "native_zero" {
        "local one = one"
    } else if name == "vec_pop_push" {
        "local remove, insert = table.remove, table.insert"
    } else {
        ""
    };
    format!(
        "return function(v) {setup}; local sum, i = 0, 0;
         while i < {size} do {action}; i = i + 1 end; return sum end"
    )
}

fn emit(phase: &str, name: &str, engine: &str, size: i64, sample: usize, ns: u128, sum: i64) {
    println!("{phase},ops_{name},{engine},{size},1,{sample},{ns},{sum}");
}

pub(super) fn run(options: &Options) {
    let mut builder = KagariEngine::builder().unwrap();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = None;
    builder.config(config);
    let mut native = ModuleBuilder::new("bench::stdlib", builder.declarations()).unwrap();
    let entry = native
        .define_function(FunctionDecl::new("one").returns(Type::i64()))
        .unwrap();
    native.bind(entry, native_one).unwrap();
    builder.install(native.finish().unwrap()).unwrap();
    let engine = builder.build().unwrap();
    let start = Instant::now();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("stdlib_ops.kgr", SOURCE),
            Default::default(),
        )
        .expect("stdlib operation compilation");
    emit(
        "source_to_artifact",
        "module",
        "kagari_vm",
        0,
        0,
        start.elapsed().as_nanos(),
        0,
    );
    let start = Instant::now();
    let prepared =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    emit(
        "artifact_prepare",
        "module",
        "kagari_vm",
        0,
        0,
        start.elapsed().as_nanos(),
        0,
    );
    let context = ExecutionContext::default();
    let start = Instant::now();
    let mut runtime = engine.runtime(context.clone());
    emit(
        "runtime_init",
        "module",
        "kagari_vm",
        0,
        0,
        start.elapsed().as_nanos(),
        0,
    );
    let start = Instant::now();
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    emit(
        "program_link",
        "module",
        "kagari_vm",
        0,
        0,
        start.elapsed().as_nanos(),
        0,
    );
    let lua = Lua::new();
    lua.gc_stop();
    lua.globals()
        .set("one", lua.create_function(|_, ()| Ok(one())).unwrap())
        .unwrap();
    let verify: Function = lua
        .load("return function(v) local s = #v; for i=1,64 do s=s+i*v[i] end; return s end")
        .eval()
        .unwrap();
    let verify_kagari = runtime
        .function::<(ScriptValue,), i64>(&loaded.root(), "verify")
        .unwrap();
    let mut names = [
        "loop_control",
        "native_zero",
        "vec_len",
        "vec_get",
        "vec_set_get",
        "vec_pop_push",
        "fold_scalar",
        "fold_references",
    ];
    if options.reverse {
        names.reverse();
    }
    if let Some(selected) = &options.profile {
        assert!(
            matches!(selected.as_str(), "native_zero" | "vec_len" | "vec_get"),
            "stdlib sampling supports the three nonallocating Native/access routes"
        );
    }
    for name in names {
        if options
            .profile
            .as_ref()
            .is_some_and(|selected| selected != name)
        {
            continue;
        }
        #[cfg(feature = "diagnostics")]
        if options.diagnostics
            && !matches!(name, "loop_control" | "native_zero" | "vec_len" | "vec_get")
        {
            continue;
        }
        let size = if name.starts_with("fold_") { FOLD } else { N };
        let width = if name.starts_with("fold_") {
            FOLD
        } else {
            WIDTH
        };
        let references = name == "fold_references";
        let start = Instant::now();
        let entry: Function = lua.load(lua_source(name)).eval().unwrap();
        emit(
            "prepare",
            name,
            "lua54",
            size,
            0,
            start.elapsed().as_nanos(),
            0,
        );
        let start = Instant::now();
        let factory = runtime
            .function::<(i64,), ScriptValue>(
                &loaded.root(),
                if references { "references" } else { "numbers" },
            )
            .unwrap();
        let kagari_entry = runtime
            .function::<(ScriptValue,), i64>(&loaded.root(), name)
            .unwrap();
        let data = runtime.call(&factory, (width,)).unwrap();
        emit(
            "dataset_init",
            name,
            "kagari_vm",
            size,
            0,
            start.elapsed().as_nanos(),
            0,
        );
        let start = Instant::now();
        let table = lua.create_table_with_capacity(width as usize, 0).unwrap();
        for i in 0..width {
            if references {
                let item = lua.create_table().unwrap();
                item.set("value", i).unwrap();
                table.set(i + 1, item).unwrap();
            } else {
                table.set(i + 1, i).unwrap();
            }
        }
        emit(
            "dataset_init",
            name,
            "lua54",
            size,
            0,
            start.elapsed().as_nanos(),
            0,
        );
        let expected = match name {
            "loop_control" | "native_zero" => N,
            "vec_len" => N * WIDTH,
            "vec_get" => (0..N).map(|i| i % WIDTH).sum(),
            "vec_set_get" => (0..N).map(|i| i % 97).sum(),
            "vec_pop_push" => N * (WIDTH - 1),
            _ => FOLD * (FOLD - 1) / 2,
        };
        let mut final_values: Vec<i64> = (0..WIDTH).collect();
        if name == "vec_set_get" {
            for i in 0..N {
                final_values[(i % WIDTH) as usize] = i % 97;
            }
        }
        let final_checksum = WIDTH
            + final_values
                .iter()
                .enumerate()
                .map(|(i, v)| (i as i64 + 1) * v)
                .sum::<i64>();
        #[cfg(feature = "diagnostics")]
        if options.diagnostics {
            diagnostics::measure_typed(&mut runtime, &kagari_entry, &data, name, expected);
            continue;
        }
        if options.profile.is_some() {
            profile::run_typed(&mut runtime, &kagari_entry, &data, name, expected);
            continue;
        }
        for iteration in 0..options.warmups + options.samples {
            let mut engines = ["kagari_vm", "lua54"];
            if (iteration % 2 == 1) != options.reverse {
                engines.reverse();
            }
            for engine in engines {
                // Collection is explicit and untimed on both engines. This suite
                // measures warmed operation cost, not collector throughput.
                let (elapsed, result) = if engine == "lua54" {
                    lua.gc_collect().unwrap();
                    let start = Instant::now();
                    let result = black_box(entry.call::<i64>(table.clone()).unwrap());
                    (start.elapsed().as_nanos(), result)
                } else {
                    runtime.runtime().collect_garbage().unwrap();
                    let start = Instant::now();
                    let result = runtime.call(&kagari_entry, (data.clone(),)).unwrap();
                    let elapsed = start.elapsed().as_nanos();
                    (elapsed, black_box(result))
                };
                assert_eq!(result, expected, "{name}/{engine}");
                if matches!(name, "vec_set_get" | "vec_pop_push") {
                    let actual = if engine == "lua54" {
                        verify.call::<i64>(table.clone()).unwrap()
                    } else {
                        runtime.call(&verify_kagari, (data.clone(),)).unwrap()
                    };
                    assert_eq!(actual, final_checksum, "final contents {name}/{engine}");
                }
                if iteration >= options.warmups {
                    emit(
                        "execute",
                        name,
                        engine,
                        size,
                        iteration - options.warmups,
                        elapsed,
                        result,
                    );
                }
            }
        }
        // Keep the retained script input alive through the entire workload.
        drop(data);
    }
}
