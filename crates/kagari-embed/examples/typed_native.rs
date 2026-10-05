//! Documented native registration and owned host arguments/results.
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, registration::FunctionSpec, typed::NativeContext,
};
use kagari_source::source::SourceFile;

fn main() {
    let mut builder = KagariEngine::builder().unwrap();
    let mut text = ModuleBuilder::new("application::text", builder.declarations());
    text.documentation("# Text utilities\n\nApplication-owned Unicode text functions.");
    text.add_function(
        FunctionSpec::new("split")
            .parameter_names(["text", "separator"])
            .documentation("Split text around each literal separator, preserving empty fields.")
            .parameter_documentation("text", "The text to split.")
            .parameter_documentation("separator", "A literal separator, which may be empty.")
            .return_documentation("An owned sequence of fields, in source order."),
        |cx: &mut NativeContext<'_>,
         (text, separator): (String, String)|
         -> NativeResult<Vec<String>> {
            cx.collect(text.split(&separator).map(str::to_owned))
        },
    )
    .unwrap();
    builder.install(text.finish().unwrap()).unwrap();
    let engine = builder.build().unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "typed.kgr",
                r#"
        use application::text::split;
        fn main(text: String) -> Vec<String> { split(text, ",") }
    "#,
            ),
            Default::default(),
        )
        .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let fields: Vec<String> = runtime
        .execute_typed(&loaded, "main", ("hello,雪,".to_owned(),), &context)
        .unwrap();
    assert_eq!(fields, ["hello", "雪", ""]);
    println!("{fields:?}");
}
