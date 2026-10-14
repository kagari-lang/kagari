//! Nominal DTO schemas share ordinary declarations and transactional installation.
use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{analysis::AnalysisDatabase, native::render::declaration_source};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::KagariType,
        module::NativeModule,
        schema::{TypeRegistration, ValueField, ValueVariant},
        types::Type,
    },
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::ty::{NominalTy, Ty};
use std::{collections::BTreeMap, sync::Arc};

struct Leaf;

struct Tree;

struct Choice;

impl KagariType for Leaf {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(cx: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        cx.structure::<Self>("Leaf", |_| Ok(vec![ValueField::new("value", Type::i32())]))
    }
}

impl KagariType for Tree {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(cx: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        cx.structure::<Self>("Tree", |cx| {
            Ok(vec![
                ValueField::new("leaf", cx.dependency::<Leaf>()?),
                ValueField::new("children", cx.dependency::<Vec<Tree>>()?),
            ])
        })
    }
}

impl KagariType for Choice {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(cx: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        cx.enumeration::<Self>("Choice", |cx| {
            Ok(vec![
                ValueVariant::new("Empty", []),
                ValueVariant::new("Leaf", [cx.dependency::<Leaf>()?]),
            ])
        })
    }
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

struct Missing;

impl KagariType for Missing {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(cx: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        let definitions = DefinitionContext::new().unwrap();
        let root = definitions
            .intern_root(&ModuleIdentity::single_file("absent"))
            .unwrap();
        let id = definitions
            .intern_child(root, DefinitionKind::Struct, "Unknown", 0)
            .unwrap();
        let ty = Type::from_semantic(
            Ty::Struct(NominalTy {
                declaration: id,
                arguments: vec![],
                associated_types: Default::default(),
            }),
            &definitions.snapshot(),
        )?;
        cx.structure::<Self>("Missing", |_| Ok(vec![ValueField::new("bad", ty)]))
    }
}

#[test]
fn missing_schema_dependency_rejects_without_publishing_headers_or_mappings() {
    let mut builder = ModuleBuilder::new("game::atomic", &DeclarationCatalog::default()).unwrap();
    assert!(builder.register_type::<Missing>().is_err());
    assert!(builder.declarations().rust_type::<Missing>().is_err());
    builder.register_type::<Leaf>().unwrap();
    let module = builder.finish().unwrap();
    assert_eq!(module.declaration().types.len(), 1);
    assert_eq!(module.declaration().types[0].name, "Leaf");
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
        .set(
            "schema.kgr",
            "use game::data::Tree; pub fn inspect(tree: Tree) -> i32 { tree.leaf.value }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(file, &Default::default()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let bytecode =
        lower_program_to_bytecode(&lower_program_to_mir(&checked, &Default::default()).unwrap())
            .unwrap()
            .import_into(&context, &Default::default())
            .unwrap()
            .into_records();
    let artifact = KbcArtifact::from_program(bytecode, Default::default(), &context).unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records()
        .into_verified(&Default::default(), &context)
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

struct FailedChild;

impl KagariType for FailedChild {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(cx: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        cx.structure::<Self>("FailedChild", |cx| {
            cx.dependency::<Missing>()?;
            Err(RuntimeError::metadata_conflict("field schema failed"))
        })
    }
}

struct CatchesChild;

impl KagariType for CatchesChild {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(cx: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        cx.structure::<Self>("CatchesChild", |cx| {
            let _ = cx.dependency::<FailedChild>();
            Ok(vec![])
        })
    }
}

#[test]
fn caught_child_schema_failure_cannot_publish_an_incomplete_dependency() {
    let mut builder = ModuleBuilder::new("game::caught", &DeclarationCatalog::default()).unwrap();
    assert!(
        builder
            .register_type::<CatchesChild>()
            .unwrap_err()
            .message()
            .contains("incomplete schema")
    );
    assert!(builder.declarations().rust_type::<CatchesChild>().is_err());
    assert!(builder.declarations().rust_type::<FailedChild>().is_err());
    builder.register_type::<Leaf>().unwrap();
    builder.finish().unwrap();
}
