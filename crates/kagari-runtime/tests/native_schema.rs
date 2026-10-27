//! Nominal DTO schemas share ordinary declarations and transactional installation.
mod native_schema_derive;
#[cfg(feature = "serde")]
mod native_schema_serde;
use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_derive::KagariValue;
use kagari_hir::{analysis::AnalysisDatabase, native::render::declaration_source};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    frame::types::arguments::TypeArgument,
    gc::{GcHeapConfig, HeapObjectId},
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{
            FromKagari, IntoKagari, KagariType,
            context::{ConversionContext, ConversionLimits},
        },
        module::NativeModule,
        schema::{TypeRegistration, ValueField},
        types::Type,
    },
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::collections::vector::VecValues;
use kagari_types::ty::Ty;
#[path = "support/schema_leaf.rs"]
mod leaf;
use leaf::Leaf;

use std::{
    cell::Cell,
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq, KagariValue)]
struct Tree {
    leaf: Leaf,
    children: VecValues<Tree>,
}

#[derive(Debug, Clone, PartialEq, KagariValue)]
enum Choice {
    Empty,
    Leaf(Leaf),
}

fn module(name: &str) -> NativeModule {
    let providers = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new(name, &providers).unwrap();
    builder.register_type::<Tree>().unwrap();
    builder.register_type::<Choice>().unwrap();
    assert!(builder.register_type::<Leaf>().is_err());
    builder.finish().unwrap()
}

#[test]
fn offline_schemas_reserve_recursive_headers_and_render_ordinary_writable_fields() {
    let module = module("game::data");
    let metadata = Arc::new(module.to_declaration());
    let source = declaration_source(&metadata, std::slice::from_ref(&metadata)).unwrap();
    assert!(source.text.contains("pub struct Tree"));
    assert!(source.text.contains("pub var leaf: Leaf"));
    assert!(source.text.contains("pub enum Choice"));
    assert_eq!(module.declaration().types.len(), 3);
    assert!(module.declaration().functions.is_empty());
    assert!(module.declaration().implementations.is_empty());
    let catalog = module.catalog();
    assert!(Tree::kagari_type(&catalog).is_ok());
    assert!(Leaf::kagari_type(&catalog).is_ok());
}

#[test]
fn catalog_local_rust_identity_rejects_duplicate_module_install_atomically() {
    let first = module("game::first");
    let second = module("game::second");
    let first_type = Tree::kagari_type(&first.catalog()).unwrap();
    let second_type = Tree::kagari_type(&second.catalog()).unwrap();
    let owner = |ty: &Type| {
        let Ty::Struct(nominal) = ty.abi() else {
            panic!("struct schema")
        };
        ty.definitions()
            .unwrap()
            .resolve(nominal.declaration)
            .unwrap()
            .module()
            .clone()
    };
    assert_ne!(owner(&first_type), owner(&second_type));
    let mut runtime = Runtime::default();
    let providers = kagari_stdlib::modules().unwrap();
    NativeModule::install_all(&providers, &mut runtime).unwrap();
    assert!(NativeModule::install_all(&[first.clone(), second], &mut runtime).is_err());
    // The rejected batch did not install the first module either.
    first.install(&mut runtime).unwrap();
}

fn program(module: &NativeModule) -> DefinitionMetadata<BytecodeProgram> {
    program_for_source(
        module,
        "use game::data::Tree; pub fn inspect(tree: Tree) -> i32 { tree.leaf.value }",
    )
}

fn program_for_source(module: &NativeModule, text: &str) -> DefinitionMetadata<BytecodeProgram> {
    let mut modules = kagari_stdlib::modules().unwrap();
    modules.push(module.clone());
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        modules
            .iter()
            .map(|m| Arc::new(m.to_declaration()))
            .collect(),
    );
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("schema.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(file, &Default::default()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let bytecode = lower_program_to_bytecode(
        &lower_program_to_mir(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .map(|program| program.into_unverified())
    .unwrap()
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

#[test]
fn source_free_loading_checks_registered_schema_without_native_calls() {
    let module = module("game::data");
    let program = program(&module);
    let mut runtime = Runtime::default();
    let mut modules = kagari_stdlib::modules().unwrap();
    modules.push(module);
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    runtime.load_program("valid", program.clone()).unwrap();
    // Compile a separately valid provider with the same identity but a changed
    // field capability. This reaches installed-contract validation rather than
    // only proving that bytecode rejects an internally inconsistent layout.
    let metadata = modules.last().unwrap().to_declaration();
    let definitions = metadata.definitions().clone();
    let mut declaration = metadata.into_records();
    declaration
        .types
        .iter_mut()
        .find(|ty| ty.name == "Leaf")
        .unwrap()
        .fields[0]
        .mutable = false;
    let providers = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let altered = ModuleBuilder::from_declaration(
        DefinitionMetadata::checked(definitions, declaration, &Default::default()).unwrap(),
        &providers,
        BTreeMap::new(),
    )
    .unwrap()
    .finish()
    .unwrap();
    let failure = runtime
        .load_program("corrupt", self::program(&altered))
        .unwrap_err();
    assert!(
        failure.message().contains("registered value schema"),
        "{failure}"
    );
}

impl IntoKagari for Leaf {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        let mut fields = cx.write_struct(expected)?;
        fields.field("value", self.value)?;
        fields.finish()
    }
}

thread_local! {
    static REWRITE_PARENT: Cell<Option<HeapObjectId>> = const { Cell::new(None) };
    static PANIC_IN_LEAF: Cell<bool> = const { Cell::new(false) };
}

impl FromKagari for Leaf {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        if let Some(parent) = REWRITE_PARENT.take() {
            let empty = cx.encode(VecValues::<Tree>(vec![]))?;
            let layout = cx.runtime().gc().struct_layout(parent).unwrap();
            cx.runtime().gc().struct_set_slot(
                parent,
                &layout,
                1,
                empty.value(cx.runtime().gc()).unwrap(),
            )?;
            cx.runtime().collect_garbage()?;
        }
        assert!(!PANIC_IN_LEAF.replace(false), "DTO child conversion unwind");
        let mut fields = cx.read_struct(expected, value)?;
        Ok(Self {
            value: fields.field("value")?,
        })
    }
}

fn conversion_fixture() -> (Runtime, kagari_runtime::module::ExecutableModule) {
    let module = module("game::data");
    let program = program(&module);
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        ..Default::default()
    });
    let mut modules = kagari_stdlib::modules().unwrap();
    modules.push(module);
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let owner_program = runtime.load_program("dto", program).unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();
    (runtime, owner)
}

#[test]
fn char_host_conversions_preserve_domains_in_scalars_and_generic_containers() {
    let (runtime, owner) = conversion_fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    for ch in ['\0', 'é', '🙂', char::MAX] {
        let root = cx.encode(ch).unwrap();
        assert_eq!(root.value(runtime.gc()), Some(Value::Char(ch)));
        assert_eq!(cx.decode::<char>(&root).unwrap(), ch);
        assert!(cx.decode::<u32>(&root).is_err());
        let integer = cx.encode(u32::from(ch)).unwrap();
        assert!(cx.decode::<char>(&integer).is_err());
    }
    let expected = (
        VecValues(vec!['\0', 'é', '🙂']),
        Some('a'),
        Ok::<char, char>(char::MAX),
    );
    let root = cx.encode(expected.clone()).unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(
        cx.decode::<(VecValues<char>, Option<char>, Result<char, char>)>(&root)
            .unwrap(),
        expected
    );
    assert!(
        cx.decode::<(VecValues<u32>, Option<char>, Result<char, char>)>(&root)
            .is_err()
    );
}

#[test]
fn nested_dtos_and_enum_payloads_round_trip_with_owned_isolation_and_collection() {
    let (runtime, owner) = conversion_fixture();
    let input = Tree {
        leaf: Leaf { value: 42 },
        children: VecValues(vec![Tree {
            leaf: Leaf { value: 7 },
            children: VecValues(vec![]),
        }]),
    };
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let root = cx.encode(input.clone()).unwrap();
    let mut decoded: Tree = cx.decode(&root).unwrap();
    decoded.leaf.value = 99;
    assert_eq!(cx.decode::<Tree>(&root).unwrap(), input);
    let Value::Struct(id) = root.value(runtime.gc()).unwrap() else {
        panic!("DTO struct")
    };
    let layout = runtime.gc().struct_layout(id).unwrap();
    let Value::Struct(leaf) = runtime.gc().struct_get_slot(id, &layout, 0).unwrap() else {
        panic!("nested DTO")
    };
    let leaf_layout = runtime.gc().struct_layout(leaf).unwrap();
    runtime
        .gc()
        .struct_set_slot(leaf, &leaf_layout, 0, Value::I32(12))
        .unwrap();
    assert_eq!(decoded.leaf.value, 99);
    assert_eq!(cx.decode::<Tree>(&root).unwrap().leaf.value, 12);
    for input in [Choice::Empty, Choice::Leaf(Leaf { value: 23 })] {
        let root = cx.encode(input.clone()).unwrap();
        runtime.collect_garbage().unwrap();
        assert_eq!(cx.decode::<Choice>(&root).unwrap(), input);
    }
    assert!(runtime.gc().stats().collections > 0);
    drop(root);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn dto_limits_and_incomplete_initializers_release_unpublished_roots() {
    let (runtime, owner) = conversion_fixture();
    let input = Tree {
        leaf: Leaf { value: 42 },
        children: VecValues(vec![Tree {
            leaf: Leaf { value: 7 },
            children: VecValues(vec![]),
        }]),
    };
    let mut cx = ConversionContext::with_limits(
        &runtime,
        &owner,
        ConversionLimits {
            max_depth: 3,
            ..Default::default()
        },
    )
    .unwrap();
    let error = cx.encode(input.clone()).unwrap_err();
    assert!(error.message().contains("field leaf"), "{error}");
    assert_eq!(runtime.gc().active_roots(), 0);
    drop(cx);
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let expected = cx.type_for::<Tree>().unwrap();
    let mut fields = cx.write_struct(&expected).unwrap();
    fields.field("leaf", Leaf { value: 1 }).unwrap();
    assert!(fields.finish().unwrap_err().message().contains("missing"));
    assert_eq!(runtime.gc().active_roots(), 0);
    let mut fields = cx.write_struct(&expected).unwrap();
    fields.field("leaf", Leaf { value: 1 }).unwrap();
    assert!(
        fields
            .field("leaf", Leaf { value: 2 })
            .unwrap_err()
            .message()
            .contains("duplicate")
    );
    drop(fields);
    let root = cx.encode(input).unwrap();
    let mut restricted = ConversionContext::with_limits(
        &runtime,
        &owner,
        ConversionLimits {
            max_depth: 3,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(restricted.decode::<Tree>(&root).is_err());
    assert_eq!(runtime.gc().active_roots(), 1);
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn dto_snapshot_survives_alias_mutation_collection_and_child_unwind() {
    let (runtime, owner) = conversion_fixture();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let input = Tree {
        leaf: Leaf { value: 1 },
        children: VecValues(vec![Tree {
            leaf: Leaf { value: 2 },
            children: VecValues(vec![]),
        }]),
    };
    let root = cx.encode(input.clone()).unwrap();
    let Value::Struct(parent) = root.value(runtime.gc()).unwrap() else {
        panic!("Tree")
    };
    REWRITE_PARENT.set(Some(parent));
    assert_eq!(cx.decode::<Tree>(&root).unwrap(), input);
    assert!(cx.decode::<Tree>(&root).unwrap().children.is_empty());
    PANIC_IN_LEAF.set(true);
    assert!(catch_unwind(AssertUnwindSafe(|| cx.decode::<Tree>(&root))).is_err());
    assert_eq!(runtime.gc().active_roots(), 1);
    assert!(cx.decode::<Tree>(&root).unwrap().children.is_empty());
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}
