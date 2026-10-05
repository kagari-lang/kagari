#![cfg(feature = "source")]
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_embed::{
    context::ExecutionContext, engine::KagariEngine, error::EmbeddingError,
    program::PreparedProgram,
};
use kagari_runtime::{
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        context::CallContext,
        declarations::{FunctionDecl, MethodDecl},
        module::NativeModule,
        registration::FunctionSpec,
        storage::{NativePayload, NativeStorage},
        typed::NativeContext,
        types::Type,
    },
    value::Value,
};
use kagari_source::{source::SourceFile, source_database::SourceLayer};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const DOC: &str = "# Registered API\n\nParagraph with [a link](https://example.com).\n\n- First item\n- Second item\n\n```kgr\nfn sample() -> i32 { 42 }\n```\n\n中文 😀";
static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn typed_registration_preserves_parameter_and_return_docs_in_materialized_navigation() {
    let cache = Cache::new();
    let mut builder = KagariEngine::builder().unwrap();
    let mut module = ModuleBuilder::new("application::typed", builder.declarations());
    module.documentation("# Text tools\n\nOwned Unicode text conversions.");
    module
        .add_function(
            FunctionSpec::new("duplicate")
                .parameter_names(["text"])
                .documentation(DOC)
                .parameter_documentation("text", "Input text.\n\nUnicode is preserved.")
                .return_documentation("Two independent strings.\n\n```kgr\nduplicate(\"雪\")\n```"),
            |_: &mut NativeContext<'_>, (text,): (String,)| -> NativeResult<(String, String)> {
                Ok((text.clone(), text))
            },
        )
        .unwrap();
    builder.install(module.finish().unwrap()).unwrap();
    builder.declaration_cache(&cache.0);
    let engine = builder.build().unwrap();
    let text =
        "use application::typed::duplicate; fn main() -> (String, String) { duplicate(\"雪\") }";
    let file = engine
        .set_source("memory://typed.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = engine
        .analyze(engine.source_snapshot(), &Default::default())
        .unwrap();
    snapshot.check_program(file, &Default::default()).unwrap();
    let doc = snapshot
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
    let target = snapshot.source(doc.declaration.location.file).unwrap();
    assert_eq!(
        fs::read_to_string(physical_path(target.name())).unwrap(),
        target.text()
    );
    let range = target.local_range(doc.declaration.location).unwrap();
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
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let result: (String, String) = runtime
        .execute_typed(&loaded, "main", (), &context)
        .unwrap();
    assert_eq!(result, ("雪".into(), "雪".into()));
}

struct Cache(PathBuf);
impl Cache {
    fn new() -> Self {
        Self(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/lr02-cache-tests/中文 cache")
                .join(format!(
                    "{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                )),
        )
    }
}
impl Drop for Cache {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug)]
struct Payload;
impl NativePayload for Payload {
    fn trace<'payload>(&'payload self, _: &mut dyn FnMut(&'payload Value)) {}
    fn units(&self) -> usize {
        0
    }
}

fn application(
    builder: &kagari_embed::engine::builder::KagariEngineBuilder,
    doc: &str,
) -> NativeModule {
    let mut module = ModuleBuilder::new("application::api", builder.declarations());
    module.documentation(doc);
    let mut ty = module.define_type("Opaque");
    ty.documentation(DOC);
    ty.native_storage(NativeStorage::payload::<Payload>())
        .unwrap();
    ty.finish().unwrap();
    let mut trait_ = module.define_trait("Describe");
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
        .bind(function, |_cx: &mut CallContext<'_>| Ok(42i32))
        .unwrap();
    module.finish().unwrap()
}

fn engine(cache: Option<&PathBuf>, doc: &str) -> KagariEngine {
    let mut builder = KagariEngine::builder().unwrap();
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

#[test]
fn generated_files_docs_and_navigation_share_the_checked_snapshot() {
    let cache = Cache::new();
    let engine = engine(Some(&cache.0), DOC);
    let text = "use application::api; use application::api::{Opaque, Describe}; fn main() -> i32 { api::answer() }";
    let file = engine
        .set_source("memory://registration.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = engine
        .analyze(engine.source_snapshot(), &Default::default())
        .unwrap();
    let doc = snapshot
        .documentation_at(file, text.find("answer()").unwrap())
        .unwrap();
    assert_eq!(doc.documentation, DOC);
    let target = snapshot.source(doc.declaration.location.file).unwrap();
    let path = physical_path(target.name());
    assert_eq!(fs::read_to_string(&path).unwrap(), target.text());
    let range = target.local_range(doc.declaration.location).unwrap();
    assert_eq!(&target.text()[range.start..range.end], "answer");
    let module_doc = snapshot
        .module_documentation_at(file, text.find("api;").unwrap())
        .unwrap();
    assert_eq!(module_doc.documentation, DOC);
    assert_eq!(module_doc.location.file, target.id());
    for name in ["Opaque", "Describe"] {
        let item = snapshot
            .documentation_at(file, text.find(name).unwrap())
            .unwrap();
        assert_eq!(item.documentation, DOC);
    }
    let source = engine
        .native_declaration_sources()
        .iter()
        .find(|source| source.uri == target.name())
        .unwrap();
    for site in source.sites.values() {
        assert!(site.name_span.end <= source.text.len());
        assert!(site.span.start <= site.name_span.start);
    }
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("memory://run.kgr", text),
            Default::default(),
        )
        .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn content_paths_reuse_unchanged_docs_and_keep_old_snapshot_targets() {
    let cache = Cache::new();
    let old = engine(Some(&cache.0), DOC);
    let again = engine(Some(&cache.0), DOC);
    assert_eq!(
        old.native_declaration_sources()
            .iter()
            .map(|s| &s.uri)
            .collect::<Vec<_>>(),
        again
            .native_declaration_sources()
            .iter()
            .map(|s| &s.uri)
            .collect::<Vec<_>>()
    );
    let changed = engine(Some(&cache.0), "# Changed\n\nNew documentation.");
    let module = ModuleIdentity {
        package: PackageId("application".into()),
        path: vec!["api".into()],
    };
    let old_snapshot = old
        .declarations(old.source_snapshot(), &Default::default())
        .unwrap();
    let new_snapshot = changed
        .declarations(changed.source_snapshot(), &Default::default())
        .unwrap();
    let old_doc = old_snapshot.module_documentation(&module).unwrap();
    let new_doc = new_snapshot.module_documentation(&module).unwrap();
    assert_eq!(old_doc.documentation, DOC);
    assert_ne!(old_doc.documentation, new_doc.documentation);
    let old_source = old_snapshot.source(old_doc.location.file).unwrap();
    let new_source = new_snapshot.source(new_doc.location.file).unwrap();
    assert_ne!(old_source.name(), new_source.name());
    assert_eq!(
        fs::read_to_string(physical_path(old_source.name())).unwrap(),
        old_source.text()
    );
    assert_eq!(
        old_snapshot
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
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = changed.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn failed_registration_is_atomic_and_default_embedding_uses_memory() {
    let mut builder = KagariEngine::builder().unwrap();
    let one = application(&builder, DOC);
    let two = application(&builder, DOC);
    assert!(builder.install_all([one.clone(), two]).is_err());
    builder.install(one).unwrap();
    let engine = builder.build().unwrap();
    assert!(
        engine
            .native_declaration_sources()
            .iter()
            .all(|source| source.uri.starts_with("kagari://"))
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
    let snapshot = old
        .declarations(old.source_snapshot(), &Default::default())
        .unwrap();
    let module = ModuleIdentity {
        package: PackageId("application".into()),
        path: vec!["api".into()],
    };
    let doc = snapshot.module_documentation(&module).unwrap();
    let source = snapshot.source(doc.location.file).unwrap();
    let path = physical_path(source.name());
    fs::write(&path, "edited cache").unwrap();
    let mut builder = KagariEngine::builder().unwrap();
    let module = application(&builder, DOC);
    builder.install(module).unwrap();
    builder.declaration_cache(&cache.0);
    assert!(matches!(
        builder.build(),
        Err(EmbeddingError::DeclarationCache { .. })
    ));
    assert_eq!(fs::read_to_string(path).unwrap(), "edited cache");
    assert!(source.text().contains("pub fn answer"));
    assert_eq!(
        snapshot
            .module_documentation(&ModuleIdentity {
                package: PackageId("application".into()),
                path: vec!["api".into()]
            })
            .unwrap()
            .documentation,
        DOC
    );
}
