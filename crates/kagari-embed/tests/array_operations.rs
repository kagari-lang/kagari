use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let mut config = kagari_embed::EngineConfig::default();
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
fn repeats_evaluate_value_elements_once() {
    execute(
        r#"
    struct Cell { var value: i32 }
    fn item(log: ArrayList<i32>) -> i32 { log.push(1); 7 }
    fn count(log: ArrayList<i32>) -> usize { log.push(2); 3 }
    fn main() -> i32 {
        val log = [];
        val values = [item(log); count(log)];
        std::debug::assert(log.len() == 2usize && log[0] == 1 && log[1] == 2, "order");
        values[0] = 42;
        std::debug::assert(values[2] == 7, "independent slots");
        val empty = [item(log); 0];
        std::debug::assert(empty.is_empty() && log.len() == 3usize, "zero still evaluates");
        val inferred: ArrayList<u8> = [1; 4];
        std::debug::assert(inferred[3] == 1u8, "context");
        42
    }
    "#,
    );
}

#[test]
fn bulk_operations_preserve_aliases_and_allow_replacement_during_iteration() {
    execute(
        r#"
    struct Cell { var value: i32 }
    fn main() -> i32 {
        val array = [0; 3];
        val view: List<i32> = array;
        array.fill(7);
        std::debug::assert(view[2] == 7, "alias");
        array.copy_from([10, 20, 30]);
        array.copy_from(view);
        std::debug::assert(array[0] == 10 && array[2] == 30, "self copy");
        for value in array { array.fill(42); }
        val cell = Cell { value: 1 };
        val cells = ArrayList::from_fn(2, |i| cell);
        val copied = ArrayList::from_fn(2, |i| Cell { value: 0 });
        copied.copy_from(cells);
        copied[0].value = 42;
        std::debug::assert(cells[1].value == 42, "shallow copy");
        val empty: ArrayList<i32> = [];
        empty.fill(0);
        empty.copy_from([]);
        array[0]
    }
    "#,
    );
}

#[test]
fn invalid_repeat_counts_and_read_only_mutations_are_compile_errors() {
    let engine = KagariEngine::default();
    for source in [
        "fn main() { val a = ArrayList::from_fn(-1, |i| i); }",
        "fn main() { val a = ArrayList::from_fn(1i32, |i| i); }",
        "fn main() { val a = ArrayList::from_fn(1, |a, b| a); }",
        "fn main() { val a: ArrayList<u8> = ArrayList::from_fn(1, |i| 1i32); }",
        "fn main() { val a = [0; -1]; }",
        "fn main() { val a = [0; true]; }",
        "fn main() { val a = [0; 1i32]; }",
        "fn main() { val a = [1, 2; 3]; }",
        "fn main() { val a: List<i32> = [0; 2]; a.fill(1); }",
        "fn main() { val a: List<i32> = [0; 2]; a.copy_from([1, 2]); }",
        "fn main() { val a = [0; 2]; a.copy_from([true, false]); }",
        "fn main() { val a: List<i32> = [0; 2]; a.copy_within(.., 0); }",
        "fn main() { val a = [0; 2]; a.copy_within(0i32..1i32, 0); }",
        "fn main() { val a = [0; 2]; a.copy_within(0..1, 0i32); }",
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
fn array_operation_example() {
    execute(include_str!(
        "../../../examples/syntax/array-operations.kgr"
    ));
}

#[test]
fn range_values_iterate_lazily_and_preserve_integer_width() {
    execute(
        r#"
    fn identity<T>(range: Range<T>) -> Range<T> { range }
    fn upper<R: RangeBounds<usize>>(range: R) -> Bound<usize> { range.end_bound() }
    fn main() -> i32 {
        val qualified: std::ops::Range<u8> = identity(1u8..4u8);
        val bound: std::ops::Bound<u8> = qualified.start_bound();
        std::debug::assert(bound == std::ops::Bound::Included(1u8), "qualified");
        val range: Range<u8> = 1u8..4u8;
        val negative = (-128i8..=-126i8).iter().collect::<ArrayList<i8>>();
        std::debug::assert(negative[2] == -126i8, "signed endpoints");
        val upper: Bound<usize> = upper(..);
        std::debug::assert(upper == Bound::Unbounded, "qualified full bound");
        var total = 0u8;
        for n in range { total += n; }
        for n in range { total += n; }
        std::debug::assert(total == 12u8, "fresh cursors");
        val high = (254u8..=255u8).iter().collect::<ArrayList<u8>>();
        std::debug::assert(high.len() == 2usize && high[1] == 255u8, "inclusive max");
        val wide = (0u64..18446744073709551615u64).iter().take(3usize).collect::<ArrayList<u64>>();
        std::debug::assert(wide[2] == 2u64, "lazy wide range");
        val unbounded = (10i16..).iter().take(2usize).collect::<ArrayList<i16>>();
        std::debug::assert(unbounded[1] == 11i16, "open range");
        42
    }
    "#,
    );
}

#[test]
fn copy_within_accepts_all_range_forms_and_custom_bounds() {
    execute(
        r#"
    struct Region { val log: ArrayList<i32> }
    impl RangeBounds<usize> for Region {
        fn start_bound(self) -> Bound<usize> { self.log.push(1); Bound::Excluded(0) }
        fn end_bound(self) -> Bound<usize> { self.log.push(2); Bound::Included(2) }
    }
    fn copy<R: RangeBounds<usize>>(array: ArrayList<i32>, range: R, destination: usize) {
        array.copy_within(range, destination);
    }
    fn main() -> i32 {
        val a = [1, 2, 3, 4];
        a.copy_within(0..3, 1);
        std::debug::assert(a[1] == 1 && a[3] == 3, "forward overlap");
        a.copy_within(1..=3, 0);
        std::debug::assert(a[0] == 1 && a[2] == 3, "backward overlap");
        a.copy_within(..2, 2);
        a.copy_within(..=0, 1);
        a.copy_within(3.., 0);
        a.copy_within(.., 0);
        a.copy_within(4..4, 4);
        val log = [];
        a.copy_from([1, 2, 3, 4]);
        copy(a, Region { log }, 0);
        std::debug::assert(a[0] == 2 && a[1] == 3, "custom bounds");
        std::debug::assert(log.len() == 2usize && log[0] == 1 && log[1] == 2, "bound calls once");
        copy(a, 0..2, 2);
        std::debug::assert(a[2] == 2 && a[3] == 3, "generic inference");
        val range = 1u8..=3u8;
        std::debug::assert(range.start_bound() == Bound::Included(1u8), "start");
        std::debug::assert(range.end_bound() == Bound::Included(3u8), "end");
        42
    }
    "#,
    );
}

#[test]
fn failed_interval_copy_keeps_completed_argument_and_bound_effects() {
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
    for (body, expected) in [
        ("a.copy_within(3..1, 0);", vec![1, 2, 3]),
        (
            "a.copy_within(..=18446744073709551615usize, 0);",
            vec![1, 2, 3],
        ),
        ("a.copy_within(3..3, 4);", vec![1, 2, 3]),
        ("a.copy_from([1, 2]);", vec![1, 2, 3]),
        ("a.swap(0usize, 9usize);", vec![1, 2, 3]),
        ("for x in a { a.reverse(); }", vec![1, 2, 3]),
        ("for x in a { a.swap_remove(0usize); }", vec![1, 2, 3]),
        ("a.copy_within(Region { a }, 0);", vec![9, 2]),
        (
            r#"val result = ArrayList::from_fn(3, |i| { a.push(i as i32); std::debug::assert(i < 1usize, "callback failed"); Region { a } });"#,
            vec![1, 2, 3, 0, 1],
        ),
    ] {
        let source = format!(
            r#"
        struct Region {{ val a: ArrayList<i32> }}
        impl RangeBounds<usize> for Region {{
            fn start_bound(self) -> Bound<usize> {{ self.a[0] = 9; Bound::Included(0) }}
            fn end_bound(self) -> Bound<usize> {{ self.a.pop(); Bound::Excluded(3) }}
        }}
        fn main() {{ val a = demo::memory(); {body} }}
        "#
        );
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("failure.kgr", source),
                kagari_embed::CompileOptions {
                    language_profile: profile,
                },
                Default::default(),
            )
            .unwrap();
        for encoded in [false, true] {
            let artifact = if encoded {
                BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
            } else {
                artifact.clone()
            };
            let mut context = ExecutionContext {
                language_profile: profile,
                ..Default::default()
            };
            context.capabilities.host_calls = true;
            context.host_policy.allowed_host_functions = vec!["demo.memory".into()];
            let mut runtime = engine.runtime(context.clone());
            let memory = runtime
                .runtime()
                .alloc_array(vec![Value::I32(1), Value::I32(2), Value::I32(3)])
                .unwrap();
            let root = runtime
                .runtime()
                .gc()
                .root_value(Value::Array(memory))
                .unwrap();
            runtime
                .register_host_function(HostFunction::new(declaration.clone(), move |_, _| {
                    Ok(Value::Array(memory))
                }))
                .unwrap();
            let loaded = runtime.load_program(artifact, Default::default()).unwrap();
            let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
            assert_eq!(
                runtime.runtime().gc().array_snapshot(memory).unwrap(),
                expected.iter().copied().map(Value::I32).collect::<Vec<_>>(),
                "{body}: {error:?}"
            );
            drop(root);
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
        }
    }
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
            format!("{error:?}").contains("ArrayList::from_fn"),
            "{body}: {error:?}"
        );
    }
}

#[test]
fn repeated_value_aggregates_and_per_element_initializers() {
    execute(
        r#"
    enum Wrapped<T> { Empty, Data(T) }
    struct Cell { var value: usize }
    fn build<T>(value: T) -> ArrayList<T> { ArrayList::from_fn(2, |i| value) }
    fn main() -> i32 {
        val enums = [Wrapped::Data((1, "hello")); 2];
        std::debug::assert(enums[0] == enums[1], "value enum");
        val options: ArrayList<Option<i32>> = [None; 2];
        val strings = ["hello"; 2];
        val cells = ArrayList::from_fn(3, |i| Cell { value: i });
        cells[0].value = 42usize;
        std::debug::assert(cells[1].value == 1usize && cells[2].value == 2usize, "independent");
        var calls = 0;
        val empty: ArrayList<Cell> = ArrayList::from_fn(0, |i| { calls += 1; Cell { value: i } });
        std::debug::assert(calls == 0 && empty.is_empty(), "zero callbacks");
        val log = [];
        val ordered = ArrayList::from_fn({ log.push(9); 2usize }, { log.push(10); |i| { log.push(i as i32); i } });
        std::debug::assert(log.len() == 4usize && log[0] == 9 && log[1] == 10 && log[2] == 0 && log[3] == 1, "argument and callback order");
        val indices = ArrayList::from_fn(4, |i| { calls += 1; i });
        std::debug::assert(calls == 4 && indices[3] == 3usize, "indices and call count");
        val shared = build(cells[0]);
        shared[0].value = 7usize;
        std::debug::assert(shared[1].value == 7usize, "explicit sharing");
        42
    }
    "#,
    );
}

#[test]
fn array_initialization_termination_releases_execution_roots() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "initialization-limit.kgr",
                r#"
        struct Cell { var value: usize }
        fn main() { val cells = ArrayList::from_fn(1000, |i| Cell { value: i }); }
        fn healthy() -> i32 { 42 }
    "#,
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for case in 0..3 {
        let mut context = ExecutionContext::default();
        if case == 0 {
            context.resources.max_instruction_steps = Some(40);
        }
        if case == 1 {
            context.resources.max_heap_units = Some(8);
        }
        if case == 2 {
            context.cancellation.cancel();
        }
        let mut runtime = engine.runtime(Default::default());
        let loaded = runtime
            .load_program(artifact.clone(), Default::default())
            .unwrap();
        let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
        assert_eq!(
            error.code(),
            if case == 2 {
                "KG_RUNTIME_CANCELLED"
            } else {
                "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED"
            }
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert!(runtime.runtime().execution_root().is_none());
        assert_eq!(
            runtime
                .execute(&loaded, "healthy", &[], &Default::default())
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
}
