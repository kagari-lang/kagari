//! Documented native registration and owned host arguments/results.
use kagari::{context::ExecutionContext, engine::KagariEngine};
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, conversion::KagariType,
    declarations::FunctionDecl, typed::NativeContext,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;

fn main() {
    let mut builder = KagariEngine::builder().unwrap();
    builder.install_all(stdlib::modules().unwrap()).unwrap();
    let mut text = ModuleBuilder::new("application::text", builder.declarations()).unwrap();
    text.documentation("# Text utilities\n\nApplication-owned Unicode text functions.");
    let string = String::kagari_type(text.declarations()).unwrap();
    let pair = Option::<(String, String)>::kagari_type(text.declarations()).unwrap();
    let split = text
        .define_function(
            FunctionDecl::new("split_once")
                .parameter("text", string.clone())
                .parameter("separator", string)
                .returns(pair)
                .documentation(concat!(
                    "Split text around the first literal separator, preserving empty fields.",
                    "\n\n# Parameters\n\n## `text`\n\nThe text to split.\n",
                    "\n## `separator`\n\nA literal separator, which may be empty.\n",
                    "\n\n# Returns\n\nAn optional owned pair of fields, in source order.",
                )),
        )
        .unwrap();
    text.bind(
        split,
        |_cx: &mut NativeContext<'_>,
         (text, separator): (String, String)|
         -> NativeResult<Option<(String, String)>> {
            Ok(text
                .split_once(&separator)
                .map(|(head, tail)| (head.to_owned(), tail.to_owned())))
        },
    )
    .unwrap();
    builder.install(text.finish().unwrap()).unwrap();
    let engine = builder.build().unwrap();
    let program = engine
        .compile_to_program(
            SourceFile::new(
                "typed.kgr",
                r#"
        use application::text::split_once;
        pub fn split_text(text: String) -> Option<(String, String)> { split_once(text, ",") }
    "#,
            ),
            Default::default(),
        )
        .unwrap();

    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let fields: Option<(String, String)> = runtime
        .execute_typed(&loaded, "split_text", ("hello,雪,".to_owned(),), &context)
        .unwrap();
    assert_eq!(fields, Some(("hello".to_owned(), "雪,".to_owned())));
    println!("{fields:?}");
}
