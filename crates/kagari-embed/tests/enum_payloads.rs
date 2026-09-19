use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, KagariEngine};
use kagari_hir::types::BuiltinType;
use kagari_ir::module::{PublicAbiItem, abi::AbiType};

fn compile(engine: &KagariEngine, source: &str) -> BytecodeArtifact {
    let checked = engine
        .compile_source(
            SourceFile::new("memory://events.kgr", source),
            Default::default(),
        )
        .unwrap();
    engine.emit_bytecode(&checked, Default::default()).unwrap()
}

#[test]
fn payload_abi_roundtrips_and_rejects_changed_reload_before_publication() {
    let engine = KagariEngine::default();
    let mut runtime = engine.runtime(Default::default());
    let source = "struct Point { val x: i32 } pub enum Event { Empty, Data(Point, (i32, [String])) } fn main() -> i32 { 42 }";
    let original = compile(&engine, source);
    let module = &original.program.modules[original.program.root.index()];
    let variant = module
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicAbiItem::Type(ty) if ty.name == "Event" => Some(&ty.variants[1]),
            _ => None,
        })
        .unwrap();
    assert!(
        matches!(&variant.payload[0], AbiType::Struct(id) if id.module == module.identity && id.path[0].name == "Point")
    );
    assert_eq!(
        variant.payload[1],
        AbiType::Tuple(vec![
            AbiType::Builtin(BuiltinType::I32),
            AbiType::Array(Box::new(AbiType::Builtin(BuiltinType::String))),
        ])
    );
    let decoded = BytecodeArtifact::from_bytes(&original.to_bytes().unwrap()).unwrap();
    decoded.validate_for_loader(&Default::default()).unwrap();
    assert_eq!(
        decoded.program.modules[decoded.program.root.index()].public_items,
        module.public_items
    );
    let loaded = runtime.load_program(decoded, Default::default()).unwrap();
    let changed = compile(
        &engine,
        &source.replace("(i32, [String])", "(i64, [String])"),
    );
    assert_ne!(
        original.verification.public_abi_fingerprints,
        changed.verification.public_abi_fingerprints
    );
    let before = runtime.runtime().modules().loaded_count();
    let error = runtime
        .reload_program(&loaded, changed, Default::default())
        .unwrap_err();
    assert_eq!(error.code(), "KG_RELOAD_PUBLIC_ABI_FINGERPRINT_MISMATCH");
    assert_eq!(runtime.runtime().modules().loaded_count(), before);
    assert_eq!(
        runtime
            .runtime()
            .modules()
            .latest(&loaded.name)
            .unwrap()
            .epoch,
        loaded.epoch
    );
    // The original entry remains usable after rejection.
    assert!(
        runtime
            .execute(&loaded, "main", &[], &Default::default())
            .is_ok()
    );
    let body_edit = compile(&engine, &source.replace("{ 42 }", "{ 43 }"));
    assert_eq!(
        original.verification.public_abi_fingerprints,
        body_edit.verification.public_abi_fingerprints
    );
    assert!(
        runtime
            .reload_program(&loaded, body_edit, Default::default())
            .is_ok()
    );
    let mut old_format = original;
    old_format.header.format_version = 10;
    assert!(BytecodeArtifact::from_bytes(&old_format.to_bytes().unwrap()).is_err());
}

#[test]
fn same_spelled_payload_types_from_different_modules_have_different_abi() {
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::SourceLayer,
    };
    let engine = KagariEngine::default();
    for name in ["left", "right"] {
        let path = format!("memory://{name}.kgr");
        engine
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        engine
            .set_source(
                &path,
                "pub struct Point { val x: i32 }".into(),
                SourceLayer::Base,
            )
            .unwrap();
    }
    let a = compile(
        &engine,
        "use pkg::left::Point; pub enum Event { Data(Point) } fn main() -> i32 { 1 }",
    );
    let b = compile(
        &engine,
        "use pkg::right::Point; pub enum Event { Data(Point) } fn main() -> i32 { 1 }",
    );
    assert_ne!(
        a.verification.public_abi_fingerprints,
        b.verification.public_abi_fingerprints
    );
}
