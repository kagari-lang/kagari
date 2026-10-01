use kagari_common::source::SourceFile;
use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};

use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("standard-traits.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let mut context = ExecutionContext::default();
        context.capabilities.jit = jit;
        context.language_profile.allow_jit = jit;
        context.jit_policy = if jit {
            JitPolicy::Enabled
        } else {
            JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let result = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value, Value::I32(42));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn builtin_and_custom_parsers() {
    execute(
        r#"
struct Count { val value: i32 }
impl FromStr for Count {
    type Err = String;
    fn from_str(text: String) -> Result<Self, String> {
        if text == "answer" { Ok(Count { value: 42 }) } else { Err("not a count") }
    }
}
fn parsed<T: FromStr>(text: String) -> Result<T, <T as FromStr>::Err> { text.parse::<T>() }
fn main() -> i32 {
    val inferred: Result<u8, ParseError> = "255".parse();
    std::debug::assert(inferred == Ok(255u8), "context inference");
    std::debug::assert("42".parse::<i32>() == Ok(42), "decimal");
    std::debug::assert("-128".parse::<i8>() == Ok(-128i8), "signed minimum");
    std::debug::assert("255".parse::<u8>() == Ok(255u8), "unsigned maximum");
    std::debug::assert("256".parse::<u8>() == Err(ParseError::OutOfRange), "overflow");
    std::debug::assert("".parse::<i32>() == Err(ParseError::Empty), "empty");
    std::debug::assert(" 42".parse::<i32>() == Err(ParseError::InvalidDigit), "no trim");
    std::debug::assert("1_000".parse::<i32>().is_err(), "no underscores");
    std::debug::assert("0xff".parse::<u8>().is_err(), "no prefix");
    std::debug::assert("+42".parse::<u16>() == Ok(42u16), "plus");
    std::debug::assert("-1".parse::<u64>().is_err(), "unsigned minus");
    std::debug::assert(u8::from_str_radix("ff", 16u32) == Ok(255u8), "hex");
    std::debug::assert(i16::from_str_radix("-80", 16u32) == Ok(-128i16), "signed hex");
    std::debug::assert(i32::from_str_radix("z", 36u32) == Ok(35), "radix 36");
    std::debug::assert(i32::from_str_radix("1", 1u32) == Err(ParseError::InvalidRadix), "bad radix");
    std::debug::assert("true".parse::<bool>() == Ok(true), "bool");
    std::debug::assert("TRUE".parse::<bool>() == Err(ParseError::InvalidSyntax), "exact bool");
    std::debug::assert("1.5e1".parse::<f64>() == Ok(15.0), "float");
    std::debug::assert("inf".parse::<f64>().is_ok(), "infinity");
    std::debug::assert("NaN".parse::<f32>().is_ok(), "NaN");
    std::debug::assert("1.0 ".parse::<f64>().is_err(), "no float trim");
    std::debug::assert(parsed::<u32>("42") == Ok(42u32), "generic builtin");
    val count: Result<Count, String> = parsed("answer");
    std::debug::assert(count.map_or(0, |x| x.value) == 42, "custom parser");
    std::debug::assert("bad".parse::<Count>().is_err(), "custom error");
    std::debug::assert(i32::from_str("42") == Ok(42), "trait call");
    std::debug::assert("127".parse::<i8>() == Ok(127i8), "i8 max");
    std::debug::assert("128".parse::<i8>() == Err(ParseError::OutOfRange), "i8 overflow");
    std::debug::assert("32767".parse::<i16>() == Ok(32767i16), "i16 max");
    std::debug::assert("32768".parse::<i16>() == Err(ParseError::OutOfRange), "i16 overflow");
    std::debug::assert("2147483647".parse::<i32>() == Ok(2147483647i32), "i32 max");
    std::debug::assert("2147483648".parse::<i32>() == Err(ParseError::OutOfRange), "i32 overflow");
    std::debug::assert("9223372036854775807".parse::<i64>() == Ok(9223372036854775807i64), "i64 max");
    std::debug::assert("9223372036854775808".parse::<i64>() == Err(ParseError::OutOfRange), "i64 overflow");
    std::debug::assert("9223372036854775807".parse::<isize>() == Ok(9223372036854775807isize), "isize max");
    std::debug::assert("9223372036854775808".parse::<isize>() == Err(ParseError::OutOfRange), "isize overflow");
    std::debug::assert("255".parse::<u8>() == Ok(255u8), "u8 max");
    std::debug::assert("256".parse::<u8>() == Err(ParseError::OutOfRange), "u8 overflow");
    std::debug::assert("65535".parse::<u16>() == Ok(65535u16), "u16 max");
    std::debug::assert("65536".parse::<u16>() == Err(ParseError::OutOfRange), "u16 overflow");
    std::debug::assert("4294967295".parse::<u32>() == Ok(4294967295u32), "u32 max");
    std::debug::assert("4294967296".parse::<u32>() == Err(ParseError::OutOfRange), "u32 overflow");
    std::debug::assert("18446744073709551615".parse::<u64>() == Ok(18446744073709551615u64), "u64 max");
    std::debug::assert("18446744073709551616".parse::<u64>() == Err(ParseError::OutOfRange), "u64 overflow");
    std::debug::assert("18446744073709551615".parse::<usize>() == Ok(18446744073709551615usize), "usize max");
    std::debug::assert("18446744073709551616".parse::<usize>() == Err(ParseError::OutOfRange), "usize overflow");
    42
}
"#,
    );
}
