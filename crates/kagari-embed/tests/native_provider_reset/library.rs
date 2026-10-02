use kagari_common::source::SourceFile;
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::value::Value;

#[test]
fn optional_collection_algorithms_use_the_normal_default_installation_path() {
    let engine = KagariEngine::new(Default::default());
    let sources = engine.native_declaration_sources();
    assert_eq!(sources.len(), 1);
    let program = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://sort.kgr",
                r#"
        use std::collections::sort;
        fn main() -> i32 { val values = [3,1,2]; sort(values); values[0] }
    "#,
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let program =
        PreparedProgram::from_artifact(program, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(1)
    );
}

#[test]
fn disabling_optional_modules_keeps_language_collections_and_removes_algorithms() {
    let engine = KagariEngine::builder()
        .default_modules(false)
        .build()
        .unwrap();
    assert!(engine.native_declaration_sources().is_empty());
    assert!(
        engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://missing.kgr",
                    "use std::collections::sort; fn main() { sort([2,1]); }"
                ),
                Default::default(),
                Default::default()
            )
            .is_err()
    );
    let program = engine.compile_to_artifact(SourceFile::new("memory://foundation.kgr", r#"
        fn main() -> i32 {
            val values = [20,22]; val map: HashMap<i32,i32> = HashMap::new(); map.insert(1, values[0]);
            val set: HashSet<i32> = HashSet::new(); set.insert(values[1]);
            if set.contains(22) && map.contains_key(1) { values[0] + values[1] } else { 0 }
        }
    "#), Default::default(), Default::default()).unwrap();
    let program =
        PreparedProgram::from_artifact(program, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}
