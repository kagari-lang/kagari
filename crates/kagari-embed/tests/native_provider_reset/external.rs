//! The embedding owns this ordinary Rust handler and uses only public native APIs.
use super::{artifact, engine};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_embed::{context::ExecutionContext, engine::EngineConfig, program::PreparedProgram};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::NativeResult, builder::ModuleBuilder, callable::CallableHandle,
        context::CallContext, declarations::FunctionDecl, language::LanguageContracts,
        module::NativeModule, types::Type,
    },
    value::Value,
};

fn fill(
    cx: &mut CallContext<'_>,
    count: usize,
    callback: CallableHandle<'_>,
) -> NativeResult<Value> {
    let value = cx.allocate_result()?;
    let output = cx
        .heap()
        .root_value(value)
        .ok_or_else(|| RuntimeError::module_validation("output root"))?;
    let Value::Array(array) = output.value() else {
        return Err(RuntimeError::module_validation("array output"));
    };
    for index in 0..count {
        let value = callback.call_values(cx, &[Value::U64(index as u64)])?;
        // Protect a heap result before any allocation or further script reentry.
        let value = cx
            .heap()
            .root_value(value)
            .ok_or_else(|| RuntimeError::module_validation("callback result root"))?;
        cx.collect_garbage()?;
        cx.heap().array_push(array, value.value())?;
    }
    Ok(output.value())
}

pub(super) fn module() -> NativeModule {
    let language = LanguageContracts::default();
    let mut module = ModuleBuilder::new("external::fixture", &language);
    let function = module
        .define_function(FunctionDecl::new("from_fn"))
        .unwrap();
    module
        .function(&function, |function| {
            let item = function.type_parameter("T")?;
            function.parameter("count", Type::usize());
            function.parameter("callback", Type::function([Type::usize()], item.ty()));
            function.returns(language.array_list(item.ty()));
            Ok(())
        })
        .unwrap();
    module.bind(function, fill).unwrap();
    module.finish().unwrap()
}

#[test]
fn an_embedding_owned_provider_can_allocate_and_chain_checked_callbacks() {
    let bytes = artifact(
        "fn main() -> i32 { val values = native::from_fn(2usize, |i| { [21] }); values[0usize][0usize] + values[1usize][0usize] }",
    ).to_bytes().unwrap();
    let program = PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    // A fresh engine installs the same checked module without source compilation.
    let mut runtime = engine(config).runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}
