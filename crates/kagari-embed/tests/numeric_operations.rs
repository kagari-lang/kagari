use kagari_common::source::SourceFile;
use {
    kagari_embed::{
        context::JitPolicy,
        engine::{EngineConfig, source::CompileOptions},
    },
    kagari_runtime::security::LanguageProfile,
};

use kagari_embed::{
    BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::PreparedProgram,
};
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("type-inference.kgr", source),
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
fn fixed_width_bits_and_shifts() {
    execute(
        r#"
        fn assert(value: bool) { std::debug::assert(value, "numeric assertion"); }
        const MASK: u8 = !0u8;
        fn main() -> i32 {
            var x = 128u8;
            x <<= 1u16;
            x |= 3u8;
            x ^= 1u8;
            x &= 2u8;
            x >>= 1u32;
            assert(x == 1u8);
            assert(MASK == 255u8);
            assert((128u8 << 1) == 0u8);
            assert((-128i8 >> 7u8) == -1i8);
            assert((128u8 >> 7i64) == 1u8);
            assert(!0u64 == 18446744073709551615u64);
            assert((1u16 << 8u8) == 256u16);
            assert((1 | 2 & 0 ^ 4) == 5);
            42
        }
    "#,
    );
}

#[test]
fn bitwise_example() {
    execute(include_str!("../../../examples/syntax/bitwise.kgr"));
}

#[test]
fn generic_bitwise_static_dispatch() {
    execute(
        r#"
        struct Bits { val value: i32 }
        impl std::ops::BitOr<Bits> for Bits {
            type Output = Bits;
            fn bitor(self, rhs: Bits) -> Bits { Bits { value: self.value | rhs.value } }
        }
        fn combine<T: std::ops::BitOr<T, Output = T>>(a: T, b: T) -> T { a | b }
        fn main() -> i32 { combine(Bits { value: 32 }, Bits { value: 10 }).value }
    "#,
    );
}

#[test]
fn shifts_reject_negative_and_width_counts() {
    let engine = KagariEngine::default();
    for expression in ["1u8 << 8", "1u8 >> -1", "1u64 << 64u16"] {
        let source = format!("fn main() {{ val value = {expression}; }}");
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("shift.kgr", source),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
        assert!(format!("{error:?}").contains("shift out of range"));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn failed_shift_keeps_target_and_completed_rhs_effects() {
    use kagari_common::{
        collection::CollectionAccess,
        host_interface::{HostFunctionDeclaration, HostInterface, value_type::HostValueType},
    };
    use kagari_runtime::host::HostFunction;
    let declaration = HostFunctionDeclaration::new(
        "demo.memory",
        vec![],
        HostValueType::Array(Box::new(HostValueType::I32), CollectionAccess::Mutable),
    );
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface {
            functions: vec![declaration.clone()],
            ..Default::default()
        })
        .unwrap();
    let profile = LanguageProfile {
        allow_host_calls: true,
        ..Default::default()
    };
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "commit.kgr",
                r#"
        fn index(log: ArrayList<i32>) -> usize { log.push(1); 0usize }
        fn count(log: ArrayList<i32>) -> u16 { log.push(2); 32u16 }
        fn main() { val memory = demo::memory(); memory[index(memory)] <<= count(memory); }
    "#,
            ),
            CompileOptions {
                language_profile: profile,
            },
            Default::default(),
        )
        .unwrap();
    let mut context = ExecutionContext {
        language_profile: profile,
        ..Default::default()
    };
    context.capabilities.host_calls = true;
    context.host_policy.allowed_host_functions = vec!["demo.memory".into()];
    let mut runtime = engine.runtime(context.clone());
    let memory = runtime.runtime().alloc_array(vec![Value::I32(7)]).unwrap();
    runtime
        .register_host_function(HostFunction::new(declaration, move |_, _| {
            Ok(Value::Array(memory))
        }))
        .unwrap();
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
    assert!(
        format!("{error:?}").contains("shift out of range"),
        "{error:?}"
    );
    assert_eq!(
        runtime.runtime().gc().array_snapshot(memory).unwrap(),
        vec![Value::I32(7), Value::I32(1), Value::I32(2)]
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn explicit_integer_policies() {
    execute(
        r#"
        fn assert(value: bool) { std::debug::assert(value, "integer policy"); }
        fn main() -> i32 {
            assert(255u8.wrapping_add(1u8) == 0u8);
            assert(0u16.wrapping_sub(1u16) == 65535u16);
            assert(200u8.wrapping_mul(2u8) == 144u8);
            assert(255u8.checked_add(1u8) == None);
            assert(0u8.checked_sub(1u8) == None);
            assert(200u8.checked_mul(2u8) == None);
            assert(7u8.checked_div(0u8) == None);
            assert((-128i8).checked_div(-1i8) == None);
            assert((-128i8).checked_rem(-1i8) == None);
            assert(42u8.checked_rem(5u8) == Some(2u8));
            assert(255u8.overflowing_add(1u8) == (0u8, true));
            assert(0u8.overflowing_sub(1u8) == (255u8, true));
            assert(127i8.overflowing_add(1i8) == (-128i8, true));
            assert(255u8.saturating_add(1u8) == 255u8);
            assert((-128i8).saturating_sub(1i8) == -128i8);
            assert((-128i8).saturating_mul(-1i8) == 127i8);
            assert(18446744073709551615u64.wrapping_mul(18446744073709551615u64) == 1u64);
            assert(18446744073709551615u64.saturating_mul(18446744073709551615u64) == 18446744073709551615u64);
            assert(0u16.wrapping_add_signed(-1i16) == 65535u16);
            assert(128u8.rotate_left(1u32) == 1u8);
            assert(1u8.rotate_right(9u32) == 128u8);
            42
        }
    "#,
    );
}

#[test]
fn numeric_casts_match_const_and_runtime_rules() {
    execute(
        r#"
        fn assert(value: bool) { std::debug::assert(value, "cast"); }
        const BYTE: u8 = 256u16 as u8;
        const SIGN: i8 = 255u8 as i8;
        const CLAMP: u8 = 999.75 as u8;
        fn main() -> i32 {
            assert(BYTE == 0u8);
            assert(SIGN == -1i8);
            assert(CLAMP == 255u8);
            assert((-1i8 as u16) == 65535u16);
            assert((255u16 as i8) == -1i8);
            assert((65535u16 as u8) == 255u8);
            assert((65535u16 as f64) == 65535.0);
            assert((16777217u32 as f32) == 16777216.0f32);
            assert((42.9 as u8) == 42u8);
            assert((-42.9 as i8) == -42i8);
            assert((-42.9 as u8) == 0u8);
            assert(((0.0 / 0.0) as u8) == 0u8);
            assert(((1.0 / 0.0) as u64) == 18446744073709551615u64);
            assert((true as u8) == 1u8);
            assert((false as i32) == 0);
            assert((2u8 as u16 * 21u16) == 42u16);
            42
        }
    "#,
    );
}

#[test]
fn builtin_numeric_conversion_traits() {
    execute(
        r#"
        use std::convert::{From, TryFrom, TryFromIntError, Infallible};
        fn widen<T: From<u8>>(value: u8) -> T { T::from(value) }
        fn checked<T: TryFrom<u16>>(value: u16) -> Result<T, T::Error> { T::try_from(value) }
        fn narrow(value: u16) -> Result<u8, TryFromIntError> { Ok(u8::try_from(value)?) }
        fn assert(value: bool) { std::debug::assert(value, "numeric conversion"); }
        fn main() -> i32 {
            assert(u16::from(255u8) == 255u16);
            assert(widen::<u32>(42u8) == 42u32);
            val wide: u16 = 42u8.into();
            assert(wide == 42u16);
            assert(f64::from(65535u16) == 65535.0);
            assert(u8::from(true) == 1u8);
            assert(narrow(255u16) == Ok(255u8));
            assert(checked::<u8>(256u16).is_err());
            assert(narrow(256u16) == Err(TryFromIntError::OutOfRange));
            assert(u64::try_from(-1i8) == Err(TryFromIntError::OutOfRange));
            val fail: Result<u8, TryFromIntError> = 256u16.try_into();
            assert(fail.is_err());
            val safe: Result<u16, Infallible> = u16::try_from(42u8);
            assert(safe == Ok(42u16));
            assert(i8::try_from(127u16) == Ok(127i8));
            assert(i8::try_from(128u16).is_err());
            42
        }
    "#,
    );
}

#[test]
fn numeric_conversion_example() {
    execute(include_str!(
        "../../../examples/syntax/numeric-conversions.kgr"
    ));
}

#[test]
fn conversions_reject_implicit_loss_and_invalid_cast_targets() {
    let engine = KagariEngine::default();
    for expr in [
        "u8::from(256u16)",
        "i8::from(255u8)",
        "u16::from(-1i8)",
        "f32::from(16777217u32)",
        "f64::from(1u64)",
        "u8::try_from(1.0)",
        "1u8 as bool",
        "true as f64",
        "1u8 as String",
        "1u8 as Missing",
    ] {
        let result = engine.compile_to_artifact(
            SourceFile::new(
                "invalid-cast.kgr",
                format!("fn main() {{ val value = {expr}; }}"),
            ),
            Default::default(),
            Default::default(),
        );
        assert!(result.is_err(), "unexpectedly accepted {expr}");
    }
}

#[test]
fn casts_respect_early_return_and_nested_generics() {
    execute(
        r#"
        fn stop() -> i32 { (if true { return 42; } else { return 1; }) as u8; 0 }
        fn main() -> i32 {
            val nested: ArrayList<ArrayList<u8>> = [[8u8 >> 1]];
            std::debug::assert(nested[0][0] == 4u8, "generic closers");
            stop()
        }
    "#,
    );
}

#[test]
fn invalid_numeric_artifact_contracts_are_rejected_before_execution() {
    use kagari_abi::scalar::BuiltinType;
    use kagari_bytecode::instruction::BytecodeInstruction;
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("conversion.kgr", "fn main() -> u8 { 256u16 as u8 }"),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for target in [BuiltinType::Bool, BuiltinType::String] {
        let mut forged = artifact.clone();
        let conversion = forged
            .program
            .modules
            .iter_mut()
            .flat_map(|m| &mut m.functions)
            .flat_map(|f| &mut f.instructions)
            .find_map(|instruction| match instruction {
                BytecodeInstruction::Convert { conversion, .. } => Some(conversion),
                _ => None,
            })
            .unwrap();
        conversion.target = target;
        assert!(forged.validate_for_loader(&Default::default()).is_err());
    }
}

#[test]
fn hardware_numeric_example_with_artifacts_and_gc() {
    execute(include_str!("../../../examples/6502-numeric.kgr"));
}

#[test]
fn ordinary_narrow_remainder_and_compound_arithmetic_still_trap() {
    let engine = KagariEngine::default();
    for body in [
        "val n = -128i8; val r = n % -1i8;",
        "var n = 255u8; n += 1u8;",
        "var n = -32768i16; n %= -1i16;",
        "var n = 255u8; n *= 2u8;",
    ] {
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("checked.kgr", format!("fn main() {{ {body} }}")),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let program_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let program = runtime
            .load_program(&program_program, Default::default())
            .unwrap();
        assert!(
            runtime.execute(&program, "main", &[], &context).is_err(),
            "{body}"
        );
    }
    assert!(
        engine
            .compile_to_artifact(
                SourceFile::new(
                    "checked-const.kgr",
                    "const VALUE: i8 = -128i8 % -1i8; fn main() -> i8 { VALUE }"
                ),
                Default::default(),
                Default::default()
            )
            .is_err()
    );
}
