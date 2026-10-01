//! Run MutableList through a native package and inspect its generated declaration view.
use kagari_common::SourceFile;
use kagari_embed::{ExecutionContext, KagariEngine, program::PreparedProgram};
use kagari_runtime::{
    NativeAction, NativeContext, NativeFactory, NativeInvocationState, RuntimeError, native_module,
    value::Value,
};

struct Answer;
impl NativeInvocationState for Answer {
    fn advance(&mut self, _: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        Ok(NativeAction::Complete(Value::I32(22)))
    }
}
fn answer() -> NativeFactory {
    NativeFactory::new(0, |_| Ok(Box::new(Answer)))
}

fn main() {
    let application = native_module! {
        module demo::math;
        /// Return a value supplied by the application.
        fn answer() -> i32 => answer;
    }
    .expect("register application native API");
    let engine = KagariEngine::with_native_apis(Default::default(), vec![application])
        .expect("install default and application packages");
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://mutable-list.kgr",
                r#"
            use demo::math::answer;
            fn update<L: MutableList<i32>>(values: L) {
                values.set(0usize, 20);
            }
            fn main() -> i32 {
                val values = [0, 0];
                update(values);
                val mutable: MutableList<i32> = values;
                mutable.set(1usize, answer());
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
