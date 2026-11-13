use crate::{BytecodeArtifact, engine::KagariEngine, program::Program};
use kagari_bytecode::instruction::EnumId;
use kagari_common::identity::{
    ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::types::PublicItem;
use kagari_runtime::reload::ReloadValidationError;
use kagari_runtime::{
    error::RuntimeErrorKind,
    module::{EnumVariantRef, ExecutableModule, entry::ProgramEntry},
    native::conversion::context::ConversionContext,
    value::{EnumTag, carriers::RootedValue},
    value_semantics::script_equal,
};
use kagari_source::{source::SourceFile, source_database::SourceLayer};
use kagari_stdlib as stdlib;
use kagari_types::{language::binding, scalar::BuiltinType, ty::Ty};

fn compile(engine: &KagariEngine, source: &str) -> DefinitionMetadata<BytecodeArtifact> {
    engine
        .bind_module(
            "memory://events.kgr",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["events".into()],
            },
        )
        .unwrap();
    let checked = engine
        .compile_source(SourceFile::new("memory://events.kgr", source))
        .unwrap();
    engine.emit_artifact(&checked, Default::default()).unwrap()
}

fn variant(module: &ExecutableModule, name: &str) -> EnumVariantRef {
    let slot = module
        .bytecode
        .enumerations
        .iter()
        .position(|layout| {
            module
                .definitions()
                .resolve(layout.declaration)
                .unwrap()
                .segments()
                .last()
                .unwrap()
                .name
                == name
        })
        .unwrap();
    module.enum_variant(EnumId::new(slot), 0).unwrap()
}

#[test]
fn enum_values_retain_versions_and_reject_foreign_or_changed_payload_layouts() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let mut runtime = engine.runtime(Default::default());
    let source = "enum Option { Some(i32) } enum Other { Some(i32) } enum Holder { Data(Option, [i32]) } fn main() -> Option { Option::Some(42) }";
    let original = compile(&engine, source);
    let loaded = runtime
        .load_program(
            &Program::from_artifact(original.clone(), &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    let _selected_loaded = runtime.select_program(&loaded).unwrap();
    let option = variant(&_selected_loaded, "Option");
    let holder = variant(&_selected_loaded, "Holder");
    let report = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &Default::default())
        .unwrap();
    let root = report.return_value;
    let expected = runtime
        .alloc_enum(
            EnumTag::Declared(option.clone()),
            &[RootedValue::from(42_i32)],
        )
        .unwrap();
    assert!(script_equal(&mut runtime, &root, &expected).unwrap());
    drop(expected);
    let name = runtime.reflect_type_of(&root).unwrap();
    assert_eq!(
        ConversionContext::new(&mut runtime, &_selected_loaded)
            .unwrap()
            .decode::<String>(&name)
            .unwrap(),
        "Option"
    );
    drop(name);
    let applied = runtime
        .resolve_type_arguments(
            &_selected_loaded,
            &[binding::option(
                _selected_loaded.definitions(),
                Ty::Builtin(BuiltinType::I32),
            )
            .unwrap()],
        )
        .unwrap()
        .pop()
        .unwrap();
    let standard = runtime
        .make_enum_member(
            &_selected_loaded,
            &applied,
            "Some",
            &[RootedValue::from(42_i32)],
        )
        .unwrap();
    assert!(!script_equal(&mut runtime, &root, &standard).unwrap());
    drop(standard);
    for fields in [vec![], vec![RootedValue::from(true)]] {
        assert_eq!(
            runtime
                .alloc_enum(EnumTag::Declared(option.clone()), &fields)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ScriptTrap
        );
    }
    let mut foreign_runtime = engine.runtime(Default::default());
    let foreign = foreign_runtime
        .load_program(
            &Program::from_artifact(original, &Default::default(), &Default::default()).unwrap(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        runtime
            .alloc_enum(
                EnumTag::Declared(variant(
                    &foreign_runtime.select_program(&foreign).unwrap(),
                    "Option"
                )),
                &[RootedValue::from(42_i32)]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let array = runtime
        .alloc_array(
            &_selected_loaded,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(1_i32)],
        )
        .unwrap();
    let wrong = runtime
        .alloc_enum(
            EnumTag::Declared(variant(&_selected_loaded, "Other")),
            &[RootedValue::from(42_i32)],
        )
        .unwrap();
    assert_eq!(
        runtime
            .alloc_enum(
                EnumTag::Declared(holder.clone()),
                &[wrong.clone(), array.clone()]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    drop(
        runtime
            .alloc_enum(
                EnumTag::Declared(holder.clone()),
                &[root.clone(), array.clone()],
            )
            .unwrap(),
    );
    let old_key = _selected_loaded.key();
    let changed = compile(
        &engine,
        &source
            .replace("Some(i32)", "Some(String)")
            .replace("Some(42)", "Some(\"new\")"),
    );
    let changed =
        Program::from_artifact(changed, &Default::default(), &Default::default()).unwrap();
    assert!(
        runtime
            .program(&loaded)
            .map_err(ReloadValidationError::Runtime)
            .and_then(|base| base.prepare_replacement(&changed, &Default::default()))
            .and_then(|candidate| runtime.replace_program(&loaded, &candidate, Default::default()))
            .is_err()
    );
    assert_eq!(runtime.select_program(&loaded).unwrap().key(), old_key);
    // Frozen layouts reject publication; another installation has independent payload types.
    let new = runtime.load_program(&changed, Default::default()).unwrap();
    assert_eq!(
        runtime
            .alloc_enum(
                EnumTag::Declared(variant(&runtime.select_program(&new).unwrap(), "Holder")),
                &[root.clone(), array.clone()]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    drop((holder, loaded, wrong, array));
    runtime.collect_garbage().unwrap();
    let expected = runtime
        .alloc_enum(
            EnumTag::Declared(option.clone()),
            &[RootedValue::from(42_i32)],
        )
        .unwrap();
    assert!(script_equal(&mut runtime, &root, &expected).unwrap());
    drop(expected);
    runtime.collect_garbage().unwrap();
    let before_release = runtime.gc_stats();
    drop(root);
    runtime.collect_garbage().unwrap();
    let after_release = runtime.gc_stats();
    assert_eq!(
        after_release.objects + 1,
        before_release.objects,
        "the scalar enum has no remaining data owner"
    );
    assert_eq!(
        after_release.root_registrations + 1,
        before_release.root_registrations
    );
    // Immutable layout facts remain usable independently of the previous data instance.
    drop(
        runtime
            .alloc_enum(EnumTag::Declared(option), &[RootedValue::from(7_i32)])
            .unwrap(),
    );
}

#[test]
fn imported_enum_constructors_use_the_pinned_dependency_layouts() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
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
        "use pkg::left; use pkg::right; fn main() -> bool { (match left::make() { left::Event::Data(n) => n == 7 }) && (match right::make() { right::Event::Data(n) => n == 9 }) }",
    );
    let decoded =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime
        .load_program(
            &Program::from_artifact(decoded, &Default::default(), &Default::default()).unwrap(),
            Default::default(),
        )
        .unwrap();
    assert!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &Default::default())
            .unwrap()
            .return_value
            .as_bool()
            .unwrap()
    );
}

#[test]
fn payload_abi_roundtrips_and_rejects_changed_reload_before_publication() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let mut runtime = engine.runtime(Default::default());
    let source = "struct Point { val x: i32 } pub enum Event { Empty, Data(Point, (i32, [String])) } fn main() -> i32 { 42 }";
    let original = compile(&engine, source);
    let definitions = DefinitionContext::new().unwrap();
    let original = original
        .import_into(&definitions, &Default::default())
        .unwrap();
    let module = &original.records().program.modules[original.records().program.root.index()];
    let variant = module
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::Type(ty) if ty.name == "Event" => Some(&ty.variants[1]),
            _ => None,
        })
        .unwrap();
    let Ty::Struct(point) = &variant.payload[0] else {
        panic!("nominal payload")
    };
    let point = original.definitions().resolve(point.declaration).unwrap();
    assert_eq!(point.module(), &module.identity);
    assert_eq!(point.segments().next().unwrap().name, "Point");
    let Ty::Tuple(payload) = &variant.payload[1] else {
        panic!("tuple payload")
    };
    assert_eq!(payload[0], Ty::Builtin(BuiltinType::I32));
    assert_eq!(
        payload[1],
        Ty::Array(Box::new(Ty::Builtin(BuiltinType::String)))
    );
    let decoded =
        BytecodeArtifact::from_bytes(&original.records().to_bytes(original.definitions()).unwrap())
            .unwrap();
    let decoded = decoded
        .import_into(&definitions, &Default::default())
        .unwrap();
    decoded
        .records()
        .validate_for_loader(&Default::default(), &definitions, &Default::default())
        .unwrap();
    assert_eq!(
        decoded.records().program.modules[decoded.records().program.root.index()].public_items,
        module.public_items
    );
    let loaded = runtime
        .load_program(
            &Program::from_artifact(decoded, &Default::default(), &Default::default()).unwrap(),
            Default::default(),
        )
        .unwrap();
    let changed = compile(
        &engine,
        &source.replace("(i32, [String])", "(i64, [String])"),
    );
    assert_ne!(
        original.records().verification.public_abi_fingerprints,
        changed.records().verification.public_abi_fingerprints
    );
    let before = runtime.modules().loaded_count();
    let before_epoch = runtime.select_program(&loaded).unwrap().epoch;
    let error = runtime
        .program(&loaded)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| {
            base.prepare_replacement(
                &Program::from_artifact(changed, &Default::default(), &Default::default()).unwrap(),
                &Default::default(),
            )
        })
        .and_then(|candidate| runtime.replace_program(&loaded, &candidate, Default::default()))
        .unwrap_err();
    assert_eq!(error.code(), "KG_RELOAD_PUBLIC_ABI_FINGERPRINT_MISMATCH");
    assert_eq!(runtime.modules().loaded_count(), before);
    assert_eq!(runtime.select_program(&loaded).unwrap().epoch, before_epoch);
    // The original entry remains usable after rejection.
    assert!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &Default::default())
            .is_ok()
    );
    let body_edit = compile(&engine, &source.replace("{ 42 }", "{ 43 }"));
    assert_eq!(
        original.records().verification.public_abi_fingerprints,
        body_edit.records().verification.public_abi_fingerprints
    );
    assert!(
        runtime
            .program(&loaded)
            .map_err(ReloadValidationError::Runtime)
            .and_then(|base| base.prepare_replacement(
                &Program::from_artifact(body_edit, &Default::default(), &Default::default())
                    .unwrap(),
                &Default::default()
            ))
            .and_then(|candidate| runtime.replace_program(&loaded, &candidate, Default::default()))
            .is_ok()
    );
    let (definitions, mut old_format) = original.into_parts();
    old_format.header.format_version = 11;
    assert!(BytecodeArtifact::from_bytes(&old_format.to_bytes(&definitions).unwrap()).is_err());
}

#[test]
fn same_spelled_payload_types_from_different_modules_have_different_abi() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
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
        a.records().verification.public_abi_fingerprints,
        b.records().verification.public_abi_fingerprints
    );
}
