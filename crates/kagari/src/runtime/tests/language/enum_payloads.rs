use crate::{BytecodeArtifact, engine::KagariEngine, program::PreparedProgram};
use kagari_bytecode::instruction::EnumId;
use kagari_common::identity::{
    ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::types::PublicItem;
use kagari_runtime::{
    error::RuntimeErrorKind,
    module::{EnumVariantRef, ExecutableModule, entry::ProgramEntry},
    reflection,
    value::{EnumTag, Value},
    value_semantics::script_equal_transport,
};
use kagari_source::{source::SourceFile, source_database::SourceLayer};
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
    let engine = KagariEngine::default();
    let mut runtime = engine.runtime(Default::default());
    let source = "enum Option { Some(i32) } enum Other { Some(i32) } enum Holder { Data(Option, [i32]) } fn main() -> Option { Option::Some(42) }";
    let original = compile(&engine, source);
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(
                original.clone(),
                &Default::default(),
                &Default::default(),
            )
            .unwrap(),
            Default::default(),
        )
        .unwrap();
    let _selected_loaded = runtime.runtime().select_program(&loaded).unwrap();
    let option = variant(&_selected_loaded, "Option");
    let holder = variant(&_selected_loaded, "Holder");
    let report = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &Default::default())
        .unwrap();
    let root = report.return_value;
    let Value::Enum(handle) =
        unsafe { root.value_transport(runtime.runtime().gc()) }.expect("retained execution result")
    else {
        panic!("constructed enum")
    };
    assert_eq!(
        unsafe { runtime.runtime().gc().enum_snapshot_transport(handle) }
            .unwrap()
            .fields,
        [Value::I32(42)]
    );
    let Value::Str(name) =
        unsafe { reflection::type_of_transport(runtime.runtime().gc(), &Value::Enum(handle)) }
            .unwrap()
    else {
        panic!("enum type name")
    };
    assert_eq!(
        &*unsafe { runtime.runtime().gc().string_transport(name) }.unwrap(),
        "Option"
    );
    let standard = unsafe {
        runtime.runtime().make_enum_member_transport(
            &_selected_loaded,
            &runtime
                .runtime()
                .resolve_type_arguments(
                    &_selected_loaded,
                    &[binding::option(
                        _selected_loaded.definitions(),
                        Ty::Builtin(BuiltinType::I32),
                    )
                    .unwrap()],
                )
                .unwrap()[0],
            "Some",
            vec![Value::I32(42)],
        )
    }
    .unwrap();
    assert!(
        !unsafe { script_equal_transport(runtime.runtime().gc(), &Value::Enum(handle), &standard) }
            .unwrap()
    );

    for fields in [vec![], vec![Value::Bool(true)]] {
        assert_eq!(
            unsafe {
                runtime
                    .runtime()
                    .alloc_enum_transport(EnumTag::Declared(option.clone()), fields)
            }
            .unwrap_err()
            .kind(),
            RuntimeErrorKind::ScriptTrap
        );
    }
    let mut foreign_runtime = engine.runtime(Default::default());
    let foreign = foreign_runtime
        .load_program(
            &PreparedProgram::from_artifact(original, &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        unsafe {
            runtime.runtime().alloc_enum_transport(
                EnumTag::Declared(variant(
                    &foreign_runtime.runtime().select_program(&foreign).unwrap(),
                    "Option",
                )),
                vec![Value::I32(42)],
            )
        }
        .unwrap_err()
        .kind(),
        RuntimeErrorKind::ModuleValidation
    );

    let array = unsafe {
        runtime.runtime().alloc_array_transport(
            &_selected_loaded,
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(1)],
        )
    }
    .unwrap();
    let wrong = unsafe {
        runtime.runtime().alloc_enum_transport(
            EnumTag::Declared(variant(&_selected_loaded, "Other")),
            vec![Value::I32(42)],
        )
    }
    .unwrap();
    assert!(
        unsafe {
            runtime.runtime().alloc_enum_transport(
                EnumTag::Declared(holder.clone()),
                vec![Value::Enum(wrong), Value::Array(array)],
            )
        }
        .is_err()
    );
    assert!(
        unsafe {
            runtime.runtime().alloc_enum_transport(
                EnumTag::Declared(holder.clone()),
                vec![Value::Enum(handle), Value::Array(array)],
            )
        }
        .is_ok()
    );
    let old_key = _selected_loaded.key();
    let changed = compile(
        &engine,
        &source
            .replace("Some(i32)", "Some(String)")
            .replace("Some(42)", "Some(\"new\")"),
    );
    let changed =
        PreparedProgram::from_artifact(changed, &Default::default(), &Default::default()).unwrap();
    assert!(
        runtime
            .reload_program(&loaded, &changed, Default::default())
            .is_err()
    );
    assert_eq!(
        runtime.runtime().select_program(&loaded).unwrap().key(),
        old_key
    );
    // Frozen payload layouts reject publication. An independent installation with
    // the changed schema still cannot consume the retained old values.
    let new = runtime.load_program(&changed, Default::default()).unwrap();
    assert!(
        unsafe {
            runtime.runtime().alloc_enum_transport(
                EnumTag::Declared(variant(
                    &runtime.runtime().select_program(&new).unwrap(),
                    "Holder",
                )),
                vec![Value::Enum(handle), Value::Array(array)],
            )
        }
        .is_err()
    );
    drop(holder);
    drop(option);
    drop(loaded);
    runtime.runtime().collect_garbage().unwrap();
    let snapshot = unsafe { runtime.runtime().gc().enum_snapshot_transport(handle) }.unwrap();
    let EnumTag::Declared(retained) = snapshot.tag;
    assert_eq!(retained.module().key(), old_key);
    assert_eq!(snapshot.fields, [Value::I32(42)]);
    assert!(
        unsafe {
            runtime
                .runtime()
                .alloc_enum_transport(EnumTag::Declared(retained.clone()), vec![Value::I32(7)])
        }
        .is_ok()
    );
    drop(root);
    runtime.runtime().collect_garbage().unwrap();
    assert!(unsafe { runtime.runtime().gc().enum_snapshot_transport(handle) }.is_none());
    assert!(
        unsafe {
            runtime.runtime().alloc_enum_transport(
                EnumTag::Declared(retained.clone()),
                vec![Value::Enum(handle)],
            )
        }
        .is_err()
    );
}

#[test]
fn imported_enum_constructors_use_the_pinned_dependency_layouts() {
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
        "use pkg::left; use pkg::right; fn main() -> bool { (match left::make() { left::Event::Data(n) => n == 7 }) && (match right::make() { right::Event::Data(n) => n == 9 }) }",
    );
    let decoded =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &Default::default())
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| view.as_bool().map(Value::Bool))
            .expect("retained execution result"),
        Value::Bool(true)
    );
}

#[test]
fn payload_abi_roundtrips_and_rejects_changed_reload_before_publication() {
    let engine = KagariEngine::default();
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
            &PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default())
                .unwrap(),
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
    let before = runtime.runtime().modules().loaded_count();
    let before_epoch = runtime.runtime().select_program(&loaded).unwrap().epoch;
    let error = runtime
        .reload_program(
            &loaded,
            &PreparedProgram::from_artifact(changed, &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap_err();
    assert_eq!(error.code(), "KG_RELOAD_PUBLIC_ABI_FINGERPRINT_MISMATCH");
    assert_eq!(runtime.runtime().modules().loaded_count(), before);
    assert_eq!(
        runtime.runtime().select_program(&loaded).unwrap().epoch,
        before_epoch
    );
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
            .reload_program(
                &loaded,
                &PreparedProgram::from_artifact(
                    body_edit,
                    &Default::default(),
                    &Default::default()
                )
                .unwrap(),
                Default::default()
            )
            .is_ok()
    );
    let (definitions, mut old_format) = original.into_parts();
    old_format.header.format_version = 11;
    assert!(BytecodeArtifact::from_bytes(&old_format.to_bytes(&definitions).unwrap()).is_err());
}

#[test]
fn same_spelled_payload_types_from_different_modules_have_different_abi() {
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
        a.records().verification.public_abi_fingerprints,
        b.records().verification.public_abi_fingerprints
    );
}
