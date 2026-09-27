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
fn repeats_evaluate_once_and_keep_shallow_identity() {
    execute(
        r#"
    struct Cell { var value: i32 }
    fn item(log: MutableArray<i32>) -> Cell { log.push(1); Cell { value: 7 } }
    fn count(log: MutableArray<i32>) -> usize { log.push(2); 3 }
    fn main() -> i32 {
        val log = [];
        val values = [item(log); count(log)];
        std::debug::assert(log.len() == 2usize && log[0] == 1 && log[1] == 2, "order");
        values[0].value = 42;
        std::debug::assert(values[2].value == 42, "shared object");
        val empty = [item(log); 0];
        std::debug::assert(empty.is_empty() && log.len() == 3usize, "zero still evaluates");
        val inferred: MutableArray<u8> = [1; 4];
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
        val view: Array<i32> = array;
        array.fill(7);
        std::debug::assert(view[2] == 7, "alias");
        array.copy_from_slice([10, 20, 30]);
        array.copy_from_slice(view);
        std::debug::assert(array[0] == 10 && array[2] == 30, "self copy");
        for value in array { array.fill(42); }
        val cell = Cell { value: 1 };
        val cells = [cell; 2];
        val copied = [Cell { value: 0 }; 2];
        copied.copy_from_slice(cells);
        copied[0].value = 42;
        std::debug::assert(cells[1].value == 42, "shallow copy");
        val empty: MutableArray<i32> = [];
        empty.fill(0);
        empty.copy_from_slice([]);
        array[0]
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
        "fn main() { val a: Array<i32> = [0; 2]; a.fill(1); }",
        "fn main() { val a: Array<i32> = [0; 2]; a.copy_from_slice([1, 2]); }",
        "fn main() { val a = [0; 2]; a.copy_from_slice([true, false]); }",
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
