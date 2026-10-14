//! A consumer opts into Serde DTOs without exposing Rust methods or object IDs.
use kagari::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_derive::KagariSchema;
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, declarations::FunctionDecl, serde::Serde,
    typed::NativeContext,
};
use kagari_source::source::SourceFile;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, KagariSchema)]
#[serde(rename = "DamageRequest", rename_all = "camelCase")]
struct Request {
    target_id: i64,
    damage: i32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, KagariSchema)]
enum DamageResult {
    Applied(i64, i32),
    Rejected(String),
}

fn main() {
    let mut builder = KagariEngine::builder().unwrap();
    let mut module = ModuleBuilder::new("game::serde", builder.declarations()).unwrap();
    let request = module.register_type::<Serde<Request>>().unwrap();
    let result = module.register_type::<Serde<DamageResult>>().unwrap();
    let process = module
        .define_function(
            FunctionDecl::new("process")
                .parameter("request", request)
                .returns(result),
        )
        .unwrap();
    module
        .bind(
            process,
            |_: &mut NativeContext<'_>,
             (Serde(request),): (Serde<Request>,)|
             -> NativeResult<Serde<DamageResult>> {
                Ok(Serde(if request.damage < 0 {
                    DamageResult::Rejected("negative damage".into())
                } else {
                    DamageResult::Applied(request.target_id, request.damage)
                }))
            },
        )
        .unwrap();
    builder.install(module.finish().unwrap()).unwrap();
    let engine = builder.build().unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "serde.kgr",
                r#"
        use game::serde::{DamageRequest, DamageResult, process};
        pub fn run(request: DamageRequest) -> DamageResult { process(request) }
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
    let Serde(result): Serde<DamageResult> = runtime
        .execute_typed(
            &loaded,
            "run",
            (Serde(Request {
                target_id: 7,
                damage: 20,
            }),),
            &context,
        )
        .unwrap();
    assert_eq!(result, DamageResult::Applied(7, 20));
    println!("{result:?}");
}
