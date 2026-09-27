use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = kagari_embed::EngineConfig::default();
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
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let result = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
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
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
        assert!(format!("{error:?}").contains("shift out of range"));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn failed_shift_keeps_target_and_completed_rhs_effects() {
    use kagari_common::{
        collection::CollectionAccess,
        host_interface::{HostFunctionDeclaration, HostInterface, HostValueType},
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
    let profile = kagari_runtime::LanguageProfile {
        allow_host_calls: true,
        ..Default::default()
    };
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "commit.kgr",
                r#"
        fn index(log: MutableArray<i32>) -> usize { log.push(1); 0usize }
        fn count(log: MutableArray<i32>) -> u16 { log.push(2); 32u16 }
        fn main() { val memory = demo::memory(); memory[index(memory)] <<= count(memory); }
    "#,
            ),
            kagari_embed::CompileOptions {
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
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
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
