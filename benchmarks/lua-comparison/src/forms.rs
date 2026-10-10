//! Same recurrence through concrete and dynamic boundaries, plus bounded byte state.
#[cfg(feature = "diagnostics")]
use crate::diagnostics;

use crate::{Options, profile};
use kagari_bytecode::instruction::BinaryOp;
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::{
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::CallContext,
        declarations::FunctionDecl, types::Type,
    },
    numeric,
    value::Value,
};
use kagari_source::source::SourceFile;
use mlua::{Error as LuaError, Function, Lua};
use std::{hint::black_box, time::Instant};

const N: i32 = 5_000;
const SOURCE: &str = r#"
use bench::numeric::native_step;
use core::ops::Add;
fn step(state: i32) -> i32 { (state * 3 + 7) % 251 }
fn add<T: Add<T>>(a: T, b: T) -> T::Output { a + b }
trait Step { fn step(self, state: i32) -> i32; }
impl Step for i32 { fn step(self, state: i32) -> i32 { (state * 3 + 7) % 251 } }
trait Forward { fn forward<T>(self, value: T) -> T { value } }
impl Forward for i32 {}
struct Counter { var value: i32 }
fn direct() -> i32 { var state = 1; var sum = 0; var i = 0;
    while i < 5000 { state = (state * 3 + 7) % 251; sum += state; i += 1; } sum }
fn helper() -> i32 { var state = 1; var sum = 0; var i = 0;
    while i < 5000 { state = step(state); sum += state; i += 1; } sum }
fn concrete_generic() -> i32 { var state = 1; var sum = 0; var i = 0;
    while i < 5000 { state = add(state * 3, 7) % 251; sum += state; i += 1; } sum }
fn interface() -> i32 { val receiver: Step = 0; var state = 1; var sum = 0; var i = 0;
    while i < 5000 { state = receiver.step(state); sum += state; i += 1; } sum }
fn shared_generic() -> i32 { val receiver: Forward = 0; var state = 1; var sum = 0; var i = 0;
    while i < 5000 { state = receiver.forward((state * 3 + 7) % 251); sum += state; i += 1; } sum }
fn capture_cell() -> i32 { var state = 1; val next = || { state = (state * 3 + 7) % 251; state };
    var sum = 0; var i = 0; while i < 5000 { sum += next(); i += 1; } sum }
fn field() -> i32 { val counter = Counter { value: 1 }; var sum = 0; var i = 0;
    while i < 5000 { counter.value = (counter.value * 3 + 7) % 251; sum += counter.value; i += 1; } sum }
fn native() -> i32 { var state = 1; var sum = 0; var i = 0;
    while i < 5000 { state = native_step(state); sum += state; i += 1; } sum }
fn host_callback() -> i32 { var state = 1; var sum = 0; var i = 0;
    while i < 5000 { state = native_step(state); sum += state; i += 1; } sum }
fn string_identity(text: String) -> String { text }
fn string_constants() -> i32 { var sum = 0; var i = 0;
    while i < 5000 {
        val text = if i % 2 == 0 { "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ" }
            else { "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ-extra" };
        sum += text.len() as i32; i += 1;
    } sum }
fn string_calls() -> i32 { var sum = 0; var i = 0;
    while i < 5000 {
        val text = if i % 2 == 0 { "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ" }
            else { "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ-extra" };
        val result = string_identity(text);
        sum += result.len() as i32; i += 1;
    } sum }
fn byte_state() -> i32 { val memory: Vec<u8> = [0u8; 256]; var state = 1u8; var sum = 0; var i = 0;
    while i < 5000 {
        state = (((state as u32) * 13u32 + 17u32) as u8) ^ (state >> 3u32);
        val index = ((i as u32) & 255u32) as i32;
        memory[index] = ((memory[index] as u32) + (state as u32)) as u8;
        sum += memory[index] as i32;
        i += 1;
    } sum }
"#;

fn native_step(_: &mut CallContext<'_>, state: i32) -> NativeResult<i32> {
    host_step(state)
}

fn host_step(state: i32) -> NativeResult<i32> {
    let multiplied = numeric::binary(BinaryOp::Mul, Value::I32(state), Value::I32(3))?;
    let added = numeric::binary(BinaryOp::Add, multiplied, Value::I32(7))?;
    let Value::I32(result) = numeric::binary(BinaryOp::Rem, added, Value::I32(251))? else {
        unreachable!()
    };
    Ok(result)
}

fn reference(name: &str) -> i32 {
    if matches!(name, "string_constants" | "string_calls") {
        return (0..N).map(|i| if i % 2 == 0 { 62 } else { 68 }).sum();
    }
    let mut state = 1u32;
    let mut sum = 0;
    let mut memory = [0u8; 256];
    for i in 0..N {
        if name == "byte_state" {
            state = ((state * 13 + 17) & 255) ^ (state >> 3);
            let slot = &mut memory[i as usize & 255];
            *slot = slot.wrapping_add(state as u8);
            sum += i32::from(*slot);
        } else {
            state = (state * 3 + 7) % 251;
            sum += state as i32;
        }
    }
    sum
}

fn lua_source(name: &str) -> String {
    if matches!(name, "string_constants" | "string_calls") {
        let transfer = if name == "string_calls" {
            "text = identity(text);"
        } else {
            ""
        };
        return format!(
            "return function() local function identity(text) return text end; local sum, i = 0, 0; while i < {N} do local text; if i % 2 == 0 then text = 'abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ' else text = '0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ-extra' end; {transfer} sum = sum + #text; i = i + 1 end; return sum end"
        );
    }
    let (setup, action) = match name {
        "direct" => ("", "state = (state * 3 + 7) % 251"),
        "helper" | "native" => (
            "local function step(x) return (x * 3 + 7) % 251 end",
            "state = step(state)",
        ),
        "host_callback" => ("", "state = host_step(state)"),
        "concrete_generic" => (
            "local function add(a, b) return a + b end",
            "state = add(state * 3, 7) % 251",
        ),
        "shared_generic" => (
            "local receiver = { forward = function(self, value) return value end }",
            "state = receiver:forward((state * 3 + 7) % 251)",
        ),
        "interface" => (
            "local receiver = { step = function(self, x) return (x * 3 + 7) % 251 end }",
            "state = receiver:step(state)",
        ),
        "capture_cell" => (
            "local function step() state = (state * 3 + 7) % 251; return state end",
            "state = step()",
        ),
        "field" => (
            "local counter = { value = 1 }",
            "counter.value = (counter.value * 3 + 7) % 251; state = counter.value",
        ),
        "byte_state" => (
            "local memory = {}; for j = 0, 255 do memory[j] = 0 end",
            "state = (((state * 13 + 17) & 255) ~ (state >> 3)); local j = i & 255; memory[j] = (memory[j] + state) & 255; sum = sum + memory[j]",
        ),
        _ => unreachable!(),
    };
    let accumulate = if name == "byte_state" {
        ""
    } else {
        "sum = sum + state"
    };
    format!(
        "return function() local state, sum, i = 1, 0, 0; {setup}; while i < {N} do {action}; {accumulate}; i = i + 1 end; return sum end"
    )
}

pub(super) fn run(options: &Options) {
    let mut builder = KagariEngine::builder().expect("standard engine");
    let mut native = ModuleBuilder::new("bench::numeric", builder.declarations());
    let step = native
        .define_function(
            FunctionDecl::new("native_step")
                .parameter("state", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    native.bind(step, native_step).unwrap();
    builder.install(native.finish().unwrap()).unwrap();
    let engine = builder.build().unwrap();
    let start = Instant::now();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("source_forms.kgr", SOURCE),
            Default::default(),
        )
        .expect("source forms compilation");
    eprintln!("forms source_to_artifact_ns={}", start.elapsed().as_nanos());
    let start = Instant::now();
    let prepared =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
            .expect("forms verification");
    eprintln!("forms artifact_prepare_ns={}", start.elapsed().as_nanos());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let start = Instant::now();
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    eprintln!("forms link_ns={}", start.elapsed().as_nanos());
    let lua = Lua::new();
    lua.globals()
        .set(
            "host_step",
            lua.create_function(|_, state: i32| host_step(state).map_err(LuaError::external))
                .unwrap(),
        )
        .unwrap();
    let mut names = [
        "direct",
        "helper",
        "concrete_generic",
        "interface",
        "shared_generic",
        "capture_cell",
        "field",
        "native",
        "byte_state",
        "host_callback",
        "string_constants",
        "string_calls",
    ];
    if let Some(selected) = &options.profile {
        assert!(
            names.contains(&selected.as_str()),
            "unknown source-form workload"
        );
    }
    if options.reverse {
        names.reverse();
    }
    for name in names {
        if options
            .profile
            .as_ref()
            .is_some_and(|selected| selected != name)
        {
            continue;
        }
        let entry: Function = lua.load(lua_source(name)).eval().unwrap();
        let expected = reference(name);
        let execute = |engine: &str| {
            if engine == "lua54" {
                return entry.call::<i32>(()).unwrap();
            }
            let report = runtime.execute(&loaded, name, &[], &context).unwrap();
            let Value::I32(value) = report.return_value.value(runtime.runtime().gc()).unwrap()
            else {
                panic!("forms result");
            };
            value
        };
        #[cfg(feature = "diagnostics")]
        if options.diagnostics {
            assert_eq!(execute("lua54"), expected);
            diagnostics::measure(&runtime, &loaded, &context, name, &[], expected);
            continue;
        }
        for _ in 0..options.warmups {
            for engine in ["kagari_vm", "lua54"] {
                assert_eq!(execute(engine), expected);
            }
        }
        if options.profile.is_some() {
            profile::count_lua(&lua, &entry, expected);
            profile::run(&runtime, &loaded, &context, name, name, expected);
            continue;
        }
        for sample in 0..options.samples {
            let mut engines = ["kagari_vm", "lua54"];
            if (sample % 2 == 1) != options.reverse {
                engines.reverse();
            }
            for engine in engines {
                let start = Instant::now();
                let value = black_box(execute(engine));
                let elapsed = start.elapsed().as_nanos();
                assert_eq!(value, expected, "{name}/{engine}");
                println!("execute,forms_{name},{engine},{N},1,{sample},{elapsed},{value}");
            }
        }
    }
}
