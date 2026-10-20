//! Explicit private source-trait references and common native interface binding.
//! Run with `cargo run -p kagari --example host_interfaces`.
use kagari::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::PreparedProgram,
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, metadata::DefinitionMetadata,
};
use kagari_runtime::{
    host::{HostCallContext, HostError, HostObjectId, HostSchemaEpoch},
    module::entry::ProgramEntry,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, types::Type},
    value::Value,
};
use kagari_source::source_database::SourceLayer;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::declaration::host::{
    HostCallPolicy, HostFunctionEffects, HostPassingStyle, HostReflectionPolicy, HostTypeOwnership,
    HostTypePolicy, PathAccess,
};

const SOURCE: &str = include_str!("../../../examples/host-interfaces.kgr");
const SOURCE_URI: &str = "mem://host-interface";

fn compile_offline() -> (KagariEngine, DefinitionMetadata<BytecodeArtifact>) {
    // Read the trait's checked ABI independently of its host implementation.
    // The source owns this private trait; Rust does not declare or export it.
    let author = KagariEngine::default();
    let contract_source = SOURCE
        .split("\nfn read<")
        .next()
        .unwrap()
        .replace("use demo::{Counter, make};", "");
    let file = author
        .set_source(SOURCE_URI, contract_source, SourceLayer::Base)
        .unwrap();
    let checked = author
        .compile_snapshot(author.source_snapshot(), file, &Default::default())
        .unwrap();
    let contract = author.emit_artifact(&checked, Default::default()).unwrap();
    let source_module =
        &contract.records().program.modules[contract.records().program.root.index()];
    let reader = source_module
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Reader")
        .unwrap();
    let path = DefinitionPath {
        module: source_module.identity.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Reader".into(),
            occurrence: 0,
        }],
    };
    let reader = DefinitionMetadata::checked(
        contract.definitions().clone(),
        reader.abi.clone(),
        &Default::default(),
    )
    .unwrap();
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut module = ModuleBuilder::new("demo", &catalog).unwrap();
    let reader = module.reference_trait(&path, reader).unwrap();
    let mut counter = module.define_type("Counter").unwrap();
    counter
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadOnly,
        })
        .unwrap();
    let counter = counter.finish().unwrap();
    module
        .implement(counter.clone(), |group| {
            group.trait_impl(reader.apply([])?, |methods| {
                methods.associated_type("Item", Type::i32())?;
                methods.host_policy(
                    "read",
                    HostCallPolicy {
                        passing: vec![HostPassingStyle::SharedBorrow, HostPassingStyle::Owned],
                        effects: Default::default(),
                    },
                )?;
                methods.bind("read", |_: &HostCallContext<'_>, args: &[Value]| {
                    let [Value::HostRoot(_), Value::I32(amount)] = args else {
                        return Err(HostError::new("checked Counter receiver and i32 argument"));
                    };
                    Ok(Value::I32(*amount))
                })
            })
        })
        .unwrap();
    let make = module
        .define_function(
            FunctionDecl::new("make")
                .returns(counter.apply([]).unwrap())
                .host_policy(HostCallPolicy {
                    passing: vec![],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    module
        .bind(make, |cx: &HostCallContext<'_>, _: &[Value]| {
            let root = cx.runtime().host().root(HostObjectId(7)).unwrap();
            cx.runtime()
                .gc()
                .alloc_host_root(root)
                .map_err(|error| HostError::new(error.to_string()))
        })
        .unwrap();
    // One checked installation owns both declarations and callbacks. Compilation
    // consumes the cold declarations and never executes either callback.
    let engine =
        KagariEngine::with_native_modules(Default::default(), vec![module.finish().unwrap()])
            .unwrap();
    let file = engine.bind_module(SOURCE_URI, path.module).unwrap();
    engine
        .set_source(SOURCE_URI, SOURCE.into(), SourceLayer::Base)
        .unwrap();
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap();
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    (engine, artifact)
}

fn main() {
    let (engine, artifact) = compile_offline();
    let context = ExecutionContext {
        jit_policy: JitPolicy::Disabled,
        ..Default::default()
    };
    let mut runtime = engine.runtime(context.clone());
    let ty = runtime
        .runtime()
        .types()
        .get_by_name("demo::Counter")
        .unwrap()
        .id;
    runtime
        .runtime_mut()
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let result = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap();
    let value = result.return_value.value(runtime.runtime().gc()).unwrap();
    assert_eq!(value, Value::I32(42));
    println!("static + dynamic host interface result: {value:?}");
}
