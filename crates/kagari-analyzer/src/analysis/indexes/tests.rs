use crate::{
    analysis::{
        AnalysisState,
        diagnostics::ProjectionLimits,
        features::{DocumentQueryInput, navigation::implementation::ImplementationStep},
        indexes::{ImplementationIndex, IndexFailure},
    },
    documents::DocumentInput,
    protocol::source_name,
    tests::fixture::Fixture,
    workspace::{CaptureRequest, acquisition, budget::InputBudget},
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::{
    analysis::signatures::relations::ImplementationTarget,
    declarations::DeclarationId,
    native::render::NATIVE_RENDERER_NAMESPACE,
    types::{NominalType, TypeId},
};
use kagari_source::{generated::GeneratedDocuments, source::SourceFile};
use lsp_types::GotoDefinitionResponse;
use std::{fs, sync::Arc};
use tokio::sync::oneshot;

#[test]
fn reverse_dependents_require_complete_shards_and_exact_cross_context_provenance() {
    let fixture = Fixture::new();
    let cloned = Fixture::new();
    let api = include_str!("../../../tests/fixtures/analysis/index-api.kgr");
    for fixture in [&fixture, &cloned] {
        fs::write(fixture.directory().join("first/api/src/lib.kgr"), api).unwrap();
        fs::write(
            fixture.directory().join("first/app/src/lib.kgr"),
            include_str!("../../../tests/fixtures/analysis/index-app.kgr"),
        )
        .unwrap();
    }
    let root = source_name(&fixture.uri("first")).unwrap();
    let budget = InputBudget::new(256 * 1024 * 1024);
    let worker = acquisition::start(root.clone(), budget.clone()).unwrap();
    let api_uri = fixture.uri("first/api/src/lib.kgr");
    let api_name = source_name(&api_uri).unwrap();
    let document = DocumentInput {
        source: Some(Arc::new(SourceFile::new(&api_name, api))),
        source_name: api_name.clone(),
        uri: api_uri.clone(),
        version: 1,
        incarnation: 1,
        unavailable: false,
    };
    let (sender, receiver) = oneshot::channel();
    worker
        .port
        .submit(
            CaptureRequest::new(
                1,
                vec![fixture.uri("first"), cloned.uri("first")],
                vec![document.clone()],
                &budget,
            )
            .unwrap(),
            CancellationToken::default(),
            sender,
        )
        .unwrap();
    let capture = Arc::new(receiver.blocking_recv().unwrap().unwrap());
    assert_eq!(capture.contexts.len(), 4);
    let selected = capture
        .contexts
        .iter()
        .find(|context| {
            context.origins.values().any(|origin| {
                origin.uri.as_ref() == Some(&api_uri) && origin.module == context.root
            })
        })
        .unwrap();
    let mut state = AnalysisState::new(
        root,
        budget.clone(),
        ProjectionLimits {
            uris: 1024,
            diagnostics: 16_384,
            bytes: 16 * 1024 * 1024,
        },
    );
    let store = GeneratedDocuments::open(
        &fixture.directory().join("cache"),
        NATIVE_RENDERER_NAMESPACE,
    )
    .unwrap();
    state.prepare(&store, &Default::default()).unwrap();
    let signatures = state
        .signatures_for_context(selected, &Default::default())
        .unwrap();
    let file = selected.sources.module(&selected.root).unwrap().id();
    let declaration = signatures
        .snapshot
        .file(file)
        .unwrap()
        .definition_at(api.find("Read").unwrap())
        .unwrap();
    let DeclarationId::Definition(id) = declaration.id else {
        panic!("trait identity")
    };
    let target = ImplementationTarget {
        declaration: Some(id),
        receiver: None,
        assumptions: Default::default(),
    };
    let mut index = ImplementationIndex::new(capture.clone(), 1, Default::default()).unwrap();
    assert!(matches!(
        index.query(
            &selected.key,
            &signatures.snapshot,
            &target,
            &Default::default()
        ),
        Err(IndexFailure::Incomplete(_))
    ));
    while let Some(context) = index.next_context().cloned() {
        let shard = state
            .signatures_for_context(&context, &Default::default())
            .unwrap();
        index
            .insert(
                &context.key,
                shard.snapshot,
                &budget,
                usize::MAX,
                &Default::default(),
            )
            .unwrap();
        drop(shard._lease);
    }
    let result = index
        .query(
            &selected.key,
            &signatures.snapshot,
            &target,
            &Default::default(),
        )
        .unwrap();
    assert_eq!(
        result.len(),
        1,
        "the reverse dependent in this provider is found while an independent physical namespace clone stays unrelated"
    );
    let clone_api = cloned.uri("first/api/src/lib.kgr");
    let clone_context = capture
        .contexts
        .iter()
        .find(|context| {
            context.origins.values().any(|origin| {
                origin.uri.as_ref() == Some(&clone_api) && origin.module == context.root
            })
        })
        .unwrap();
    assert_eq!(
        clone_context.root, selected.root,
        "the two physical providers deliberately have equal explicit semantic package identities"
    );
    let destination = &result[0];
    let context = capture
        .contexts
        .iter()
        .find(|context| context.key == destination.context)
        .unwrap();
    assert_ne!(context.key, selected.key);
    assert!(
        context
            .origins
            .get(&destination.location.selection.file)
            .is_some_and(
                |origin| origin.uri.as_ref() == Some(&fixture.uri("first/app/src/lib.kgr"))
            )
    );
    assert_ne!(
        signatures.snapshot.definitions().id(),
        index.signatures(&context.key).unwrap().definitions().id(),
        "portable joins must work across independent identity writers"
    );

    let caller = index.signatures(&context.key).unwrap();
    let caller_file = context.sources.module(&context.root).unwrap().id();
    let caller_file = caller.file(caller_file).unwrap();
    let source = caller_file.source().text();
    let DeclarationId::Definition(function) = caller_file
        .definition_at(
            source
                .find("inspect<T>")
                .unwrap_or_else(|| source.find("inspect<T:").unwrap()),
        )
        .unwrap()
        .id
    else {
        panic!("caller identity")
    };
    let parameter = caller_file
        .declarations()
        .parameters_of(&function)
        .into_iter()
        .next()
        .unwrap();
    let imported = caller
        .declaration_snapshot()
        .files()
        .find(|file| file.source().text() == api)
        .unwrap();
    let DeclarationId::Definition(method) = imported
        .declarations()
        .site_at(api.find("read(self)").unwrap())
        .unwrap()
        .id
    else {
        panic!("trait member identity")
    };
    let trait_id = caller.definitions().parent(method).unwrap().unwrap();
    let receiver = TypeId::Generic(parameter);
    let open = ImplementationTarget {
        declaration: Some(method),
        receiver: Some(receiver.clone()),
        assumptions: [(
            receiver,
            vec![NominalType {
                declaration: trait_id,
                arguments: vec![],
                associated_types: Default::default(),
            }],
        )]
        .into(),
    };
    assert!(
        matches!(index.query(&context.key, caller, &open, &Default::default()), Err(IndexFailure::Incomplete(message)) if message.contains("bindings")),
        "a shared trait anchor cannot turn an untranslatable caller binder into partial exhaustive success"
    );

    let mut conflicting = (*capture).clone();
    for context in &mut conflicting.contexts {
        if context.key != selected.key
            && let Some(origin) = context
                .origins
                .values_mut()
                .find(|origin| origin.uri.as_ref() == Some(&api_uri))
        {
            origin.module.path.push("other-semantic-module".into());
        }
    }
    let mut rejected =
        ImplementationIndex::new(Arc::new(conflicting), 1, Default::default()).unwrap();
    while let Some(context) = rejected.next_context().cloned() {
        let shard = state
            .signatures_for_context(&context, &Default::default())
            .unwrap();
        rejected
            .insert(
                &context.key,
                shard.snapshot,
                &budget,
                usize::MAX,
                &Default::default(),
            )
            .unwrap();
        drop(shard._lease);
    }
    assert!(
        matches!(rejected.query(&selected.key, &signatures.snapshot, &target, &Default::default()), Err(IndexFailure::Incomplete(message)) if message.contains("provenance"))
    );

    state.retain_contexts(
        &capture
            .contexts
            .iter()
            .map(|context| context.key.clone())
            .collect(),
    );
    state.retain_input_generation(capture.generation);
    let input = DocumentQueryInput {
        serial: 7,
        capture: capture.clone(),
        documents: vec![document].into(),
        source_name: api_name,
        uri: api_uri,
        offset: api.find("Read").unwrap(),
        limit: 1024 * 1024,
    };
    let mut completed = false;
    for _ in 0..=capture.contexts.len() + 1 {
        if let ImplementationStep::Complete(result) =
            state.implementation(&input, &Default::default()).unwrap()
        {
            assert!(
                matches!(result, Some(GotoDefinitionResponse::Array(locations)) if locations.len() == 1)
            );
            completed = true;
            break;
        }
    }
    assert!(completed);
    assert!(
        state.indexes.retained_bytes() > 0,
        "the completed current scope is warm"
    );
    state.retain_input_generation(capture.generation + 1);
    assert_eq!(
        state.indexes.retained_bytes(),
        0,
        "advancing inputs retires a completed stale view without another implementation query"
    );
    assert!(
        !state.contexts.is_empty(),
        "input acquisition does not force a cold database reset"
    );
    drop(worker.port);
    worker.thread.join().unwrap();
    worker.terminated.blocking_recv().unwrap().unwrap();
}
