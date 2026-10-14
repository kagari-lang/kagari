//! Query source dependencies, then encode, load and execute their shared program.

use kagari_runtime::module::entry::ProgramEntry;
use {kagari_contract::types::PublicItem, kagari_types::ty::Ty};

use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{ModuleIdentity, PackageId, map::DefinitionContext},
};
use kagari_compiler::{bytecode, source::program};
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_source::source_database::SourceLayer;

fn main() {
    let engine = KagariEngine::default();
    for (name, text) in [
        (
            "shared",
            "pub struct Data { pub val number: i32 } pub fn value() -> i32 { 42 }",
        ),
        ("left", "use demo::shared;"),
        ("right", "use demo::shared;"),
        (
            "root",
            "use demo::left; use demo::right; use demo::shared::value; use demo::shared::Data; fn pass(x: Data) -> i32 { x.number } pub fn make() -> Data { Data { number: 42 } } fn main() -> i32 { value() }",
        ),
    ] {
        let source = format!("memory://{name}");
        engine.bind_module(&source, identity(name)).unwrap();
        engine
            .set_source(&source, text.into(), SourceLayer::Base)
            .unwrap();
    }
    let snapshot = engine
        .analyze(engine.source_snapshot(), &CancellationToken::default())
        .unwrap();
    for module in snapshot
        .module_graph()
        .reachable_order(&identity("root"), &CancellationToken::default())
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
        function
            .signature
            .return_type
            .display_name(file.definitions())
    );
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let type_offset = file.source().text().find("Data)").unwrap();
    let declaration = snapshot.definition_at(root.file, type_offset).unwrap();
    assert_ne!(declaration.location.file, root.file);
    let defining_text = snapshot
        .file(declaration.location.file)
        .unwrap()
        .source()
        .text();
    assert_eq!(
        &defining_text[declaration.location.range.start..declaration.location.range.end],
        "Data"
    );
    println!(
        "imported type {} has a dependency-owned definition",
        declaration.name
    );
    let checked = snapshot
        .check_program(root.file, &CancellationToken::default())
        .unwrap();
    let ir = program::lower_program_to_mir(&checked, &Default::default()).unwrap();
    for module in ir.modules() {
        for function in &module.functions {
            let binding = ir.function(&function.instance).unwrap();
            println!(
                "{}::{} -> module {}, function {}",
                module.identity,
                function.name,
                binding.module,
                binding.function.index()
            );
        }
    }
    let program = bytecode::lower_program_to_bytecode(&ir).unwrap();
    let definitions = DefinitionContext::new().unwrap();
    let program = program
        .import_into(&definitions, &Default::default())
        .unwrap();
    let artifact =
        KbcArtifact::from_program(program.into_records(), Default::default(), &definitions)
            .unwrap();
    let encoded = artifact.to_bytes(&definitions.snapshot()).unwrap();
    let decoded = KbcArtifact::from_bytes(&encoded).unwrap();
    let decoded_program = &decoded.records().program;
    let root_bytecode = &decoded_program.modules[decoded_program.root.index()];
    let PublicItem::Function(make) = root_bytecode
        .public_items
        .iter()
        .find(|item| item.name() == "make")
        .unwrap()
    else {
        panic!("public make signature")
    };
    let Ty::Struct(result) = &make.return_type else {
        panic!("nominal return type")
    };
    let result_owner = decoded
        .definitions()
        .resolve(result.declaration)
        .unwrap()
        .module();
    assert_eq!(result_owner, &identity("shared"));
    println!("public return type belongs to {}", result_owner);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let program =
        PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let report = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap();
    assert_eq!(
        report
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        kagari_runtime::value::Value::I32(42)
    );
    let mut second = engine.runtime(ExecutionContext::default());
    assert!(
        second
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .is_err()
    );
    let second_loaded = second.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        second
            .execute(&second_loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(second.runtime().gc())
            .unwrap(),
        kagari_runtime::value::Value::I32(42)
    );
    println!(
        "{} modules, result {:?}",
        loaded.members().count(),
        report
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result")
    );
}

fn identity(name: &str) -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("demo".into()),
        path: vec![name.into()],
    }
}
