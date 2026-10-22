use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;

mod boundaries;
mod contracts;
mod math;
mod semantics;
mod transport;

#[derive(Clone, Copy)]
struct Float {
    ty: &'static str,
    uint: &'static str,
    sign: u64,
    infinity: u64,
    quiet_nan: u64,
    signaling_nan: u64,
}

const FLOATS: [Float; 2] = [
    Float {
        ty: "f32",
        uint: "u32",
        sign: 0x8000_0000,
        infinity: 0x7f80_0000,
        quiet_nan: 0x7fc1_2345,
        signaling_nan: 0x7f81_2345,
    },
    Float {
        ty: "f64",
        uint: "u64",
        sign: 0x8000_0000_0000_0000,
        infinity: 0x7ff0_0000_0000_0000,
        quiet_nan: 0x7ff8_1234_5678_9abc,
        signaling_nan: 0x7ff0_1234_5678_9abc,
    },
];

impl Float {
    fn bits(self, value: f64) -> u64 {
        if self.ty == "f32" {
            u64::from((value as f32).to_bits())
        } else {
            value.to_bits()
        }
    }

    fn value(self, bits: u64) -> String {
        format!("{}::from_bits({bits}{})", self.ty, self.uint)
    }

    fn patterns(self) -> Vec<u64> {
        let normal = if self.ty == "f32" { 1 << 23 } else { 1 << 52 };
        [
            0,
            1,
            normal - 1,
            normal,
            normal + 1,
            self.bits(1.0),
            self.bits(1.0) + 1,
            self.infinity - 1,
            self.infinity,
            self.quiet_nan,
            self.quiet_nan + 1,
            self.signaling_nan,
            self.infinity + 1,
        ]
        .into_iter()
        .flat_map(|bits| [bits, bits | self.sign])
        .collect()
    }
}

fn engine() -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::new(config)
}

fn context() -> ExecutionContext {
    ExecutionContext {
        jit_policy: JitPolicy::Disabled,
        ..Default::default()
    }
}

fn programs(source: &str) -> [(&'static str, PreparedProgram); 2] {
    let compiler = engine();
    let checked = compiler
        .compile_source(SourceFile::new("float-methods.kgr", source))
        .unwrap_or_else(|error| panic!("{error:?}\n{source}"));
    let direct = compiler.prepare(&checked, Default::default()).unwrap();
    let artifact = compiler
        .emit_artifact(&checked, Default::default())
        .unwrap();
    let decoded =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let encoded =
        PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    [("source", direct), ("encoded", encoded)]
}

fn check_source(checks: Vec<String>) -> String {
    // Bound per-function MIR work without raising compiler resource limits.
    let mut source = String::new();
    let mut main = String::from("fn main() -> i32 {\n");
    for (group, checks) in checks.chunks(6).enumerate() {
        source.push_str(&format!("fn checks{group}() -> i32 {{\n"));
        for (offset, check) in checks.iter().enumerate() {
            let index = group * 6 + offset + 1;
            source.push_str(&format!(
                "{{ val passed = {check}; if !passed {{ return {index}; }} }}\n"
            ));
        }
        source.push_str("0 }\n");
        main.push_str(&format!(
            "{{ val result = checks{group}(); if result != 0 {{ return result; }} }}\n"
        ));
    }
    source.push_str(&main);
    source.push_str("0 }\n");
    source
}

fn execute_checks(checks: Vec<String>) {
    let source = check_source(checks);
    for (route, program) in programs(&source) {
        assert_checks(&program, route, &source);
    }
}

fn assert_checks(program: &PreparedProgram, route: &str, source: &str) {
    let consumer = engine();
    let context = context();
    let mut runtime = consumer.runtime(context.clone());
    let loaded = runtime.load_program(program, Default::default()).unwrap();
    let result = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap_or_else(|error| panic!("{route}: {error:?}\n{source}"));
    assert_eq!(
        result.return_value.value(runtime.runtime().gc()).unwrap(),
        Value::I32(0),
        "{route}: nonzero result identifies the failing check (one-based)\n{source}"
    );
    drop(result);
    assert_eq!(runtime.runtime().gc().active_roots(), 0, "{route}");
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    assert!(!runtime.runtime().is_quarantined());
}
