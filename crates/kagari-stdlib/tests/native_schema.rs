//! Nominal DTO schemas share ordinary declarations and transactional installation.
// Shared preparation keeps verifier and linker failures observable.
#[path = "../../kagari-runtime/tests/support/bytecode.rs"]
mod bytecode_fixture;
use crate::bytecode_fixture::load_bytecode;
mod native_schema_derive;
mod native_schema_serde;
use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_derive::KagariValue;
use kagari_hir::compile_input::program::CheckedCompilation;
use kagari_hir::{analysis::AnalysisDatabase, native::render::declaration_source};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::context::ErrorLocation,
    execution::memory::GcHeapConfig,
    frame::types::arguments::TypeArgument,
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
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::ty::Ty;
#[path = "../../kagari-runtime/tests/support/schema_leaf.rs"]
mod leaf;
use leaf::Leaf;

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq, KagariValue)]
struct Tree {
    leaf: Leaf,
    children: Option<Box<Tree>>,
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
    program_for_source(module, include_str!("fixtures/native/schema_tree.kgr"))
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
        &lower_program_to_mir(
            &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
            &Default::default(),
        )
        .unwrap(),
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
    load_bytecode(&mut runtime, "valid", program.clone()).unwrap();
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
    let failure = load_bytecode(&mut runtime, "corrupt", self::program(&altered)).unwrap_err();
    assert!(
        failure.to_string().contains("registered value schema"),
        "{failure}"
    );
}

impl IntoKagari for Leaf {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        let mut fields = cx.write_struct(expected)?;
        fields.field("value", self.value)?;
        fields.finish()
    }
}

thread_local! {
    static REWRITE_PARENT: RefCell<Option<RootedValue>> = const { RefCell::new(None) };
    static PANIC_IN_LEAF: Cell<bool> = const { Cell::new(false) };
}

impl FromKagari for Leaf {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        if let Some(parent) = REWRITE_PARENT.with(|slot| slot.borrow_mut().take()) {
            let empty = cx.encode(None::<Box<Tree>>)?;
            let _ = cx
                .runtime_mut()
                .reflect_set_field(&parent, "children", &empty)?;
            cx.runtime_mut().collect_garbage()?;
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
    let owner_program = load_bytecode(&mut runtime, "dto", program).unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();
    runtime.collect_garbage().unwrap();
    (runtime, owner)
}

#[test]
fn char_host_conversions_preserve_domains_in_scalars_and_generic_containers() {
    let (mut runtime, owner) = conversion_fixture();
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
    for ch in ['\0', 'é', '🙂', char::MAX] {
        let root = cx.encode(ch).unwrap();
        assert_eq!(root.as_char(), Some(ch));
        assert_eq!(cx.decode::<char>(&root).unwrap(), ch);
        assert!(cx.decode::<u32>(&root).is_err());
        let integer = cx.encode(u32::from(ch)).unwrap();
        assert!(cx.decode::<char>(&integer).is_err());
    }
    let expected = (('\0', 'é', '🙂'), Some('a'), Ok::<char, char>(char::MAX));
    let root = cx.encode(expected).unwrap();
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(
        cx.decode::<((char, char, char), Option<char>, Result<char, char>)>(&root)
            .unwrap(),
        expected
    );
    assert!(
        cx.decode::<((u32, u32, u32), Option<char>, Result<char, char>)>(&root)
            .is_err()
    );
}

#[test]
fn nested_dtos_and_enum_payloads_round_trip_with_owned_isolation_and_collection() {
    let (mut runtime, owner) = conversion_fixture();
    let baseline = runtime.gc_stats();
    let input = Tree {
        leaf: Leaf { value: 42 },
        children: Some(Box::new(Tree {
            leaf: Leaf { value: 7 },
            children: None,
        })),
    };
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
    let root = cx.encode(input.clone()).unwrap();
    let mut decoded: Tree = cx.decode(&root).unwrap();
    decoded.leaf.value = 99;
    assert_eq!(cx.decode::<Tree>(&root).unwrap(), input);
    let leaf = cx.runtime_mut().reflect_get_field(&root, "leaf").unwrap();
    let _ = cx
        .runtime_mut()
        .reflect_set_field(&leaf, "value", &RootedValue::from(12i32))
        .unwrap();
    drop(leaf);
    assert_eq!(decoded.leaf.value, 99);
    assert_eq!(cx.decode::<Tree>(&root).unwrap().leaf.value, 12);
    for input in [Choice::Empty, Choice::Leaf(Leaf { value: 23 })] {
        let root = cx.encode(input.clone()).unwrap();
        cx.runtime_mut().collect_garbage().unwrap();
        assert_eq!(cx.decode::<Choice>(&root).unwrap(), input);
    }
    assert!(cx.runtime().gc_stats().completed_cycles > 0);
    drop(root);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
}

#[test]
fn dto_limits_and_incomplete_initializers_release_unpublished_roots() {
    let (mut runtime, owner) = conversion_fixture();
    let baseline = runtime.gc_stats();
    let input = Tree {
        leaf: Leaf { value: 42 },
        children: Some(Box::new(Tree {
            leaf: Leaf { value: 7 },
            children: None,
        })),
    };
    let mut cx = ConversionContext::with_limits(
        &mut runtime,
        &owner,
        ConversionLimits {
            max_depth: 3,
            ..Default::default()
        },
    )
    .unwrap();
    let error = cx.encode(input.clone()).unwrap_err();
    assert!(error.to_string().contains("field leaf"), "{error}");
    assert!(
        error
            .locations()
            .iter()
            .any(|position| matches!(position, ErrorLocation::Field(name) if name == "leaf"))
    );
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
    drop(cx);
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
    let expected = cx.type_for::<Tree>().unwrap();
    let mut fields = cx.write_struct(&expected).unwrap();
    fields.field("leaf", Leaf { value: 1 }).unwrap();
    assert!(fields.finish().unwrap_err().to_string().contains("missing"));
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
    let mut fields = cx.write_struct(&expected).unwrap();
    fields.field("leaf", Leaf { value: 1 }).unwrap();
    assert!(
        fields
            .field("leaf", Leaf { value: 2 })
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
    drop(fields);
    let root = cx.encode(input).unwrap();
    let mut restricted = ConversionContext::with_limits(
        cx.runtime_mut(),
        &owner,
        ConversionLimits {
            max_depth: 3,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(restricted.decode::<Tree>(&root).is_err());
    restricted.runtime_mut().collect_garbage().unwrap();
    assert_eq!(
        restricted.runtime().gc_stats().root_registrations,
        baseline.root_registrations + 1
    );
    drop(root);
    restricted.runtime_mut().collect_garbage().unwrap();
    assert_eq!(restricted.runtime().gc_stats().objects, baseline.objects);
    restricted.runtime_mut().collect_garbage().unwrap();
    assert_eq!(
        restricted.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn dto_snapshot_survives_alias_mutation_collection_and_child_unwind() {
    let (mut runtime, owner) = conversion_fixture();
    let baseline = runtime.gc_stats();
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
    let input = Tree {
        leaf: Leaf { value: 1 },
        children: Some(Box::new(Tree {
            leaf: Leaf { value: 2 },
            children: None,
        })),
    };
    let root = cx.encode(input.clone()).unwrap();
    REWRITE_PARENT.with(|slot| *slot.borrow_mut() = Some(root.clone()));
    assert_eq!(cx.decode::<Tree>(&root).unwrap(), input);
    assert!(cx.decode::<Tree>(&root).unwrap().children.is_none());
    PANIC_IN_LEAF.set(true);
    assert!(catch_unwind(AssertUnwindSafe(|| cx.decode::<Tree>(&root))).is_err());
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations + 1
    );
    assert!(cx.decode::<Tree>(&root).unwrap().children.is_none());
    drop(root);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
}
