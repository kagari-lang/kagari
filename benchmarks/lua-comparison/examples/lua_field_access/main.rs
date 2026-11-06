//! PUC Lua scalar table access, with local-loop controls and no timed setup/GC.

use mlua::{Function, Lua, Result as LuaResult, Table};
use std::{env, time::Instant};

const ITERATIONS: i64 = 50_000;
const WARMUPS: usize = 3;
const SAMPLES: usize = 11;
const CASES: [(&str, i64, i64); 9] = [
    ("named_read", 7, 7 * ITERATIONS),
    ("named_write", 0, ITERATIONS - 1),
    ("named_increment", 0, ITERATIONS),
    ("indexed_read", 7, 7 * ITERATIONS),
    ("indexed_write", 0, ITERATIONS - 1),
    ("indexed_increment", 0, ITERATIONS),
    ("local_read", 7, 7 * ITERATIONS),
    ("local_write", 0, ITERATIONS - 1),
    ("local_increment", 0, ITERATIONS),
];

fn main() -> LuaResult<()> {
    let process: usize = env::args()
        .nth(1)
        .map(|value| value.parse().expect("numeric process index"))
        .unwrap_or(0);
    let lua = Lua::new();
    assert_eq!(lua.globals().get::<String>("_VERSION")?, "Lua 5.4");
    let api: Table = lua.load(include_str!("workload.lua")).eval()?;
    let functions = CASES
        .iter()
        .map(|(name, _, _)| api.get::<Function>(*name))
        .collect::<LuaResult<Vec<_>>>()?;
    let named = lua.create_table_with_capacity(0, 1)?;
    named.set("value", 0)?;
    let indexed = lua.create_table_with_capacity(1, 0)?;
    indexed.set(1, 0)?;
    lua.gc_collect()?;
    lua.gc_stop();
    println!("process,phase,sample,case,iterations,elapsed_ns,checksum");
    for sample in 0..WARMUPS + SAMPLES {
        let phase = if sample < WARMUPS {
            "warmup"
        } else {
            "measure"
        };
        for position in 0..CASES.len() {
            let index = (position + process + sample) % CASES.len();
            let (name, initial, expected) = CASES[index];
            let is_indexed = name.starts_with("indexed_");
            let table = if is_indexed { &indexed } else { &named };
            if is_indexed {
                table.set(1, initial)?;
            } else {
                table.set("value", initial)?;
            }
            let args = (table.clone(), ITERATIONS);
            let start = Instant::now();
            // One host-to-Lua call per batch; all loop iterations execute in Lua.
            let checksum: i64 = functions[index].call(args)?;
            let nanoseconds = start.elapsed().as_nanos();
            assert_eq!(checksum, expected, "{name}");
            let final_value: i64 = if is_indexed {
                table.get(1)?
            } else {
                table.get("value")?
            };
            let expected_field = if name.ends_with("_read") {
                initial
            } else {
                expected
            };
            assert_eq!(final_value, expected_field, "{name}");
            println!("{process},{phase},{sample},{name},{ITERATIONS},{nanoseconds},{checksum}");
        }
    }
    drop((functions, api, named, indexed));
    lua.gc_restart();
    lua.gc_collect()?;
    Ok(())
}
