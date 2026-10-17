//! Fixed Lua setup controls, isolated from the Kagari subject's linked image/state.
use std::{env, error::Error, fs, path::PathBuf, time::Instant};

use mlua::{Function, Lua, Result as LuaResult};

const BATCH: usize = 32;
const SAMPLES: usize = 3;

struct Input {
    source: PathBuf,
    workload: String,
    size: usize,
    execution_batch: usize,
    expected_checksum: i64,
}

impl Input {
    fn parse() -> Result<Self, Box<dyn Error>> {
        let mut arguments = env::args().skip(1);
        let mut source = None;
        let mut workload = None;
        let mut size = None;
        let mut execution_batch = None;
        let mut expected_checksum = None;
        while let Some(argument) = arguments.next() {
            let value = arguments.next().ok_or("missing argument value")?;
            match argument.as_str() {
                "--source" => source = Some(PathBuf::from(value)),
                "--workload" => workload = Some(value),
                "--size" => size = Some(value.parse()?),
                "--execution-batch" => execution_batch = Some(value.parse()?),
                "--expected-checksum" => expected_checksum = Some(value.parse()?),
                _ => return Err(format!("unknown argument: {argument}").into()),
            }
        }
        let input = Self {
            source: source.ok_or("missing --source")?,
            workload: workload.ok_or("missing --workload")?,
            size: size.ok_or("missing --size")?,
            execution_batch: execution_batch.ok_or("missing --execution-batch")?,
            expected_checksum: expected_checksum.ok_or("missing --expected-checksum")?,
        };
        if input.execution_batch == 0
            || input.workload.is_empty()
            || !input
                .workload
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err("invalid workload name or execution batch".into());
        }
        Ok(input)
    }
}

struct Record<'a> {
    input: &'a Input,
    sample: usize,
}

impl Record<'_> {
    fn measure(&self, phase: &str, action: impl FnOnce() -> LuaResult<()>) -> LuaResult<()> {
        let start = Instant::now();
        action()?;
        let elapsed = start.elapsed().as_nanos();
        self.emit(phase, elapsed, 0);
        Ok(())
    }

    fn emit(&self, phase: &str, ns: u128, checksum: i64) {
        println!(
            "{phase},{},lua54,{},{BATCH},{},{ns},{checksum}",
            self.input.workload, self.input.size, self.sample,
        );
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let input = Input::parse()?;
    let source = fs::read_to_string(&input.source)?.replace("__N__", &input.size.to_string());
    println!("phase,workload,engine,size,batch,sample,ns,checksum");
    for sample in 0..SAMPLES {
        let record = Record {
            input: &input,
            sample,
        };
        let mut states = Vec::with_capacity(BATCH);
        let mut chunks = Vec::with_capacity(BATCH);
        let mut entries = Vec::with_capacity(BATCH);
        record.measure("engine_init", || {
            for _ in 0..BATCH {
                states.push(Lua::new());
            }
            Ok(())
        })?;
        for lua in &states {
            assert_eq!(lua.globals().get::<String>("_VERSION")?, "Lua 5.4");
        }
        record.measure("source_to_chunk", || {
            for lua in &states {
                chunks.push(
                    lua.load(source.as_str())
                        .set_name(input.workload.as_str())
                        .into_function()?,
                );
            }
            Ok(())
        })?;
        record.measure("module_init", || {
            for chunk in &chunks {
                entries.push(chunk.call::<Function>(())?);
            }
            Ok(())
        })?;
        // All states, chunks and returned functions stay live across preparation.
        // Execution validates the frozen workload outside every setup timer.
        for entry in &entries {
            let mut checksum = 0_i64;
            for _ in 0..input.execution_batch {
                checksum += i64::from(entry.call::<i32>(())?);
            }
            assert_eq!(checksum, input.expected_checksum, "{}", input.workload);
        }
        record.emit("validate", 0, input.expected_checksum);
    }
    Ok(())
}
