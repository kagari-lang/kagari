use super::*;
use kagari_hir::native::render::NATIVE_RENDERER_NAMESPACE;
use kagari_types::{callable::CallableImplementation, scalar::BuiltinType, ty::Ty};
use std::{
    fs,
    path::PathBuf,
    process,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn documents() -> GeneratedDocuments {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/lsp-environment-tests/中文 cache")
        .join(format!(
            "{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    GeneratedDocuments::open(&root, NATIVE_RENDERER_NAMESPACE).unwrap()
}

#[test]
fn standard_environment_validates_final_paths_before_publishing_navigation_text() {
    let documents = documents();
    let cancel = CancellationToken::default();
    let mut prepared = prepare_standard("/lsp", &documents, &cancel).unwrap();
    assert!(!prepared.publications.is_empty());
    assert!(
        prepared
            .publications
            .iter()
            .all(|document| !document.path().exists())
    );
    let snapshot = prepared
        ._database
        .snapshot(SourceSnapshot::default(), &cancel)
        .unwrap();
    let independent = prepared
        .context_database()
        .unwrap()
        .snapshot(SourceSnapshot::default(), &cancel)
        .unwrap();
    assert_ne!(
        snapshot.definitions().id(),
        independent.definitions().id(),
        "user contexts must not share the environment's append-only identity writer"
    );
    let mut declarations = 0;
    for file in snapshot.declaration_snapshot().files() {
        let source = file.source();
        assert!(
            prepared
                .sources
                .iter()
                .any(|retained| retained.content_id() == source.content_id())
        );
        let document = prepared
            .publications
            .iter()
            .find(|document| document.source_name() == source.name())
            .expect("validated source has a planned publication");
        assert_eq!(source.text(), document.text());
        assert!(
            snapshot
                .file(source.id())
                .unwrap()
                .result()
                .diagnostics()
                .is_empty()
        );
        for declaration in file.declarations().iter() {
            let target = snapshot.source(declaration.location.file).unwrap();
            assert!(
                prepared
                    .sources
                    .iter()
                    .any(|retained| retained.content_id() == target.content_id())
            );
            let range = target.local_range(declaration.location).unwrap();
            assert_eq!(&target.text()[range.start..range.end], declaration.name);
            declarations += 1;
        }
    }
    assert!(declarations > 0);
    for document in prepared.publications.iter() {
        document.publish().unwrap();
        assert_eq!(
            fs::read_to_string(document.path()).unwrap(),
            document.text()
        );
    }
}

#[test]
fn invalid_cancelled_or_oversized_environments_do_not_publish_files() {
    let documents = documents();
    let mut providers = catalog::shared().unwrap();
    let index = providers
        .iter()
        .position(|provider| {
            provider
                .records()
                .traits
                .iter()
                .any(|item| item.name == "List")
        })
        .unwrap();
    let (definitions, mut module) = providers[index].as_ref().clone().into_parts();
    let sorted = module
        .traits
        .iter()
        .find(|item| item.name == "List")
        .unwrap()
        .methods
        .iter()
        .find(|method| method.name == "sorted")
        .unwrap();
    let CallableImplementation::NativeDefault(application) = &sorted.implementation else {
        panic!("List::sorted retains a real default body");
    };
    let template = application.declaration;
    module
        .functions
        .iter_mut()
        .find(|function| function.implementation == CallableImplementation::Native(template))
        .unwrap()
        .return_type = Ty::Builtin(BuiltinType::Bool);
    providers[index] = Arc::new(
        DefinitionMetadata::checked(definitions, module, &CancellationToken::default()).unwrap(),
    );
    // The catalog inventory and identity scope remain valid. Native analysis,
    // beyond set_native_sources' inventory check, must reject this wrong contract.
    let error = prepare("/lsp", &documents, providers, &CancellationToken::default())
        .err()
        .expect("invalid generated default body must be rejected");
    assert!(
        error.starts_with("NativeApi(") && error.contains("invalid native declarations"),
        "{error}"
    );
    let cancel = CancellationToken::default();
    cancel.cancel();
    let error = prepare_standard("/lsp", &documents, &cancel)
        .err()
        .expect("cancelled bootstrap cannot produce an environment");
    assert!(error.contains("cancelled"), "{error}");
    let mut oversized = catalog::shared().unwrap();
    let (definitions, mut module) = oversized[0].as_ref().clone().into_parts();
    // Generated scale witnesses the separate publication-byte bound.
    module.module_documentation = "x".repeat(MAX_PUBLICATION_BYTES);
    oversized[0] = Arc::new(
        DefinitionMetadata::checked(definitions, module, &CancellationToken::default()).unwrap(),
    );
    let error = prepare("/lsp", &documents, oversized, &CancellationToken::default())
        .err()
        .expect("publication bytes must be bounded before HIR admission");
    assert!(error.contains("publication limit"), "{error}");
    let directory = documents
        .plan("".into())
        .path()
        .parent()
        .unwrap()
        .to_owned();
    assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
}
