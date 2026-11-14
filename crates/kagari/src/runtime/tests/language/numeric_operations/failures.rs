use crate::{context::ExecutionContext, engine::KagariEngine, program::Program};
use kagari_runtime::{
    host::HostError,
    module::entry::ProgramEntry,
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, declarations::FunctionDecl,
        host, types::Type,
    },
    value::carriers::RootedValue,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{declaration::host::HostCallPolicy, scalar::BuiltinType, ty::Ty};
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
        runtime.collect_garbage().unwrap();
        let roots = runtime.gc_stats().root_registrations;
        let error = runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap_err();
        assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
        assert!(error.to_string().contains("shift out of range"));
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().root_registrations, roots);
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
    let memory_slot = Arc::new(Mutex::new(None::<RootedValue>));
    let captured_memory = memory_slot.clone();
    builder
        .bind(
            memory,
            host::callback(move |cx, _| {
                let root = captured_memory
                    .lock()
                    .unwrap()
                    .as_ref()
                    .expect("initialized memory")
                    .clone();
                cx.import(&root)
                    .map_err(|error| HostError::new(error.to_string()))
            }),
        )
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
    let _selected_loaded = runtime.select_program(&loaded).unwrap();
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let memory = runtime
        .alloc_array(
            &_selected_loaded,
            Ty::Builtin(BuiltinType::I32),
            &[
                RootedValue::from(7_i32),
                RootedValue::from(0_i32),
                RootedValue::from(0_i32),
            ],
        )
        .unwrap();
    *memory_slot.lock().unwrap() = Some(memory.clone());
    let error = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
    assert!(
        error.to_string().contains("shift out of range"),
        "{error:?}"
    );
    {
        let mut cx = ConversionContext::new(&mut runtime, &_selected_loaded).unwrap();
        let array = cx.import(&memory).unwrap();
        assert_eq!(cx.array_len(&array).unwrap(), 3);
        let values = (0..3)
            .map(|index| {
                cx.array_element(&array, index)
                    .unwrap()
                    .unwrap()
                    .as_i32()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(values, [7, 1, 2]);
    }
    drop(memory);
    memory_slot.lock().unwrap().take();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
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
