use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::{Arc, Mutex};

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("type-inference.kgr", source),
            Default::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
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
        assert_eq!(
            result
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        drop(result);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn fixed_width_bits_and_shifts() {
    execute(
        r#"
        fn assert(value: bool) { { val passed = value; if !passed {val zero=0;1/zero;} }; }
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
fn generic_bitwise_static_dispatch() {
    execute(
        r#"use std::ops::{BitOr};

        struct Bits { val value: i32 }
        impl BitOr<Bits> for Bits {
            type Output = Bits;
            fn bitor(self, rhs: Bits) -> Bits { Bits { value: self.value | rhs.value } }
        }
        fn combine<T: BitOr<T, Output = T>>(a: T, b: T) -> T { a | b }
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
            .compile_to_artifact(SourceFile::new("shift.kgr", source), Default::default())
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
    use kagari_runtime::host::HostFunction;
    use kagari_types::{
        collection::CollectionAccess,
        host_interface::{HostFunctionDeclaration, HostInterface, value_type::HostValueType},
    };
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

    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "commit.kgr",
                r#"
        fn index(log: Vec<i32>) -> usize { log.push(1); 0usize }
        fn count(log: Vec<i32>) -> u16 { log.push(2); 32u16 }
        fn main() { val memory = demo::memory(); memory[index(memory)] <<= count(memory); }
    "#,
            ),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext {
        ..Default::default()
    };

    let mut runtime = engine.runtime(context.clone());
    let memory_slot = Arc::new(Mutex::new(None));
    let captured_memory = memory_slot.clone();
    runtime
        .register_host_function(HostFunction::new(declaration, move |_, _| {
            Ok(Value::Array(
                captured_memory.lock().unwrap().expect("initialized memory"),
            ))
        }))
        .unwrap();
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    let memory = runtime
        .runtime()
        .alloc_array(&loaded, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
        .unwrap();
    let root = runtime.runtime().root_value(Value::Array(memory)).unwrap();
    *memory_slot.lock().unwrap() = Some(memory);
    let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
    assert!(
        format!("{error:?}").contains("shift out of range"),
        "{error:?}"
    );
    assert_eq!(
        runtime.runtime().gc().array_snapshot(memory).unwrap(),
        vec![Value::I32(7), Value::I32(1), Value::I32(2)]
    );
    drop(root);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn numeric_casts_match_const_and_runtime_rules() {
    execute(
        r#"
        fn assert(value: bool) { { val passed = value; if !passed {val zero=0;1/zero;} }; }
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
fn conversions_reject_implicit_loss_and_invalid_cast_targets() {
    let engine = KagariEngine::default();
    for expr in [
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
            val nested: Vec<Vec<u8>> = [[8u8 >> 1]];
            { val passed = nested[0][0] == 4u8; if !passed {val zero=0;1/zero;} };
            stop()
        }
    "#,
    );
}

#[test]
fn invalid_numeric_artifact_contracts_are_rejected_before_execution() {
    use kagari_bytecode::instruction::BytecodeInstruction;
    use kagari_types::scalar::BuiltinType;
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("conversion.kgr", "fn main() -> u8 { 256u16 as u8 }"),
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
                Default::default()
            )
            .is_err()
    );
}
