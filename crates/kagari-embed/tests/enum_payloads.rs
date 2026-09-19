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

fn variant(
    module: &kagari_runtime::LoadedModule,
    name: &str,
) -> kagari_runtime::module::EnumVariantRef {
    let slot = module
        .bytecode
        .enumerations
        .iter()
        .position(|layout| layout.declaration.path.last().unwrap().name == name)
        .unwrap();
    module
        .enum_variant(kagari_ir::bytecode::EnumId::new(slot), 0)
        .unwrap()
}

#[test]
fn enum_values_retain_versions_and_reject_foreign_or_changed_payload_layouts() {
    use kagari_runtime::{
        RuntimeErrorKind,
        builtin::invoke_standard,
        value::{EnumTag, Value},
        value_semantics::script_equal,
    };
    let engine = KagariEngine::default();
    let mut runtime = engine.runtime(Default::default());
    let source = "enum Option { Some(i32) } enum Other { Some(i32) } enum Holder { Data(Option, [i32]) } fn main() -> Option { Option::Some(42) }";
    let original = compile(&engine, source);
    let loaded = runtime
        .load_program(original.clone(), Default::default())
        .unwrap();
    let option = variant(&loaded, "Option");
    let holder = variant(&loaded, "Holder");
    let report = runtime
        .execute(&loaded, "main", &[], &Default::default())
        .unwrap();
    let root = runtime
        .runtime()
        .root_value(report.return_value.clone())
        .unwrap();
    let Value::Enum(handle) = report.return_value else {
        panic!("constructed enum")
    };
    assert_eq!(
        runtime.runtime().gc().enum_snapshot(handle).unwrap().fields,
        [Value::I32(42)]
    );
    assert!(
        invoke_standard(
            runtime.runtime().gc(),
            kagari_ir::bytecode::StandardIntrinsic::OptionIsSome,
            &[Value::Enum(handle)]
        )
        .is_err()
    );
    let standard = runtime
        .runtime()
        .alloc_enum(EnumTag::OptionSome, vec![Value::I32(42)])
        .unwrap();
    assert!(
        !script_equal(
            runtime.runtime().gc(),
            &Value::Enum(handle),
            &Value::Enum(standard)
        )
        .unwrap()
    );
    let before = runtime.runtime().resources().counters().allocation_units;
    for fields in [vec![], vec![Value::Bool(true)]] {
        assert_eq!(
            runtime
                .runtime()
                .alloc_enum(EnumTag::Declared(option.clone()), fields)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ScriptTrap
        );
    }
    let mut foreign_runtime = engine.runtime(Default::default());
    let foreign = foreign_runtime
        .load_program(original, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .runtime()
            .alloc_enum(
                EnumTag::Declared(variant(&foreign, "Option")),
                vec![Value::I32(42)]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        runtime.runtime().resources().counters().allocation_units,
        before
    );
    let array = runtime.runtime().alloc_array(vec![Value::I32(1)]).unwrap();
    let wrong = runtime
        .runtime()
        .alloc_enum(
            EnumTag::Declared(variant(&loaded, "Other")),
            vec![Value::I32(42)],
        )
        .unwrap();
    assert!(
        runtime
            .runtime()
            .alloc_enum(
                EnumTag::Declared(holder.clone()),
                vec![Value::Enum(wrong), Value::Array(array)]
            )
            .is_err()
    );
    assert!(
        runtime
            .runtime()
            .alloc_enum(
                EnumTag::Declared(holder.clone()),
                vec![Value::Enum(handle), Value::Array(array)]
            )
            .is_ok()
    );
    let old_key = loaded.key();
    let changed = compile(
        &engine,
        &source
            .replace("Some(i32)", "Some(String)")
            .replace("Some(42)", "Some(\"new\")"),
    );
    let new = runtime
        .reload_program(&loaded, changed, Default::default())
        .unwrap();
    assert!(
        runtime
            .runtime()
            .alloc_enum(
                EnumTag::Declared(variant(&new, "Holder")),
                vec![Value::Enum(handle), Value::Array(array)]
            )
            .is_err()
    );
    drop(holder);
    drop(option);
    drop(loaded);
    runtime.runtime().collect_garbage().unwrap();
    let snapshot = runtime.runtime().gc().enum_snapshot(handle).unwrap();
    let EnumTag::Declared(retained) = snapshot.tag else {
        panic!("nominal variant")
    };
    assert_eq!(retained.module().key(), old_key);
    assert_eq!(snapshot.fields, [Value::I32(42)]);
    assert!(
        runtime
            .runtime()
            .alloc_enum(EnumTag::Declared(retained), vec![Value::I32(7)])
            .is_ok()
    );
    drop(root);
    runtime.runtime().collect_garbage().unwrap();
    assert!(runtime.runtime().gc().enum_snapshot(handle).is_none());
    assert!(
        runtime
            .runtime()
            .alloc_enum(EnumTag::OptionSome, vec![Value::Enum(handle)])
            .is_err()
    );
}

#[test]
fn imported_enum_constructors_use_the_pinned_dependency_layouts() {
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::SourceLayer,
    };
    let engine = KagariEngine::default();
    for (name, value) in [("left", 7), ("right", 9)] {
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
        engine.set_source(&path, format!("pub enum Event {{ Data(i32) }} pub fn make() -> Event {{ Event::Data({value}) }}"), SourceLayer::Base).unwrap();
    }
    let artifact = compile(
        &engine,
        "use pkg::left; use pkg::right; fn main() -> bool { left::make() == left::Event::Data(7) && right::make() == right::Event::Data(9) }",
    );
    let decoded = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime.load_program(decoded, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &Default::default())
            .unwrap()
            .return_value,
        kagari_runtime::value::Value::Bool(true)
    );
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
    old_format.header.format_version = 11;
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
