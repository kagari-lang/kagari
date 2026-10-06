//! Matched finite arithmetic/bit/conversion workloads across every numeric type.
use crate::Options;
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;
use mlua::{Function, Lua};
use std::{hint::black_box, time::Instant};

const TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "isize", "u8", "u16", "u32", "u64", "usize", "f32", "f64",
];
const N: i32 = 20_000;

fn source(ty: &str) -> String {
    let float = ty.starts_with('f');
    let literal = if float { ".0" } else { "" };
    let bits = if float {
        String::new()
    } else {
        format!("state = ((state ^ 5{ty}) << 1u32) >> 1u32;")
    };
    format!("fn numeric_{ty}(n: i32) -> i32 {{
        var state: {ty} = 1{literal}{ty};
        var sum = 0;
        var i = 0;
        while i < n {{
            state = (((state + 7{literal}{ty}) * 2{literal}{ty}) / 2{literal}{ty}) % 31{literal}{ty};
            {bits}
            sum += state as i32;
            i += 1;
        }}
        sum
    }}
    fn main_{ty}() -> i32 {{ numeric_{ty}({N}) }}")
}

fn expected(float: bool) -> i32 {
    let mut state = 1;
    let mut sum = 0;
    for _ in 0..N {
        state = (state + 7) % 31;
        if !float {
            state ^= 5;
        }
        sum += state;
    }
    sum
}

pub(super) fn run(options: &Options) {
    let mut types = TYPES.to_vec();
    if options.reverse {
        types.reverse();
    }
    let text = types
        .iter()
        .map(|ty| source(ty))
        .collect::<Vec<_>>()
        .join("\n");
    let engine = KagariEngine::default();
    let start = Instant::now();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("numeric_matrix.kgr", text),
            Default::default(),
        )
        .expect("numeric compilation");
    eprintln!(
        "numeric source_to_artifact_ns={}",
        start.elapsed().as_nanos()
    );
    let start = Instant::now();
    let prepared =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
            .expect("numeric verification");
    eprintln!("numeric artifact_prepare_ns={}", start.elapsed().as_nanos());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared, Default::default())
        .expect("numeric link");
    let lua = Lua::new();
    for ty in types {
        let float = ty.starts_with('f');
        let division = if float { "/" } else { "//" };
        let bits = if float {
            ""
        } else {
            "state = ((state ~ 5) << 1) >> 1"
        };
        let entry: Function = lua
            .load(format!(
                "local function run(n)
            local state, sum = 1, 0
            for i = 1, n do
                state = (((state + 7) * 2) {division} 2) % 31
                {bits}
                sum = sum + state
            end
            return sum
        end
        return function() return run({N}) end"
            ))
            .eval()
            .expect("numeric Lua compilation");
        let name = format!("numeric_{ty}");
        let entry_name = format!("main_{ty}");
        let reference = expected(float);
        let run = |engine: &str| {
            if engine == "lua54" {
                return entry.call::<i32>(()).expect("numeric Lua execution");
            }
            let report = runtime
                .execute(&loaded, &entry_name, &[], &context)
                .expect("numeric execution");
            let Value::I32(value) = report
                .return_value
                .value(runtime.runtime().gc())
                .expect("numeric result")
            else {
                panic!("numeric result type")
            };
            value
        };
        for _ in 0..options.warmups {
            for engine in ["kagari_vm", "lua54"] {
                assert_eq!(run(engine), reference);
            }
        }
        for sample in 0..options.samples {
            let mut engines = ["kagari_vm", "lua54"];
            if (sample % 2 == 1) != options.reverse {
                engines.reverse();
            }
            for engine in engines {
                let start = Instant::now();
                let value = black_box(run(engine));
                let elapsed = start.elapsed().as_nanos();
                assert_eq!(value, reference, "{ty}/{engine}");
                println!("execute,{name},{engine},{N},1,{sample},{elapsed},{value}");
            }
        }
    }
}
