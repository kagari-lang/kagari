//! Query source dependencies, then encode, load and execute their shared program.
use kagari_common::{
    cancellation::CancellationToken,
    identity::{ModuleIdentity, PackageId},
    source_database::SourceLayer,
};
use kagari_embed::KagariEngine;

fn main() {
    let engine = KagariEngine::default();
    for (name, text) in [
        (
            "shared",
            "pub struct Data { val number: i32 } pub fn value() -> i32 { 42 }",
        ),
        ("left", "use demo::shared;"),
        ("right", "use demo::shared;"),
        (
            "root",
            "use demo::left; use demo::right; use demo::shared::value; use demo::shared::Data; fn pass(x: Data) -> i32 { x.number } fn make() -> Data { Data { number: 42 } } fn main() -> i32 { value() }",
        ),
    ] {
        let source = format!("memory://{name}");
        engine.bind_module(&source, identity(name)).unwrap();
        engine
            .set_source(&source, text.into(), SourceLayer::Base)
            .unwrap();
    }
    let snapshot = engine
        .analyze(
            engine.source_snapshot(),
            Default::default(),
            &CancellationToken::default(),
        )
        .unwrap();
    for module in snapshot
        .module_graph()
        .initialization_order(&identity("root"), &CancellationToken::default())
        .unwrap()
    {
        println!("{module}");
    }
    let root = snapshot.module_graph().node(&identity("root")).unwrap();
    let file = snapshot.file(root.file).unwrap();
    let offset = file.source().text().rfind("value()").unwrap();
    let function = file.source_function_at(offset).unwrap();
    println!(
        "imported {} -> {}",
        function.signature.name,
        function.signature.return_type.display_name()
    );
    assert!(file.result().diagnostics().is_empty());
    let type_offset = file.source().text().find("Data)").unwrap();
    let declaration = snapshot.definition_at(root.file, type_offset).unwrap();
    assert_ne!(declaration.location.file, root.file);
    println!(
        "imported type {} has a dependency-owned definition",
        declaration.name
    );
    let checked = snapshot
        .check_program(root.file, &CancellationToken::default())
        .unwrap();
    let ir = kagari_ir::program::lower_program_to_ir(&checked, &Default::default()).unwrap();
    for module in ir.modules() {
        for function in &module.functions {
            let binding = ir.function(&function.instance.declaration).unwrap();
            println!(
                "{}::{} -> module {}, function {}",
                module.identity,
                function.name,
                binding.module,
                binding.function.index()
            );
        }
    }
    let program = kagari_ir::bytecode::lower_program_to_bytecode(&ir).unwrap();
    let artifact =
        kagari_ir::bytecode::KbcArtifact::from_program(program, Default::default()).unwrap();
    let encoded = artifact.to_bytes().unwrap();
    let decoded = kagari_ir::bytecode::KbcArtifact::from_bytes(&encoded).unwrap();
    let context = kagari_embed::ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(decoded, Default::default()).unwrap();
    let report = runtime.execute(&loaded, "main", &[], &context).unwrap();
    assert_eq!(report.return_value, kagari_runtime::value::Value::I32(42));
    println!(
        "{} modules, result {:?}",
        loaded.members().count(),
        report.return_value
    );
}

fn identity(name: &str) -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("demo".into()),
        path: vec![name.into()],
    }
}
