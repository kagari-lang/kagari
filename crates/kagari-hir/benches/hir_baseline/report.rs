//! CLI and JSON schema shared by both executables. Reporting is never a timed operation.

use std::{env, process, time::Instant};

use serde_json::{Value, json};

use crate::workloads::{GENERATION, Workload};

pub struct Options {
    pub workload: Workload,
    pub check: bool,
    pub inputs: bool,
}

impl Options {
    pub fn parse() -> Self {
        match Self::try_parse() {
            Ok(options) => options,
            Err(error) => {
                eprintln!(
                    "{error}\nusage: --workload plain|generic|dependency_edit --size 24|96|384 [--check] [--inputs]"
                );
                process::exit(2);
            }
        }
    }

    fn try_parse() -> Result<Self, String> {
        let mut name = String::from("plain");
        let mut size = 96;
        let mut check = false;
        let mut inputs = false;
        let mut seen = Vec::new();
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            if seen.contains(&arg) {
                return Err(format!("duplicate argument: {arg}"));
            }
            seen.push(arg.clone());
            match arg.as_str() {
                "--workload" => name = args.next().ok_or("missing workload")?,
                "--size" => {
                    size = args
                        .next()
                        .ok_or("missing size")?
                        .parse::<usize>()
                        .map_err(|_| "size must be a positive integer")?;
                }
                "--check" => check = true,
                "--inputs" => inputs = true,
                // Cargo supplies this flag even with harness = false.
                "--bench" => {}
                _ => return Err(format!("unknown argument: {arg}")),
            }
        }
        if !matches!(name.as_str(), "plain" | "generic" | "dependency_edit") {
            return Err(format!("unknown workload: {name}"));
        }
        if !matches!(size, 24 | 96 | 384) || (name == "dependency_edit" && size != 96) {
            return Err(format!("unsupported size {size} for {name}"));
        }
        Ok(Self {
            workload: Workload::new(&name, size),
            check,
            inputs,
        })
    }

    pub fn manifest(&self, memory: bool) -> Value {
        let workload = &self.workload;
        json!({
            "schema": 1,
            "workload": workload.name,
            "size": workload.size,
            "input_hash": workload.input_hash,
            "source_bytes": workload.source_bytes(),
            "source_files": workload.inputs.len(),
            "scenario_names": workload.scenario_names(memory),
            "generation": GENERATION,
            "logical_user_modules": workload.inputs.len(),
            "source_bytes_scope": "original physical user sources, excluding providers and edit revisions",
            "input_hash_algorithm": "blake3, length-framed original sources, edit revisions, filenames and routes",
        })
    }

    pub fn result(&self, memory: bool, provider_setup_ns: f64, scenarios: Vec<Value>) -> Value {
        let mut result = self.manifest(memory);
        let object = result.as_object_mut().unwrap();
        object.remove("scenario_names");
        object.insert(
            "mode".into(),
            json!(if memory { "memory" } else { "timing" }),
        );
        object.insert("check_only".into(), json!(self.check));
        object.insert("warmups".into(), json!(usize::from(!memory && !self.check)));
        object.insert("provider_setup_ns".into(), json!(provider_setup_ns));
        object.insert("scenarios".into(), json!(scenarios));
        result
    }
}

pub fn measure<T>(operation: impl FnOnce() -> T) -> (T, f64) {
    let start = Instant::now();
    let result = operation();
    // Preserve the clock's actual resolution, including a possible zero for tiny drops.
    let elapsed = start.elapsed().as_secs_f64() * 1_000_000_000.0;
    (result, elapsed)
}

pub fn print(value: &Value) {
    println!("{}", serde_json::to_string(value).unwrap());
}
