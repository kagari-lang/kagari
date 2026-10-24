#![cfg(feature = "serde")]
//! The state bridge operates on checked retained values, never source analysis.
use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{map::DefinitionContext, metadata::DefinitionMetadata},
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_derive::KagariValue;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    frame::types::arguments::TypeArgument,
    gc::GcHeapConfig,
    module::ExecutableModule,
    native::{
        builder::ModuleBuilder,
        conversion::{FromKagari, IntoKagari, context::ConversionContext},
        module::NativeModule,
        payload::Opaque,
        value_handle::ScriptValue,
    },
    state::{
        self,
        data::{SchemaVersion, SnapshotHeader, StateSnapshot, StateValue},
        error::SnapshotErrorKind,
        limits::SnapshotOptions,
    },
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, KagariValue)]
struct Node {
    label: String,
    children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, KagariValue)]
enum Choice {
    Empty,
    Data(Node, Option<(i16, u64)>),
}

#[derive(Clone, Debug, KagariValue)]
struct Numbers {
    unit: (),
    boolean: bool,
    character: char,
    signed: (i8, i16, i32, i64),
    unsigned: (u8, u16, u32, u64),
    floats: (f32, f64),
    text: String,
    choice: Choice,
    outcome: Result<Vec<()>, String>,
}

fn modules() -> Vec<NativeModule> {
    let mut modules = kagari_stdlib::modules().unwrap();
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("game::state", &catalog).unwrap();
    builder.register_type::<Numbers>().unwrap();
    modules.push(builder.finish().unwrap());
    modules
}

fn bytecode(modules: &[NativeModule], text: &str) -> DefinitionMetadata<BytecodeProgram> {
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        modules
            .iter()
            .map(|module| Arc::new(module.to_declaration()))
            .collect(),
    );
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("state.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(file, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default())
        .unwrap()
        .into_unverified();
    let context = DefinitionContext::new().unwrap();
    let bytecode = bytecode
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let artifact =
        KbcArtifact::from_program(bytecode, Default::default(), &context, &Default::default())
            .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records()
        .into_verified(&Default::default(), &context, &Default::default())
        .unwrap()
        .into_bytecode()
        .into_unverified()
}

fn load(
    modules: &[NativeModule],
    code: DefinitionMetadata<BytecodeProgram>,
) -> (Runtime, ExecutableModule) {
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        ..Default::default()
    });
    NativeModule::install_all(modules, &mut runtime).unwrap();
    let program = runtime.load_program("state", code).unwrap();
    let owner = runtime.select_program(&program).unwrap();
    (runtime, owner)
}

fn fixture() -> (Runtime, ExecutableModule) {
    let modules = modules();
    let code = bytecode(
        &modules,
        "use game::state::Numbers; pub fn inspect(value: Numbers) -> String { value.text }",
    );
    load(&modules, code)
}

fn header() -> SnapshotHeader {
    SnapshotHeader {
        schema: SchemaVersion::new("game-state", 1).unwrap(),
        source_deployment: "old-deployment".into(),
        state_owner: "player-42".into(),
        state_revision: 17,
    }
}

fn retain<T: IntoKagari>(cx: &mut ConversionContext<'_>, value: T) -> ScriptValue {
    let argument = cx.type_for::<T>().unwrap();
    let root = cx.encode(value).unwrap();
    cx.decode_value(&argument, &root.value(cx.runtime().gc()).unwrap())
        .unwrap()
}

fn decode<T: FromKagari>(cx: &mut ConversionContext<'_>, value: &ScriptValue) -> T {
    let argument = cx.type_for::<T>().unwrap();
    let raw = value.clone().into_kagari(cx, &argument).unwrap();
    cx.decode_value(&argument, &raw).unwrap()
}

#[test]
fn source_free_independent_runtime_roundtrip_preserves_complete_data_and_fresh_roots() {
    let modules = modules();
    let code = bytecode(
        &modules,
        "use game::state::Numbers; pub fn inspect(value: Numbers) -> String { value.text }",
    );
    let (old, old_owner) = load(&modules, code.clone());
    let (new, new_owner) = load(&modules, code);
    let mut old_cx = ConversionContext::new(&old, &old_owner).unwrap();
    let mut new_cx = ConversionContext::new(&new, &new_owner).unwrap();
    let equal = Node {
        label: "same".into(),
        children: vec![],
    };
    let source = retain(
        &mut old_cx,
        Numbers {
            unit: (),
            boolean: true,
            character: '🙂',
            signed: (i8::MIN, i16::MIN, i32::MIN, i64::MIN),
            unsigned: (u8::MAX, u16::MAX, u32::MAX, u64::MAX),
            floats: (
                f32::from_bits(0x7fc0_1234),
                f64::from_bits(0xfff8_1234_5678_9abc),
            ),
            text: "portable".into(),
            choice: Choice::Data(
                Node {
                    label: "root".into(),
                    children: vec![equal.clone(), equal],
                },
                Some((-12, u64::MAX)),
            ),
            outcome: Ok(vec![(), (), ()]),
        },
    );
    let options = SnapshotOptions::default();
    let snapshot = state::export(&mut old_cx, &source, header(), &options).unwrap();
    let bytes = snapshot.to_bytes(&options).unwrap();
    let snapshot = std::thread::spawn(move || {
        StateSnapshot::from_bytes(&bytes, &SnapshotOptions::default()).unwrap()
    })
    .join()
    .unwrap();
    let expected = new_cx.type_for::<Numbers>().unwrap();
    assert!(state::export(&mut new_cx, &source, header(), &options).is_err());
    assert!(
        state::describe(
            &mut new_cx,
            source.type_argument(),
            header().schema,
            &options
        )
        .is_err()
    );
    let restored = state::restore(
        &mut new_cx,
        &expected,
        &snapshot,
        &header().schema,
        &options,
    )
    .unwrap();
    let second = state::restore(
        &mut new_cx,
        &expected,
        &snapshot,
        &header().schema,
        &options,
    )
    .unwrap();
    let first_raw = restored
        .clone()
        .into_kagari(&mut new_cx, &expected)
        .unwrap();
    let second_raw = second.clone().into_kagari(&mut new_cx, &expected).unwrap();
    let (Value::Struct(first_id), Value::Struct(second_id)) = (first_raw, second_raw) else {
        panic!("Numbers roots must be structs");
    };
    assert_ne!(first_id, second_id);
    drop(new_cx);
    new.collect_garbage().unwrap();
    let mut new_cx = ConversionContext::new(&new, &new_owner).unwrap();
    let decoded: Numbers = decode(&mut new_cx, &restored);
    assert_eq!(decoded.signed, (i8::MIN, i16::MIN, i32::MIN, i64::MIN));
    assert_eq!(decoded.unsigned, (u8::MAX, u16::MAX, u32::MAX, u64::MAX));
    assert_eq!(decoded.floats.0.to_bits(), 0x7fc0_1234);
    assert_eq!(decoded.floats.1.to_bits(), 0xfff8_1234_5678_9abc);
    assert_eq!(decoded.character, '🙂');
    assert_eq!(decoded.outcome, Ok(vec![(), (), ()]));
    assert_eq!(
        state::export(&mut new_cx, &restored, header(), &options).unwrap(),
        snapshot
    );
    let original: Numbers = decode(&mut old_cx, &source);
    assert_eq!(original.text, "portable");
    drop(restored);
    drop(second);
    drop(new_cx);
    assert_eq!(new.gc().active_roots(), 0);
    assert_eq!(new.collect_garbage().unwrap().live_objects, 0);
}

fn alias<T: IntoKagari + Clone>(cx: &mut ConversionContext<'_>, value: T) {
    let item = cx.encode(value).unwrap();
    let raw = item.value(cx.runtime().gc()).unwrap();
    let expected = cx.type_for::<(T, T)>().unwrap();
    let tuple = cx.runtime().gc().alloc_tuple(vec![raw, raw]).unwrap();
    let tuple = cx.runtime().root_value(tuple).unwrap();
    let retained: ScriptValue = cx
        .decode_value(&expected, &tuple.value(cx.runtime().gc()).unwrap())
        .unwrap();
    let roots = cx.runtime().gc().active_roots();
    let error = state::export(cx, &retained, header(), &SnapshotOptions::default()).unwrap_err();
    let SnapshotErrorKind::Alias { first_path } = error.kind else {
        panic!("{error}");
    };
    assert_eq!(first_path.to_string(), "$[0]");
    assert_eq!(error.path.to_string(), "$[1]");
    assert_eq!(cx.runtime().gc().active_roots(), roots);
}

#[test]
fn strict_identity_covers_struct_enum_tuple_and_vec_but_not_strings() {
    let (runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    alias(
        &mut cx,
        Node {
            label: "x".into(),
            children: vec![],
        },
    );
    alias(&mut cx, Choice::Empty);
    alias(&mut cx, (1i32, 2i32));
    alias(&mut cx, vec![1i32, 2]);
    let text = cx.encode("repeat".to_owned()).unwrap();
    let raw = text.value(runtime.gc()).unwrap();
    let expected = cx.type_for::<(String, String)>().unwrap();
    let raw = runtime.gc().alloc_tuple(vec![raw, raw]).unwrap();
    let value = ScriptValue::from_kagari(&mut cx, &expected, &raw).unwrap();
    let snapshot = state::export(&mut cx, &value, header(), &SnapshotOptions::default()).unwrap();
    assert_eq!(
        snapshot.payload(),
        &StateValue::Tuple(vec![
            StateValue::String("repeat".into()),
            StateValue::String("repeat".into())
        ])
    );
    drop(text);
    drop(value);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn cycles_report_first_and_current_paths_and_release_all_snapshot_roots() {
    let (runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let expected = cx.type_for::<Node>().unwrap();
    let node = retain(
        &mut cx,
        Node {
            label: "root".into(),
            children: vec![],
        },
    );
    let raw = node.clone().into_kagari(&mut cx, &expected).unwrap();
    let Value::Struct(id) = raw else {
        panic!("Node");
    };
    let layout = runtime.gc().struct_layout(id).unwrap();
    let Value::GcHandle(children) = runtime.gc().struct_get_slot(id, &layout, 1).unwrap() else {
        panic!("children");
    };
    runtime.gc().sequence_push(children, raw).unwrap();
    let roots = runtime.gc().active_roots();
    let error = state::export(&mut cx, &node, header(), &SnapshotOptions::default()).unwrap_err();
    let SnapshotErrorKind::Cycle { first_path } = error.kind else {
        panic!("{error}");
    };
    assert_eq!(first_path.to_string(), "$");
    assert_eq!(error.path.to_string(), "$.children[0]");
    assert_eq!(runtime.gc().active_roots(), roots);
    drop(node);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

fn parameter(runtime: &Runtime, owner: &ExecutableModule, name: &str) -> TypeArgument {
    let function = owner
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap();
    runtime
        .resolve_type_arguments(owner, &[function.metadata.semantic.params[&0].clone()])
        .unwrap()
        .remove(0)
}

#[test]
fn work_exhaustion_during_fresh_construction_releases_partial_roots() {
    let (runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let source = retain(&mut cx, vec!["payload".repeat(16); 24]);
    let expected = cx.type_for::<Vec<String>>().unwrap();
    let snapshot = state::export(&mut cx, &source, header(), &SnapshotOptions::default()).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline_roots = runtime.gc().active_roots();
    let baseline_objects = runtime.gc().allocated_objects();
    let mut options = SnapshotOptions::default();
    let (mut rejected, mut admitted) = (0, options.limits.max_work);
    let mut failed_after_allocation = false;
    // Locate this operation's work boundary without pinning a private cost or
    // assuming a particular number of schema passes. Every failed prefix must
    // preserve the old tree and release all fresh child roots.
    while admitted - rejected > 1 {
        options.limits.max_work = rejected + (admitted - rejected) / 2;
        match state::restore(&mut cx, &expected, &snapshot, &header().schema, &options) {
            Ok(restored) => {
                admitted = options.limits.max_work;
                drop(restored);
            }
            Err(error) => {
                assert_eq!(
                    error.kind,
                    SnapshotErrorKind::LimitExceeded { limit: "work" }
                );
                failed_after_allocation |= runtime.gc().allocated_objects() > baseline_objects;
                rejected = options.limits.max_work;
            }
        }
        assert_eq!(runtime.gc().active_roots(), baseline_roots);
        assert_eq!(
            runtime.collect_garbage().unwrap().live_objects,
            baseline_objects
        );
    }
    assert!(
        failed_after_allocation,
        "exercise abandonment after constructing fresh children"
    );
    assert_eq!(
        decode::<Vec<String>>(&mut cx, &source),
        vec!["payload".repeat(16); 24]
    );
}

#[test]
fn session_cancellation_and_retired_runtime_scopes_reject_without_leaking_roots() {
    let (runtime, owner) = fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let root = retain(&mut cx, vec![1i32, 2, 3]);
    let snapshot = state::export(&mut cx, &root, header(), &SnapshotOptions::default()).unwrap();
    let expected = cx.type_for::<Vec<i32>>().unwrap();
    let baseline = runtime.gc().active_roots();
    let options = runtime.execution_options();
    let cancellation = options.cancellation.clone();
    let session = runtime.begin_execution(&owner, options).unwrap();
    cancellation.cancel();
    let error = state::restore(
        &mut cx,
        &expected,
        &snapshot,
        &header().schema,
        &SnapshotOptions::default(),
    )
    .unwrap_err();
    assert_eq!(
        error.runtime_error.unwrap().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(runtime.gc().active_roots(), baseline);
    drop(session);
    assert!(state::export(&mut cx, &root, header(), &SnapshotOptions::default()).is_ok());
    runtime.close().unwrap();
    assert!(state::export(&mut cx, &root, header(), &SnapshotOptions::default()).is_err());
    assert!(
        state::restore(
            &mut cx,
            &expected,
            &snapshot,
            &header().schema,
            &SnapshotOptions::default()
        )
        .is_err()
    );
    assert_eq!(runtime.gc().active_roots(), baseline);
}

kagari_runtime::native_data! { struct Resource { value: i32 } }

#[test]
fn complete_schema_rejects_private_fields_and_forbidden_empty_branches() {
    let mut modules = kagari_stdlib::modules().unwrap();
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("game::resources", &catalog).unwrap();
    builder.register_type::<Opaque<Resource>>().unwrap();
    modules.push(builder.finish().unwrap());
    let source = r#"
        use game::resources::Resource;
        pub struct Private { val secret: i32 }
        pub enum Hidden { Empty, Callback(fn() -> i32) }
        pub fn private(value: Private) -> i32 { value.secret }
        pub fn empty(value: Vec<fn() -> i32>) -> () { () }
        pub fn inactive(value: Hidden) -> () { () }
        pub fn opaque(value: Option<Resource>) -> () { () }
    "#;
    let (runtime, owner) = load(&modules, bytecode(&modules, source));
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let none = retain(&mut cx, None::<Opaque<Resource>>);
    let empty = retain(&mut cx, Vec::<Opaque<Resource>>::new());
    for root in [&none, &empty] {
        let error =
            state::export(&mut cx, root, header(), &SnapshotOptions::default()).unwrap_err();
        assert_eq!(error.kind, SnapshotErrorKind::UnsupportedType);
    }
    for name in ["private", "empty", "inactive", "opaque"] {
        let argument = parameter(&runtime, &owner, name);
        let roots = runtime.gc().active_roots();
        let error = state::describe(
            &mut cx,
            &argument,
            header().schema,
            &SnapshotOptions::default(),
        )
        .unwrap_err();
        assert!(
            matches!(
                error.kind,
                SnapshotErrorKind::UnsupportedType | SnapshotErrorKind::Runtime
            ),
            "{name}: {error}"
        );
        assert_eq!(runtime.gc().active_roots(), roots);
    }
}

#[test]
fn restore_checks_complete_schema_before_allocating_and_limits_leave_old_root_usable() {
    let modules = kagari_stdlib::modules().unwrap();
    let old_source =
        "pub struct Data { pub val value: i32 } pub fn inspect(value: Data) -> i32 { value.value }";
    let new_source =
        "pub struct Data { pub val value: i64 } pub fn inspect(value: Data) -> i64 { value.value }";
    let (old, old_owner) = load(&modules, bytecode(&modules, old_source));
    let (new, new_owner) = load(&modules, bytecode(&modules, new_source));
    let mut old_cx = ConversionContext::new(&old, &old_owner).unwrap();
    let argument = parameter(&old, &old_owner, "inspect");
    let mut builder = old_cx.write_struct(&argument).unwrap();
    builder.field("value", 7i32).unwrap();
    let value = builder.finish().unwrap();
    let root = ScriptValue::from_kagari(&mut old_cx, &argument, &value).unwrap();
    let snapshot =
        state::export(&mut old_cx, &root, header(), &SnapshotOptions::default()).unwrap();
    let expected = parameter(&new, &new_owner, "inspect");
    let mut new_cx = ConversionContext::new(&new, &new_owner).unwrap();
    let roots = new.gc().active_roots();
    let error = state::restore(
        &mut new_cx,
        &expected,
        &snapshot,
        &header().schema,
        &SnapshotOptions::default(),
    )
    .unwrap_err();
    assert_eq!(error.kind, SnapshotErrorKind::SchemaMismatch);
    assert_eq!(new.gc().active_roots(), roots);
    assert_eq!(new.collect_garbage().unwrap().live_objects, 0);
    let mut options = SnapshotOptions::default();
    options.limits.max_nodes = 1;
    assert!(state::export(&mut old_cx, &root, header(), &options).is_err());
    assert!(state::export(&mut old_cx, &root, header(), &SnapshotOptions::default()).is_ok());
    options = SnapshotOptions::default();
    options.cancellation = CancellationToken::default();
    options.cancellation.cancel();
    let error = state::restore(
        &mut old_cx,
        &argument,
        &snapshot,
        &header().schema,
        &options,
    )
    .unwrap_err();
    assert_eq!(error.kind, SnapshotErrorKind::Cancelled);
}
