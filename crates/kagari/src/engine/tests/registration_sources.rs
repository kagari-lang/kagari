#![cfg(feature = "source")]
use crate::test_support::{declaration_cache::Cache, standard::standard_library};
use crate::{
    context::ExecutionContext,
    engine::{KagariEngine, builder::KagariEngineBuilder},
    error::EmbeddingError,
    program::Program,
};
use kagari_analyzer::{AnalyzerQueries, workspace::EditorWorkspace};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_hir::{
    analysis::{AnalysisDatabase, AnalysisSnapshot},
    declarations::DeclarationId,
    imports::environment::{ImportEnvironment, ModuleScope},
};

use kagari_runtime::{
    module::entry::ProgramEntry,
    native::{
        assembly::{ModuleDefinition, ModuleSetBuilder},
        binding::NativeResult,
        builder::ModuleBuilder,
        context::NativeCall,
        conversion::KagariType,
        declarations::{FunctionDecl, MethodDecl},
        module::NativeModule,
        storage::NativeStorage,
        typed::NativeContext,
        types::Type,
    },
};
use kagari_source::{
    identity::FileId,
    source::SourceFile,
    source_database::{CapturedSource, SourceDatabase, SourceLayer, SourceSnapshot},
};
use kagari_stdlib::namespaces;
use kagari_types::{
    declaration::{
        module::ModuleDecl,
        names::{ExportName, NameNamespace},
    },
    language::{self, Protocol},
    ty::Ty,
};
use rstest::rstest;
use std::{collections::BTreeMap, fs, path::PathBuf};

const DOC: &str = "# Registered API\n\nParagraph with [a link](https://example.com).\n\n- First item\n- Second item\n\n```kgr\nfn sample() -> i32 { 42 }\n```\n\n中文 😀";

// Fresh editor acquisition, not an update of a live workspace with user overlays.
fn editor(engine: &KagariEngine) -> (AnalysisDatabase, SourceSnapshot) {
    let project = engine.source_snapshot();
    let export = engine
        .native_declaration_sources(project.clone(), &Default::default())
        .unwrap();
    let mut documents = export.documents;
    documents.extend(project.files().map(|file| CapturedSource {
        key: file.name().to_owned(),
        display_name: file.name().to_owned(),
        module: file.module_identity().clone(),
        text: file.text().to_owned(),
    }));
    let mut database = AnalysisDatabase::default();
    database.set_import_environment(export.imports);
    let sources = SourceDatabase::default()
        .replace_captured(documents, project.packages().clone(), &Default::default())
        .unwrap();
    (database, sources)
}

fn editor_client(engine: &KagariEngine, name: &str, text: &str) -> (AnalysisSnapshot, FileId) {
    let cancel = Default::default();
    let mut sources = SourceDatabase::default();
    let file = sources.set(name, text.into(), SourceLayer::Base).unwrap();
    let export = engine
        .native_declaration_sources(sources.snapshot(), &cancel)
        .unwrap();
    let mut workspace = EditorWorkspace::new(sources).unwrap();
    workspace
        .replace_generated(export.documents, export.imports, &cancel)
        .unwrap();
    (workspace.analyze(&cancel).unwrap(), file)
}

#[rstest]
#[case::memory(false)]
#[case::published(true)]
fn typed_registration_keeps_compiler_calls_and_editor_docs_across_cache_modes(
    #[case] publish: bool,
) {
    let cache = publish.then(Cache::new);
    let mut builder = KagariEngine::builder().unwrap();
    for owner in ["application::typed", "application::twin"] {
        let mut module = ModuleBuilder::new(owner, builder.declarations()).unwrap();
        module.documentation("# Text tools\n\nOwned Unicode text conversions.");
        let registered_function = module
            .define_function(
                FunctionDecl::new("duplicate")
                    .documentation(
                        DOC.to_owned()
                            + "\n\n# Parameters\n"
                            + "\n## `text`\n\n"
                            + "Input text.\n\nUnicode is preserved."
                            + "\n"
                            + "\n\n# Returns\n\n"
                            + "Two independent strings.\n\n```kgr\nduplicate(\"雪\")\n```",
                    )
                    .parameter(
                        "text",
                        <String>::kagari_type(module.declarations()).unwrap(),
                    )
                    .returns(<(String, String)>::kagari_type(module.declarations()).unwrap()),
            )
            .unwrap();
        module
            .bind(
                registered_function,
                |_: &mut NativeContext<'_>, (text,): (String,)| -> NativeResult<(String, String)> {
                    Ok((text.clone(), text))
                },
            )
            .unwrap();
        builder.install(module.finish().unwrap()).unwrap();
    }
    if let Some(cache) = &cache {
        builder.declaration_cache(&cache.0);
    }
    let engine = builder.build().unwrap();
    let text = "use alloc::string::String; use application::{typed, twin}; fn main() -> (String, String) { twin::duplicate(\"雪\"); typed::duplicate(\"雪\") }";
    let (snapshot, file) = editor_client(&engine, "memory://typed.kgr", text);
    snapshot.check_program(file, &Default::default()).unwrap();
    let doc = snapshot
        .queries()
        .documentation_at(file, text.rfind("duplicate(").unwrap())
        .unwrap();
    assert!(doc.documentation.starts_with(DOC));
    assert!(
        doc.documentation
            .contains("# Parameters\n\n## `text`\n\nInput text.\n\nUnicode is preserved.")
    );
    assert!(
        doc.documentation
            .contains("# Returns\n\nTwo independent strings.\n\n```kgr\nduplicate(\"雪\")\n```")
    );
    let target = snapshot
        .source(doc.declaration.source_location().unwrap().file)
        .unwrap();
    let twin = snapshot
        .queries()
        .definition_at(file, text.find("duplicate(").unwrap())
        .unwrap();
    let twin = snapshot
        .source(twin.source_location().unwrap().file)
        .unwrap();
    assert_eq!(
        target.module_identity(),
        &namespaces::module("application", "typed")
    );
    assert_eq!(
        twin.module_identity(),
        &namespaces::module("application", "twin")
    );
    assert_eq!(target.text(), twin.text());
    assert_ne!(target.id(), twin.id());
    assert_ne!(target.name(), twin.name());
    if publish {
        for source in [target, twin] {
            assert_eq!(
                fs::read_to_string(physical_path(source.name())).unwrap(),
                source.text()
            );
        }
    } else {
        assert_eq!(target.name(), "kagari://native/application/typed.kgr");
    }
    let compiler = engine
        .declarations(engine.source_snapshot(), &Default::default())
        .unwrap();
    let owner = compiler
        .files()
        .find(|file| file.source().module_identity() == target.module_identity())
        .unwrap();
    assert!(owner.lowered().source_surface().is_none());
    let range = target
        .local_range(doc.declaration.source_location().unwrap())
        .unwrap();
    assert_eq!(&target.text()[range.start..range.end], "duplicate");
    assert!(
        target.text().contains("text: alloc::string::String"),
        "{}",
        target.text()
    );
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("memory://typed-run.kgr", text),
            Default::default(),
        )
        .unwrap();
    let program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let result: (String, String) = runtime
        .execute_typed(&loaded, ProgramEntry::Main, (), &context)
        .unwrap();
    assert_eq!(result, ("雪".into(), "雪".into()));
}

#[test]
fn materialized_native_reexport_navigates_to_authored_core() {
    let mut builder = KagariEngine::builder().unwrap();
    let target = language::identity(Protocol::PartialEq);
    let origin = builder
        .declarations()
        .authored_origin(&target.module)
        .unwrap();
    let mut set = ModuleSetBuilder::new(builder.declarations()).unwrap();
    let facade = ModuleIdentity {
        package: PackageId("host/application".into()),
        path: vec!["cmp".into()],
    };
    set.add(ModuleDefinition::new(facade.clone(), |module| {
        module.package_alias("application");
        module.export_path(
            ExportName::new(NameNamespace::Type, "Comparable"),
            language::identity(Protocol::PartialEq),
        )
    }))
    .unwrap();
    builder
        .install_all(set.finish().unwrap().into_modules().unwrap())
        .unwrap();
    assert_eq!(
        builder.declarations().authored_origin(&target.module),
        Some(origin)
    );
    let cache = Cache::new();
    builder.declaration_cache(&cache.0);
    let mut scope = ModuleScope::default();
    scope
        .packages
        .dependencies
        .insert("foundation".into(), target.module.package.clone());
    scope.dependencies.insert(target.module.clone());
    let client = ModuleIdentity::single_file("client");
    let rules = ModuleIdentity::single_file("rules");
    let mut client_scope = ModuleScope::default();
    client_scope
        .packages
        .dependencies
        .insert("sdk".into(), facade.package.clone());
    client_scope
        .dependencies
        .extend([rules.clone(), facade.clone()]);
    builder.import_environment(ImportEnvironment {
        modules: [(facade, scope), (client.clone(), client_scope)].into(),
    });
    let engine = builder.build().unwrap();
    let name = "memory://facade-client.kgr";
    let text = "use sdk::cmp::Comparable; fn same<T: Comparable>(a: T, b: T) -> bool { a == b }";
    engine.bind_module(name, client.clone()).unwrap();
    engine
        .set_source(name, text.into(), SourceLayer::Base)
        .unwrap();
    assert!(
        matches!(engine.declarations(engine.source_snapshot(), &Default::default()),
        Err(EmbeddingError::Source { message }) if message.contains(&format!("missing dependency {rules} of {client}")))
    );
    engine.bind_module("rules.kgr", rules).unwrap();
    engine
        .set_source("rules.kgr", String::new(), SourceLayer::Base)
        .unwrap();
    let export = engine
        .native_declaration_sources(engine.source_snapshot(), &Default::default())
        .unwrap();
    let [view] = export.documents.as_slice() else {
        panic!("only the facade is Native")
    };
    assert_eq!(view.key, "kagari://native/host%2Fapplication/cmp.kgr");
    let rendered = fs::read_to_string(physical_path(&view.display_name)).unwrap();
    assert!(
        rendered.contains("pub use foundation::cmp::PartialEq as Comparable;"),
        "{rendered}"
    );
    let (mut database, sources) = editor(&engine);
    let file = sources.module(&client).unwrap().id();
    let snapshot = database.snapshot(sources, &Default::default()).unwrap();
    snapshot.check_program(file, &Default::default()).unwrap();
    let declaration = snapshot
        .queries()
        .definition_at(file, text.rfind("Comparable").unwrap())
        .unwrap();
    let location = declaration.source_location().unwrap();
    let source = snapshot.source(location.file).unwrap();
    assert_eq!(source.name(), "kagari-core:///cmp.kgr");
    assert_eq!(
        &source.text()[location.range.start..location.range.end],
        "PartialEq"
    );
}

#[test]
fn construction_rejects_invalid_bindings_of_present_importers() {
    let mut builder = KagariEngine::builder().unwrap();
    let owner = language::identity(Protocol::PartialEq).module;
    let mut invalid = ModuleScope::default();
    invalid
        .packages
        .dependencies
        .insert("self".into(), owner.package.clone());
    let pending = ModuleScope {
        dependencies: [ModuleIdentity::single_file("rules")].into(),
        ..Default::default()
    };
    builder.import_environment(ImportEnvironment {
        modules: [
            (ModuleIdentity::single_file("client"), pending),
            (owner, invalid),
        ]
        .into(),
    });
    assert!(
        matches!(builder.build(), Err(EmbeddingError::Source { message }) if message.contains("invalid package binding self"))
    );
}

#[test]
fn registered_importers_wait_for_project_preludes_and_deferred_packages() {
    let mut builder = KagariEngine::builder().unwrap();
    let support = ModuleBuilder::new("support::api", builder.declarations())
        .unwrap()
        .finish()
        .unwrap();
    let app = application(&builder, DOC);
    let support_id = support.declaration().identity.clone();
    let app_id = app.declaration().identity.clone();
    let rules = ModuleIdentity::single_file("rules");
    let mut app_scope = ModuleScope::default();
    app_scope
        .packages
        .dependencies
        .insert("selected".into(), support_id.package.clone());
    let support_scope = ModuleScope {
        preludes: [rules.clone()].into(),
        ..Default::default()
    };
    let imports = ImportEnvironment {
        modules: [(app_id, app_scope), (support_id.clone(), support_scope)].into(),
    };
    builder.install_all([app, support]).unwrap();
    builder.import_environment(imports.clone());
    let engine = builder.build().unwrap().new_session(Default::default());
    assert!(
        matches!(engine.declarations(engine.source_snapshot(), &Default::default()),
        Err(EmbeddingError::Source { message }) if message.contains(&format!("missing dependency {rules} of {support_id}")))
    );
    engine.bind_module("rules.kgr", rules).unwrap();
    engine
        .set_source("rules.kgr", String::new(), SourceLayer::Base)
        .unwrap();
    let snapshot = engine
        .declarations(engine.source_snapshot(), &Default::default())
        .unwrap();
    assert_eq!(snapshot.native_modules().len(), 2);
    for (owner, scope) in imports.modules {
        assert_eq!(
            snapshot.import_environment().modules.get(&owner),
            Some(&scope)
        );
    }
}

fn application(builder: &KagariEngineBuilder, doc: &str) -> NativeModule {
    let mut module = ModuleBuilder::new("application::api", builder.declarations()).unwrap();
    module.documentation(doc);
    let mut ty = module.define_type("Opaque").unwrap();
    ty.documentation(DOC);
    ty.native_storage(NativeStorage::data::<()>()).unwrap();
    ty.finish().unwrap();
    let mut trait_ = module.define_trait("Describe").unwrap();
    trait_.documentation(DOC);
    trait_
        .define_method(
            MethodDecl::instance("describe")
                .documentation(DOC)
                .returns(Type::i32()),
        )
        .unwrap();
    trait_.finish().unwrap();
    let function = module
        .define_function(
            FunctionDecl::new("answer")
                .documentation(doc)
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(function, |_cx: &mut NativeCall<'_>| Ok(42i32))
        .unwrap();
    module.finish().unwrap()
}

fn engine(cache: Option<&PathBuf>, doc: &str) -> KagariEngine {
    let mut builder = KagariEngine::builder().unwrap();
    builder
        .install_all(standard_library().modules().unwrap())
        .unwrap();
    let module = application(&builder, doc);
    builder.install(module).unwrap();
    if let Some(cache) = cache {
        builder.declaration_cache(cache);
    }
    builder.build().unwrap()
}

fn physical_path(uri: &str) -> PathBuf {
    {
        let path = PathBuf::from(uri);
        assert!(path.is_absolute());
        path
    }
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn generated_files_docs_and_navigation_share_the_checked_snapshot() {
    let cache = Cache::new();
    let engine = engine(Some(&cache.0), DOC);
    let text = "use application::api; use application::api::{Opaque, Describe}; fn main() -> i32 { api::answer() }";
    let (snapshot, file) = editor_client(&engine, "memory://registration.kgr", text);
    let export = engine.native_declaration_sources(engine.source_snapshot(), &Default::default()).unwrap();
    let collections = export.documents.iter()
        .find(|source| source.text.contains("pub trait List<"))
        .unwrap();
    let declarations = standard_library().declarations().declarations();
    let owner = declarations
        .iter()
        .find(|module| module.records().identity == namespaces::module("std", "collections"))
        .unwrap();
    let list = owner
        .records()
        .traits
        .iter()
        .find(|contract| contract.name == "List")
        .unwrap();
    let sorted = list
        .methods
        .iter()
        .find(|method| method.name == "sorted")
        .unwrap();
    assert_eq!(sorted.implementation, CallableImplementation::Script);
    let trait_id = owner.records().definition(kagari_common::identity::DefinitionKind::Trait, "List", owner.definitions()).unwrap();
    let method_id = kagari_types::declaration::module::ModuleDecl::method_id(trait_id, "sorted", owner.definitions()).unwrap();
    let kagari_types::declaration::script::ScriptExpr::Call { callee, type_arguments, .. } = &owner.records().script_bodies[&method_id] else { panic!("ordinary Script call") };
    assert_eq!(type_arguments.len(), 3);
    let kagari_types::declaration::script::ScriptExpr::Definition(template) = **callee else { panic!("declared Native helper") };
    assert!(owner.records().private_functions.contains(&template));
    let template = owner.definitions().resolve(template).unwrap();
    assert_eq!(template.module(), &owner.records().identity);
    let name = template.last().unwrap().name;
    let forwarding = format!("{name}::<Self, T, <Self as core::iter::Iterable<Item = T>>::Iter>(self)");
    assert!(
        collections.text.contains(&forwarding),
        "{}",
        collections.text
    );
    assert!(
        collections
            .text
            .contains(&format!("\nfn {name}<T1, T2, T3>"))
    );
    assert_eq!(
        fs::read_to_string(physical_path(&collections.display_name)).unwrap(),
        collections.text
    );
    let doc = snapshot
        .queries().documentation_at(file, text.find("answer()").unwrap())
        .unwrap();
    assert_eq!(doc.documentation, DOC);
    let target = snapshot.source(doc.declaration.source_location().unwrap().file).unwrap();
    let path = physical_path(target.name());
    assert_eq!(fs::read_to_string(&path).unwrap(), target.text());
    let range = target.local_range(doc.declaration.source_location().unwrap()).unwrap();
    assert_eq!(&target.text()[range.start..range.end], "answer");
    let module_doc = snapshot
        .queries().module_documentation_at(file, text.find("api;").unwrap())
        .unwrap();
    assert_eq!(module_doc.documentation, DOC);
    assert_eq!(module_doc.location.unwrap().file, target.id());
    for name in ["Opaque", "Describe"] {
        let item = snapshot
            .queries().documentation_at(file, text.find(name).unwrap())
            .unwrap();
        assert_eq!(item.documentation, DOC);
    }
    let source = export.documents.iter().find(|source| &source.module == target.module_identity()).unwrap();
    assert_eq!(source.display_name, target.name());
    assert_eq!(source.text, target.text());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("memory://run.kgr", text),
            Default::default(),
        )
        .unwrap();
    let program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        42
    );
}
*/

#[test]
fn task_navigation_uses_canonical_authored_members_and_docs() {
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
    let text = "fn stop(task: std::task::Task<i32>) { task.cancel(); } fn submit(scope: TaskScope) -> Result<Task<i32>, SpawnError> { scope.spawn(async || 42) } fn retain(future: Future<i32>) -> Future<i32> { future }";
    let file = engine
        .set_source("memory://tasks.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = engine
        .analyze(engine.source_snapshot(), &Default::default())
        .unwrap();
    snapshot.check_program(file, &Default::default()).unwrap();
    let declarations = standard_library().declarations().declarations();
    for (receiver_name, member) in [("Task", "cancel"), ("TaskScope", "spawn")] {
        let owner = declarations
            .iter()
            .find(|module| module.records().identity == namespaces::module("core", "task"))
            .unwrap();
        let (index, implementation) = owner
            .records()
            .implementations
            .iter()
            .enumerate()
            .find(|(_, implementation)| {
                let Ty::NativeObject(receiver) = &implementation.for_type else {
                    return false;
                };
                implementation.trait_type.is_none()
                    && owner
                        .definitions()
                        .resolve(receiver.declaration)
                        .unwrap()
                        .last()
                        .unwrap()
                        .name
                        == receiver_name
            })
            .unwrap();
        assert!(
            implementation
                .methods
                .iter()
                .any(|method| method.name == member)
        );
        let expected = ModuleDecl::method_id(
            owner
                .records()
                .implementation_id(index, owner.definitions())
                .unwrap(),
            member,
            owner.definitions(),
        )
        .unwrap();
        let documentation = snapshot
            .queries()
            .documentation_at(file, text.find(&format!("{member}(")).unwrap())
            .unwrap();
        let DeclarationId::Definition(actual) = documentation.declaration.id else {
            panic!("canonical method declaration")
        };
        let actual = snapshot.definitions().resolve(actual).unwrap();
        let identity = owner.definitions().resolve(expected).unwrap();
        assert_eq!(actual.module(), identity.module());
        assert_eq!(
            actual.segments().collect::<Vec<_>>(),
            identity.segments().collect::<Vec<_>>()
        );
        assert_eq!(
            documentation.documentation,
            owner.records().documentation[&expected]
        );
        assert!(documentation.declaration.source_location().is_none());
        assert!(documentation.written_signature.is_none());
    }
    let owner = declarations
        .iter()
        .find(|module| module.records().identity == namespaces::module("core", "task"))
        .unwrap();
    for name in ["Task", "TaskScope", "SpawnError"] {
        let ty = owner
            .records()
            .types
            .iter()
            .find(|ty| ty.name == name)
            .unwrap();
        let expected = owner
            .records()
            .definition(ty.kind.definition_kind(), name, owner.definitions())
            .unwrap();
        let offset = text
            .find(if name == "Task" { "Task<i32>" } else { name })
            .unwrap();
        let documentation = snapshot.queries().documentation_at(file, offset).unwrap();
        let DeclarationId::Definition(actual) = documentation.declaration.id else {
            panic!("canonical type declaration")
        };
        let actual = snapshot.definitions().resolve(actual).unwrap();
        let identity = owner.definitions().resolve(expected).unwrap();
        assert_eq!(actual.module(), identity.module());
        assert_eq!(
            actual.segments().collect::<Vec<_>>(),
            identity.segments().collect::<Vec<_>>()
        );
        assert_eq!(
            documentation.documentation,
            owner.records().documentation[&expected]
        );
    }
    let future = snapshot
        .queries()
        .documentation_at(file, text.find("Future").unwrap())
        .unwrap();
    let DeclarationId::Definition(id) = future.declaration.id else {
        panic!("canonical Future declaration")
    };
    let identity = snapshot.definitions().resolve(id).unwrap();
    assert_eq!(identity.module(), &namespaces::module("core", "future"));
    let location = future.declaration.source_location().unwrap();
    let source = snapshot.source(location.file).unwrap();
    assert_eq!(source.name(), "kagari-core:///future.kgr");
    let range = source.local_range(location).unwrap();
    assert_eq!(&source.text()[range.start..range.end], "Future");
    assert!(!future.documentation.is_empty());
}

#[test]
fn content_paths_reuse_unchanged_docs_and_keep_old_snapshot_targets() {
    let cache = Cache::new();
    let old = engine(Some(&cache.0), DOC);
    let again = engine(Some(&cache.0), DOC);
    let paths = |engine: &KagariEngine| {
        engine
            .native_declaration_sources(Default::default(), &Default::default())
            .unwrap()
            .documents
            .into_iter()
            .map(|d| (d.module, (d.key, d.display_name)))
            .collect::<BTreeMap<_, _>>()
    };
    let old_paths = paths(&old);
    assert_eq!(old_paths, paths(&again));
    let changed = engine(Some(&cache.0), "# Changed\n\nNew documentation.");
    let module = ModuleIdentity {
        package: PackageId("application".into()),
        path: vec!["api".into()],
    };
    assert_eq!(old_paths[&module].0, paths(&changed)[&module].0);
    let (mut db, sources) = editor(&old);
    let old_snapshot = db.declarations(sources, &Default::default()).unwrap();
    let (mut db, sources) = editor(&changed);
    let new_snapshot = db.declarations(sources, &Default::default()).unwrap();
    let old_doc = old_snapshot
        .queries()
        .module_documentation(&module)
        .unwrap();
    let new_doc = new_snapshot
        .queries()
        .module_documentation(&module)
        .unwrap();
    assert_eq!(old_doc.documentation, DOC);
    assert_ne!(old_doc.documentation, new_doc.documentation);
    let old_source = old_snapshot.source(old_doc.location.unwrap().file).unwrap();
    let new_source = new_snapshot.source(new_doc.location.unwrap().file).unwrap();
    assert_ne!(old_source.name(), new_source.name());
    assert_eq!(
        fs::read_to_string(physical_path(old_source.name())).unwrap(),
        old_source.text()
    );
    assert_eq!(
        old_snapshot
            .queries()
            .module_documentation(&module)
            .unwrap()
            .documentation,
        DOC
    );
    let artifact = old
        .compile_to_artifact(
            SourceFile::new(
                "memory://doc-compat.kgr",
                "use application::api::answer; fn main() -> i32 { answer() }",
            ),
            Default::default(),
        )
        .unwrap();
    let program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = changed.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        42
    );
}

#[test]
fn failed_registration_is_atomic_and_default_embedding_uses_memory() {
    let mut builder = KagariEngine::builder().unwrap();
    builder
        .install_all(standard_library().modules().unwrap())
        .unwrap();
    let one = application(&builder, DOC);
    let two = application(&builder, DOC);
    assert!(builder.install_all([one.clone(), two]).is_err());
    builder.install(one).unwrap();
    let engine = builder.build().unwrap();
    assert!(
        engine
            .native_declaration_sources(Default::default(), &Default::default())
            .unwrap()
            .documents
            .iter()
            .all(|source| source.key == source.display_name && source.key.starts_with("kagari://"))
    );
    assert!(
        engine
            .compile_source(SourceFile::new(
                "memory://atomic.kgr",
                "use application::api::answer; fn main() -> i32 { answer() }"
            ))
            .is_ok()
    );
}

#[test]
fn cache_failure_preserves_io_error_and_rejects_engine_publication() {
    let cache = Cache::new();
    fs::create_dir_all(&cache.0).unwrap();
    let blocked = cache.0.join("file");
    fs::write(&blocked, "cannot be a directory").unwrap();
    let mut builder = KagariEngine::builder().unwrap();
    builder
        .install_all(standard_library().modules().unwrap())
        .unwrap();
    builder.declaration_cache(&blocked);
    match builder.build().unwrap_err() {
        EmbeddingError::DeclarationCache { path, error } => {
            assert!(path.starts_with(&blocked));
            assert_ne!(error.kind(), std::io::ErrorKind::NotFound);
        }
        error => panic!("unexpected cache error: {error:?}"),
    }
}

#[test]
fn modified_cache_content_is_rejected_without_replacing_an_active_target() {
    let cache = Cache::new();
    let old = engine(Some(&cache.0), DOC);
    let (mut db, sources) = editor(&old);
    let snapshot = db.declarations(sources, &Default::default()).unwrap();
    let module = ModuleIdentity {
        package: PackageId("application".into()),
        path: vec!["api".into()],
    };
    let doc = snapshot.queries().module_documentation(&module).unwrap();
    let source = snapshot.source(doc.location.unwrap().file).unwrap();
    let path = physical_path(source.name());
    fs::write(&path, "edited cache").unwrap();
    let mut builder = KagariEngine::builder().unwrap();
    builder
        .install_all(standard_library().modules().unwrap())
        .unwrap();
    let module = application(&builder, DOC);
    builder.install(module).unwrap();
    builder.declaration_cache(&cache.0);
    assert!(matches!(
        builder.build(),
        Err(EmbeddingError::DeclarationCache { .. })
    ));
    assert_eq!(fs::read_to_string(path).unwrap(), "edited cache");
    assert!(source.text().contains("pub native fn answer"));
    assert_eq!(
        snapshot
            .queries()
            .module_documentation(&ModuleIdentity {
                package: PackageId("application".into()),
                path: vec!["api".into()]
            })
            .unwrap()
            .documentation,
        DOC
    );
}
