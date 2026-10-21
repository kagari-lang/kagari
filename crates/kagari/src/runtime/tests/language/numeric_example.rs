use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;

const SOURCE: &str = include_str!("../../../../../../examples/numeric-sampling.kgr");
const EXPECTED_CHECKSUM: u64 = 0x396d_503b_b05c_761a;

fn engine() -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::new(config)
}

fn reference_checksum() -> u64 {
    // Independent host recurrence from Vigna's 2015 public-domain reference:
    // https://prng.di.unimi.it/splitmix64.c
    let mut state = 0u64;
    let words: [u64; 64] = std::array::from_fn(|_| {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let first = (state ^ (state >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        let second = (first ^ (first >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        second ^ (second >> 31)
    });
    assert_eq!(
        words[..3],
        [
            0xe220_a839_7b1d_cdaf,
            0x6e78_9e6a_a1b9_65f4,
            0x06c4_5d18_8009_454f,
        ]
    );
    words.into_iter().fold(0u64, |checksum, word| {
        // floor((word >> 11) * 2^-53 * 2^16) = word >> 48.
        // Use integer arithmetic here rather than duplicating the script's f64 path.
        let mixed = checksum.wrapping_mul(0x0100_0000_01b3) ^ word;
        mixed.wrapping_mul(0x0100_0000_01b3) ^ (word >> 48)
    })
}

#[test]
fn numeric_example_source_and_encoded_are_deterministic_and_release_roots() {
    assert_eq!(reference_checksum(), EXPECTED_CHECKSUM);
    let programs = {
        let compiler = engine();
        let checked = compiler
            .compile_source(SourceFile::new("examples/numeric-sampling.kgr", SOURCE))
            .unwrap();
        let direct = compiler.prepare(&checked, Default::default()).unwrap();
        let artifact = compiler
            .emit_artifact(&checked, Default::default())
            .unwrap();
        let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
        let decoded = BytecodeArtifact::from_bytes(&bytes).unwrap();
        let encoded =
            PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default())
                .unwrap();
        [("source", direct), ("encoded", encoded)]
    };
    for (route, program) in programs {
        let consumer = engine();
        let context = ExecutionContext {
            jit_policy: JitPolicy::Disabled,
            ..Default::default()
        };
        let mut runtime = consumer.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        // Reuse the same loaded program and heap: RNG state must be local to each call.
        for run in 1..=2 {
            for (entry, expected) in [
                (ProgramEntry::Main, Value::I32(42)),
                (
                    ProgramEntry::Export("sampling_checksum"),
                    Value::U64(EXPECTED_CHECKSUM),
                ),
            ] {
                let result = runtime
                    .execute(&loaded, entry, &[], &context)
                    .unwrap_or_else(|error| panic!("{route} run {run}: {error:?}"));
                assert_eq!(
                    result.return_value.value(runtime.runtime().gc()).unwrap(),
                    expected,
                    "{route} run {run}: {entry:?}"
                );
                drop(result);
                assert_eq!(
                    runtime.runtime().gc().active_roots(),
                    0,
                    "{route} run {run}"
                );
                assert_eq!(
                    runtime.runtime().resources().counters().current_call_depth,
                    0,
                    "{route} run {run}"
                );
                assert!(!runtime.runtime().is_quarantined(), "{route} run {run}");
            }
            println!("{route} run {run}: main=42 checksum=0x{EXPECTED_CHECKSUM:016x} roots=0");
        }
    }
}
