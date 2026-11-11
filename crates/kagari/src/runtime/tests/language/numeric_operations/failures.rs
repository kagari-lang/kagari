use crate::{context::ExecutionContext, engine::KagariEngine, program::Program};
use kagari_runtime::{
    host::HostCallContext,
    module::entry::ProgramEntry,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, types::Type},
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::declaration::host::HostCallPolicy;
use std::sync::{Arc, Mutex};

#[test]
fn shifts_reject_negative_and_width_counts() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    for expression in ["1u8 << 8", "1u8 >> -1", "1u64 << 64u16"] {
        let source = format!("fn main() {{ val value = {expression}; }}");
        let artifact = engine
            .compile_to_artifact(SourceFile::new("shift.kgr", source), Default::default())
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let error = runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap_err();
        assert!(format!("{error:?}").contains("shift out of range"));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn failed_shift_keeps_target_and_completed_rhs_effects() {
    let standard = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new("demo", &standard.catalog().unwrap()).unwrap();
    let memory = builder
        .define_function(
            FunctionDecl::new("memory")
                .returns(Type::array(Type::i32()).unwrap())
                .host_policy(HostCallPolicy {
                    passing: vec![],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    let memory_slot = Arc::new(Mutex::new(None));
    let captured_memory = memory_slot.clone();
    builder
        .bind(memory, move |_: &HostCallContext<'_>, _: &[Value]| {
            Ok(Value::Array(
                captured_memory.lock().unwrap().expect("initialized memory"),
            ))
        })
        .unwrap();
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        stdlib::modules()
            .unwrap()
            .into_iter()
            .chain(vec![builder.finish().unwrap()])
            .collect(),
    )
    .unwrap();

    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "commit.kgr",
                r#"
        use demo;
        fn index(log: [i32]) -> u64 { log[1] = 1; 0u64 }
        fn count(log: [i32]) -> u16 { log[2] = 2; 32u16 }
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
    let loaded_program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    let _selected_loaded = runtime.runtime().select_program(&loaded).unwrap();
    let memory = runtime
        .runtime()
        .alloc_array(
            &_selected_loaded,
            kagari_types::ty::Ty::Builtin(kagari_types::scalar::BuiltinType::I32),
            vec![Value::I32(7), Value::I32(0), Value::I32(0)],
        )
        .unwrap();
    let root = runtime.runtime().root_value(Value::Array(memory)).unwrap();
    *memory_slot.lock().unwrap() = Some(memory);
    let error = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap_err();
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
fn ordinary_narrow_remainder_and_compound_arithmetic_still_trap() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
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
            Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        let program = runtime
            .load_program(&program_program, Default::default())
            .unwrap();
        assert!(
            runtime
                .execute(&program, ProgramEntry::Main, &[], &context)
                .is_err(),
            "{body}"
        );
    }
}
