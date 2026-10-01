//! Run MutableList through a native package and inspect its generated declaration view.
use kagari_common::SourceFile;
use kagari_embed::{ExecutionContext, KagariEngine, program::PreparedProgram};
use kagari_runtime::value::Value;

fn main() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://mutable-list.kgr",
                r#"
            fn update<L: MutableList<i32>>(values: L) {
                values.set(0usize, 20);
            }
            fn main() -> i32 {
                val values = [0, 0];
                update(values);
                val mutable: MutableList<i32> = values;
                mutable.set(1usize, 22);
                values[0usize] + values[1usize]
            }
        "#,
            ),
            Default::default(),
            Default::default(),
        )
        .expect("compile native MutableList calls");
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
            .expect("verify native MutableList program");
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&program, Default::default())
        .expect("link registered native API");
    let result = runtime
        .execute(&loaded, "main", &[], &context)
        .expect("execute MutableList calls")
        .return_value;
    assert_eq!(result, Value::I32(42));
    println!("MutableList native execution: {result:?}");
    for source in engine.native_declaration_sources() {
        println!("{}\n{}", source.uri, source.text);
    }
}
