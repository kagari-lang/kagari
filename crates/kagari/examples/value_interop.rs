//! An embedding consumer defines DTOs and an Opaque payload without core edits.
use kagari::{context::ExecutionContext, engine::KagariEngine};
use kagari_derive::KagariValue;
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, conversion::KagariType,
    declarations::FunctionDecl, payload::Opaque, typed::NativeContext,
};
use kagari_source::source::SourceFile;
use kagari_types::ty::Ty;

kagari_runtime::native_data! { struct Config { multiplier: i32 } }

#[derive(KagariValue)]
struct Request {
    damage: i32,
    config: Opaque<Config>,
}

#[derive(Debug, PartialEq, KagariValue)]
#[kagari(name = "Damage")]
struct Response {
    amount: i32,
}

#[derive(Debug, PartialEq, KagariValue)]
enum Outcome {
    Empty,
    Applied(Response),
}

fn main() {
    let mut builder = KagariEngine::builder().unwrap();
    let mut module = ModuleBuilder::new("game::damage", builder.declarations()).unwrap();
    let request = module.register_type::<Request>().unwrap();
    let outcome = module.register_type::<Outcome>().unwrap();
    let config_type = Opaque::<Config>::kagari_type(module.declarations()).unwrap();
    let process = module
        .define_function(
            FunctionDecl::new("process")
                .parameter("request", request)
                .returns(outcome),
        )
        .unwrap();
    module
        .bind(
            process,
            |cx: &mut NativeContext<'_>, (request,): (Request,)| -> NativeResult<Outcome> {
                let multiplier = request.config.read(cx, |config| Ok(config.multiplier))?;
                Ok(Outcome::Applied(Response {
                    amount: request.damage * multiplier,
                }))
            },
        )
        .unwrap();
    builder.install(module.finish().unwrap()).unwrap();
    let engine = builder.build().unwrap();
    let program = engine
        .compile_to_program(
            SourceFile::new(
                "value.kgr",
                r#"
        use game::damage::{Request, Outcome, process};
        pub fn run(request: Request) -> Outcome { process(request) }
    "#,
            ),
            Default::default(),
        )
        .unwrap();

    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let selected = runtime.runtime().select_program(&loaded).unwrap();
    let Ty::NativeObject(nominal) = config_type.abi() else {
        unreachable!()
    };
    let descriptor = runtime
        .runtime()
        .bind_native_type_declaration::<Config>(
            &loaded.root(),
            nominal.declaration,
            config_type.definitions().unwrap(),
            &[],
        )
        .unwrap();
    let config = descriptor
        .create(
            &mut NativeContext::new(runtime.runtime(), &selected).unwrap(),
            Config { multiplier: 2 },
        )
        .unwrap();
    let result: Outcome = runtime
        .execute_typed(&loaded, "run", (Request { damage: 20, config },), &context)
        .unwrap();
    assert_eq!(result, Outcome::Applied(Response { amount: 40 }));
    println!("{result:?}");
}
