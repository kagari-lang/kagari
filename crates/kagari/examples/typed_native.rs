//! Documented native registration and owned host arguments/results.
use kagari::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, conversion::KagariType,
    declarations::FunctionDecl, typed::NativeContext,
};
use kagari_source::source::SourceFile;

fn main() {
    let mut builder = KagariEngine::builder().unwrap();
    let mut text = ModuleBuilder::new("application::text", builder.declarations()).unwrap();
    text.documentation("# Text utilities\n\nApplication-owned Unicode text functions.");
    let string = String::kagari_type(text.declarations()).unwrap();
    let strings = Vec::<String>::kagari_type(text.declarations()).unwrap();
    let split = text
        .define_function(
            FunctionDecl::new("split")
                .parameter("text", string.clone())
                .parameter("separator", string)
                .returns(strings)
                .documentation(concat!(
                    "Split text around each literal separator, preserving empty fields.",
                    "\n\n# Parameters\n\n## `text`\n\nThe text to split.\n",
                    "\n## `separator`\n\nA literal separator, which may be empty.\n",
                    "\n\n# Returns\n\nAn owned sequence of fields, in source order.",
                )),
        )
        .unwrap();
    text.bind(
        split,
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
        pub fn split_text(text: String) -> Vec<String> { split(text, ",") }
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
        .execute_typed(&loaded, "split_text", ("hello,雪,".to_owned(),), &context)
        .unwrap();
    assert_eq!(fields, ["hello", "雪", ""]);
    println!("{fields:?}");
}
