//! Matched Lua/Kagari execution with setup separated and every checksum verified.
mod profile;
mod workloads;

use std::{env, hint::black_box, time::Instant};

use kagari_codegen_cranelift::CraneliftBackend;
use kagari_embed::{
    context::ExecutionContext, engine::KagariEngine, program::PreparedProgram,
    runtime::KagariRuntime,
};
use kagari_runtime::{module::LoadedModule, value::Value};
use kagari_source::source::SourceFile;
use kagari_vm::vm::{JitExecutionStatus, native::PreparedNativeEntry};
use mlua::{Function, Lua};
use workloads::{WORKLOADS, Workload};

struct Options {
    samples: usize,
    warmups: usize,
    setup_samples: usize,
    reverse: bool,
    profile: Option<String>,
}

impl Options {
    fn parse() -> Self {
        let mut options = Self {
            samples: 11,
            warmups: 3,
            setup_samples: 3,
            reverse: false,
            profile: None,
        };
        for argument in env::args().skip(1) {
            match argument.as_str() {
                "--check" => {
                    options.samples = 1;
                    options.warmups = 0;
                    options.setup_samples = 1;
                }
                "--reverse" => options.reverse = true,
                _ if argument.starts_with("--profile=") => {
                    options.profile = Some(argument["--profile=".len()..].to_owned());
                    options.setup_samples = 1;
                }
                _ => panic!(
                    "unknown argument: {argument}; supported: --check, --reverse, --profile=WORKLOAD"
                ),
            }
        }
        options
    }
}

struct Record<'a> {
    workload: &'a Workload,
    sample: usize,
}

impl Record<'_> {
    fn emit(&self, phase: &str, engine: &str, batch: usize, ns: u128, checksum: i64) {
        println!(
            "{phase},{},{engine},{},{batch},{},{ns},{checksum}",
            self.workload.name, self.workload.size, self.sample,
        );
    }

    fn setup<T>(&self, phase: &str, engine: &str, action: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let result = action();
        let elapsed = start.elapsed().as_nanos();
        self.emit(phase, engine, 1, elapsed, 0);
        result
    }

    fn execution(&self, engine: &str, expected: i32, mut action: impl FnMut() -> i32) {
        let mut checksum = 0_i64;
        let start = Instant::now();
        for _ in 0..self.workload.batch {
            checksum += i64::from(black_box(action()));
        }
        let elapsed = start.elapsed().as_nanos();
        assert_eq!(
            checksum,
            i64::from(expected) * self.workload.batch as i64,
            "{} {engine}",
            self.workload.name
        );
        self.emit("execute", engine, self.workload.batch, elapsed, checksum);
    }
}

struct ExecutionRoutes {
    runtime: KagariRuntime,
    module: LoadedModule,
    context: ExecutionContext,
    lua_entry: Function,
    native: PreparedNativeEntry,
}

impl ExecutionRoutes {
    fn kagari(&mut self, native: bool) -> i32 {
        let report = if native {
            self.runtime
                .execute_prepared(&self.module, "main", &[], &self.context, &self.native)
        } else {
            self.runtime
                .execute(&self.module, "main", &[], &self.context)
        }
        .expect("Kagari execution");
        if native {
            assert_eq!(
                report.jit.expect("native report").status,
                JitExecutionStatus::Native
            );
        }
        let Value::I32(value) = report
            .return_value
            .value(self.runtime.runtime().gc())
            .expect("retained execution result")
        else {
            panic!("expected i32 result")
        };
        value
    }

    fn lua(&self) -> i32 {
        self.lua_entry.call::<i32>(()).expect("Lua execution")
    }

    fn measure(&mut self, workload: &Workload, options: &Options) {
        let expected = (workload.reference)(workload.size);
        if options.profile.is_some() {
            profile::run(
                &mut self.runtime,
                &self.module,
                &self.context,
                workload.name,
                expected,
            );
            return;
        }
        let mut engines = vec!["kagari_vm", "lua54"];
        if matches!(self.native, PreparedNativeEntry::Native(_)) {
            engines.push("kagari_jit");
        }
        for _ in 0..options.warmups {
            for engine in &engines {
                let result = match *engine {
                    "lua54" => self.lua(),
                    "kagari_jit" => self.kagari(true),
                    _ => self.kagari(false),
                };
                assert_eq!(result, expected, "warmup {} {engine}", workload.name);
            }
        }
        for sample in 0..options.samples {
            // Rotate engines each round, reversing the second process's base order.
            let offset = sample % engines.len();
            for slot in 0..engines.len() {
                let index = if options.reverse {
                    engines.len() - 1 - slot
                } else {
                    slot
                };
                let engine = engines[(index + offset) % engines.len()];
                let record = Record { workload, sample };
                match engine {
                    "lua54" => record.execution(engine, expected, || self.lua()),
                    "kagari_jit" => record.execution(engine, expected, || self.kagari(true)),
                    _ => record.execution(engine, expected, || self.kagari(false)),
                }
            }
        }
    }
}

fn run(workload: &Workload, options: &Options) {
    let kagari_source = workload.kagari.replace("__N__", &workload.size.to_string());
    let lua_source = workload.lua.replace("__N__", &workload.size.to_string());
    for sample in 0..options.setup_samples {
        let record = Record { workload, sample };
        let engine = record.setup("engine_init", "kagari_vm", KagariEngine::default);
        let source = SourceFile::new(
            format!("benchmark_{}.kgr", workload.name),
            kagari_source.clone(),
        );
        let artifact = record.setup("source_to_artifact", "kagari_vm", || {
            engine
                .compile_to_artifact(source, Default::default())
                .expect("Kagari compilation")
        });
        let program = record.setup("artifact_prepare", "kagari_vm", || {
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .expect("artifact verification")
        });
        let context = ExecutionContext::default();
        let mut runtime = record.setup("runtime_init", "kagari_vm", || {
            engine.runtime(context.clone())
        });
        let module = record.setup("program_link", "kagari_vm", || {
            runtime
                .load_program(&program, Default::default())
                .expect("program link")
        });
        let lua = record.setup("engine_init", "lua54", Lua::new);
        assert_eq!(lua.globals().get::<String>("_VERSION").unwrap(), "Lua 5.4");
        let chunk = record.setup("source_to_chunk", "lua54", || {
            lua.load(lua_source.as_str())
                .set_name(workload.name)
                .into_function()
                .expect("Lua compilation")
        });
        let lua_entry = record.setup("module_init", "lua54", || {
            chunk.call::<Function>(()).expect("Lua module setup")
        });
        let mut backend = record.setup("native_backend_init", "kagari_jit", || {
            CraneliftBackend::for_host().unwrap()
        });
        let start = Instant::now();
        let native = runtime
            .prepare_native(&program, &module, "main", &mut backend, &Default::default())
            .expect("native preparation");
        let elapsed = start.elapsed().as_nanos();
        let phase = match &native {
            PreparedNativeEntry::Native(_) => "native_prepare",
            PreparedNativeEntry::Unsupported { diagnostics, .. } => {
                if sample == 0 {
                    eprintln!(
                        "JIT unsupported {}: {}",
                        workload.name,
                        diagnostics.join("; ")
                    );
                }
                "native_unsupported_probe"
            }
        };
        record.emit(phase, "kagari_jit", 1, elapsed, 0);
        if sample + 1 == options.setup_samples {
            if options.profile.is_some() {
                profile::count_lua(&lua, &lua_entry, (workload.reference)(workload.size));
            }
            ExecutionRoutes {
                runtime,
                module,
                context,
                lua_entry,
                native,
            }
            .measure(workload, options);
        }
    }
}

fn main() {
    let options = Options::parse();
    println!("phase,workload,engine,size,batch,sample,ns,checksum");
    if let Some(name) = &options.profile {
        let workload = WORKLOADS
            .iter()
            .find(|workload| workload.name == name)
            .expect("unknown workload");
        run(workload, &options);
        return;
    }
    if options.reverse {
        for workload in WORKLOADS.iter().rev() {
            run(workload, &options);
        }
    } else {
        for workload in WORKLOADS {
            run(workload, &options);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matched_scripts_handle_empty_and_single_element_inputs() {
        let options = Options {
            samples: 1,
            warmups: 0,
            setup_samples: 1,
            reverse: false,
            profile: None,
        };
        for size in [0, 1] {
            for workload in WORKLOADS {
                let small = Workload {
                    name: workload.name,
                    size,
                    batch: 1,
                    kagari: workload.kagari,
                    lua: workload.lua,
                    reference: workload.reference,
                };
                run(&small, &options);
            }
        }
    }
}
