use kagari_common::source::SourceFile;
use kagari_embed::{context::JitPolicy, engine::EngineConfig};

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
            SourceFile::new("array-operations.kgr", source),
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
fn repeats_evaluate_value_elements_once() {
    execute(
        r#"
    struct Cell { var value: i32 }
    fn item(log: ArrayList<i32>) -> i32 { log.push(1); 7 }
    fn count(log: ArrayList<i32>) -> usize { log.push(2); 3 }
    fn main() -> i32 {
        val log = [];
        val values = [item(log); count(log)];
        if !(log.len() == 2usize && log[0] == 1 && log[1] == 2) { return 0; }
        values[0] = 42;
        if values[2] != 7 { return 0; }
        val empty = [item(log); 0];
        if !(empty.is_empty() && log.len() == 3usize) { return 0; }
        val inferred: ArrayList<u8> = [1; 4];
        if inferred[3] != 1u8 { return 0; }
        42
    }
    "#,
    );
}

#[test]
fn invalid_repeat_counts_and_read_only_mutations_are_compile_errors() {
    let engine = KagariEngine::default();
    for source in [
        "fn main() { val a = [0; -1]; }",
        "fn main() { val a = [0; true]; }",
        "fn main() { val a = [0; 1i32]; }",
        "fn main() { val a = [1, 2; 3]; }",
        "fn main() { val a: List<i32> = [0; 2]; a.push(1); }",
        "fn main() { val a: List<i32> = [0; 2]; a[0] = 1; }",
        "fn main() { val r = true..false; }",
        "fn main() { val r = 1.0..2.0; }",
        "fn wrong(r: Range<bool>) {} fn main() {}",
        "fn main() { val r = ..=; }",
        "fn main() { for n in ..3 {} }",
        "fn main() { for n in .. {} }",
        "fn main() { val r: ArrayList<i32> = 0..3; }",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn range_values_iterate_lazily_and_preserve_integer_width() {
    execute(
        r#"
    use core::language as foundation;
    fn identity<T>(range: Range<T>) -> Range<T> { range }
    fn upper<R: RangeBounds<usize>>(range: R) -> Bound<usize> { range.end_bound() }
    fn main() -> i32 {
        val qualified: foundation::Range<u8> = identity(1u8..4u8);
        val bound: foundation::Bound<u8> = qualified.start_bound();
        if bound != foundation::Bound::Included(1u8) { return 0; }
        var last = 0i8;
        for n in -128i8..=-126i8 { last = n; }
        if last != -126i8 { return 0; }
        if upper(..) != Bound::Unbounded { return 0; }
        val range: Range<u8> = 1u8..4u8;
        var total = 0u8;
        for n in range { total += n; }
        for n in range { total += n; }
        if total != 12u8 { return 0; }
        var count = 0usize;
        var maximum = 0u8;
        for n in 254u8..=255u8 { count += 1; maximum = n; }
        if count != 2usize || maximum != 255u8 { return 0; }
        var wide = 0u64;
        for n in 0u64..18446744073709551615u64 { wide = n; if n == 2u64 { break; } }
        if wide != 2u64 { return 0; }
        var open = 0i16;
        for n in 10i16.. { open = n; if n == 11i16 { break; } }
        if open != 11i16 { return 0; }
        42
    }
    "#,
    );
}

#[test]
fn repeated_arrays_reject_mutable_identity_even_when_nested_or_empty() {
    let engine = KagariEngine::default();
    for body in [
        "val a = [Cell { value: 1 }; 2];",
        "val a = [Cell { value: 1 }; 0];",
        "val a = [[1, 2]; 2];",
        "val a = [(Cell { value: 1 }, 7); 2];",
        "val a: ArrayList<Option<Cell>> = [None; 2];",
        "val a = [Wrapped::Data(Cell { value: 1 }); 2];",
        "val a: ArrayList<Wrapped<Cell>> = [Wrapped::Empty; 2];",
        "val a = [|| 1; 2];",
    ] {
        let source = format!(
            "struct Cell {{ var value: i32 }} enum Wrapped<T> {{ Empty, Data(T) }} fn main() {{ {body} }}"
        );
        let error = engine
            .compile_to_artifact(
                SourceFile::new("invalid-repeat.kgr", source),
                Default::default(),
                Default::default(),
            )
            .unwrap_err();
        assert!(
            format!("{error:?}").contains("initialize each element separately"),
            "{body}: {error:?}"
        );
    }
}

#[test]
fn repeated_value_aggregates_keep_value_semantics() {
    execute(
        r#"
    enum Wrapped<T> { Empty, Data(T) }
    fn main() -> i32 {
        val enums = [Wrapped::Data((1, "hello")); 2];
        val options: ArrayList<Option<i32>> = [None; 2];
        val strings = ["hello"; 2];
        if enums[0] == enums[1] && options[0] == options[1] && strings[0] == strings[1] { 42 } else { 0 }
    }
    "#,
    );
}
