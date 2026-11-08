use crate::{
    analysis::{
        AnalysisFailure, AnalysisState, diagnostics::ProjectionLimits, features::FeatureInput,
        worker,
    },
    documents::DocumentInput,
    protocol::source_name,
    tests::fixture::Fixture,
    workspace::{
        CaptureIssue, CaptureRequest, WorkspaceCapture, acquisition, budget::InputBudget,
        origins::file_uri,
    },
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::native::render::NATIVE_RENDERER_NAMESPACE;
use kagari_source::{generated::GeneratedDocuments, source::SourceFile};
use lsp_types::{GotoDefinitionResponse, NumberOrString, Position, Range, Uri};
use std::{
    collections::BTreeSet,
    slice::from_ref,
    sync::{Arc, mpsc::sync_channel},
    time::Duration,
};
use tokio::sync::oneshot;

const SOURCE: &str = include_str!("../../tests/fixtures/diagnostics/inline-error.kgr");

fn limits() -> ProjectionLimits {
    ProjectionLimits {
        uris: 1024,
        diagnostics: 16_384,
        bytes: 16 * 1024 * 1024,
    }
}

fn document(uri: Uri, text: &str, version: i32) -> DocumentInput {
    let source_name = source_name(&uri).unwrap();
    DocumentInput {
        source: Some(Arc::new(SourceFile::new(&source_name, text))),
        source_name,
        uri,
        version,
        incarnation: 1,
        unavailable: false,
    }
}

fn capture(
    port: &acquisition::AcquisitionPort,
    budget: &InputBudget,
    generation: u64,
    documents: &[DocumentInput],
) -> WorkspaceCapture {
    let (sender, receiver) = oneshot::channel();
    port.submit(
        CaptureRequest::new(generation, vec![], documents.to_vec(), budget).unwrap(),
        CancellationToken::default(),
        sender,
    )
    .unwrap();
    receiver.blocking_recv().unwrap().unwrap()
}

#[test]
fn full_analysis_projects_inline_ranges_reuses_context_and_retains_leases_until_retirement() {
    let fixture = Fixture::new();
    let budget = InputBudget::new(256 * 1024 * 1024);
    let root = source_name(&fixture.uri("")).unwrap();
    let worker = acquisition::start(root.clone(), budget.clone()).unwrap();
    let mut state = AnalysisState::new(root.clone(), budget.clone(), limits());
    let store = GeneratedDocuments::open(
        &fixture.directory().join("cache"),
        NATIVE_RENDERER_NAMESPACE,
    )
    .unwrap();
    state
        .prepare(&store, &CancellationToken::default())
        .unwrap();
    let baseline_bytes = budget.retained_bytes();
    let baseline_identity = state.environment.as_mut().unwrap().baseline_identity();
    assert!(baseline_bytes > 0, "native source allocations are charged");
    let uri: Uri = "untitled:diagnostic-test".parse().unwrap();
    let first = document(uri.clone(), SOURCE, 1);
    let mut old = capture(&worker.port, &budget, 1, from_ref(&first));
    let key = old.contexts[0].key.clone();
    let result = state
        .diagnostics(&old, Some(&key), &[first], &CancellationToken::default())
        .unwrap();
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    assert_eq!(result.groups.len(), 1);
    assert_eq!(result.groups[0].uri(), &uri);
    let records = result.groups[0].records();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].diagnostic().code,
        Some(NumberOrString::String("KG_RESOLVE_UNKNOWN_NAME".into()))
    );
    assert_eq!(
        records[0].diagnostic().range,
        Range::new(Position::new(2, 8), Position::new(2, 21))
    );
    let held = state
        .snapshot(&old.contexts[0], &CancellationToken::default())
        .unwrap();
    let old_file = old.contexts[0].sources.files().next().unwrap().id();
    assert_ne!(held.snapshot.definitions().id(), baseline_identity.0);
    old.errors.push(CaptureIssue {
        uri: Some(uri.clone()),
        range: Some(0..usize::MAX),
        message: "invalid acquisition location".into(),
    });
    let unlocated = state
        .diagnostics(&old, None, &[], &CancellationToken::default())
        .unwrap();
    assert!(
        unlocated.groups.is_empty(),
        "untrusted locations never become zero-range diagnostics"
    );
    assert_eq!(unlocated.issues.len(), 1);
    assert!(unlocated.issues[0].contains("invalid acquisition location"));
    let second = document(uri, &SOURCE.replace("missing_value", "7"), 2);
    let current = capture(&worker.port, &budget, 2, from_ref(&second));
    assert_eq!(current.contexts[0].key, key);
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        state.diagnostics(&current, Some(&key), from_ref(&second), &cancel),
        Err(AnalysisFailure::Cancelled)
    ));
    assert_eq!(
        state.contexts[&key].capture.sources.revision(),
        old.contexts[0].sources.revision()
    );
    let clean = state
        .diagnostics(
            &current,
            Some(&key),
            from_ref(&second),
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(state.contexts.len(), 1);
    assert_eq!(
        clean.groups.len(),
        1,
        "first clean results also have explicit readiness"
    );
    assert!(clean.groups[0].records().is_empty());
    state.projection_limits.uris = 0;
    assert!(matches!(
        state.diagnostics(&current, Some(&key), &[], &CancellationToken::default()),
        Err(AnalysisFailure::Capacity(_))
    ));
    state.projection_limits = limits();
    let other = document("untitled:independent-context".parse().unwrap(), SOURCE, 1);
    let other_name = other.source_name.clone();
    let both = capture(&worker.port, &budget, 3, &[second.clone(), other]);
    let other_key = &both.document_contexts[&other_name].context;
    let other_context = both
        .contexts
        .iter()
        .find(|context| &context.key == other_key)
        .unwrap();
    let other_held = state
        .snapshot(other_context, &CancellationToken::default())
        .unwrap();
    assert_ne!(
        held.snapshot.definitions().id(),
        other_held.snapshot.definitions().id()
    );
    assert_ne!(other_held.snapshot.definitions().id(), baseline_identity.0);
    assert_eq!(
        state.environment.as_mut().unwrap().baseline_identity(),
        baseline_identity,
        "user declarations never enter the permanent native identity writer"
    );
    assert_eq!(state.contexts.len(), 2);
    let initializer_held = {
        let text = include_str!("../../../kagari-hir/tests/fixtures/editor/static-positions.kgr");
        let initializer = document("untitled:initializer-lease".parse().unwrap(), text, 1);
        let captured = capture(&worker.port, &budget, 4, from_ref(&initializer));
        let input = FeatureInput {
            serial: 7,
            capture: Arc::new(captured),
            documents: Arc::from([initializer.clone()]),
            source_name: initializer.source_name,
            uri: initializer.uri,
            offset: text.find("value + argument").unwrap(),
            limit: 8 * 1024 * 1024,
        };
        state
            .editor_for_input(&input, &CancellationToken::default())
            .unwrap()
    };
    drop(old);
    drop(current);
    drop(both);
    drop(second);
    drop(worker.port);
    worker.thread.join().unwrap();
    worker.terminated.blocking_recv().unwrap().unwrap();
    assert!(
        budget.retained_bytes() > baseline_bytes,
        "analysis cache retains its source lease"
    );
    state.retain_contexts(&BTreeSet::new());
    assert!(
        budget.retained_bytes() > baseline_bytes,
        "held results retain their own charged inputs"
    );
    assert_eq!(
        held.snapshot
            .analysis_at(old_file, SOURCE.find("missing_value").unwrap())
            .unwrap()
            .result()
            .diagnostics()
            .len(),
        1
    );
    assert!(
        other_held
            .snapshot
            .physical_sources()
            .files()
            .all(|source| source.text() == SOURCE)
    );
    assert_eq!(
        state.environment.as_mut().unwrap().baseline_identity(),
        baseline_identity
    );
    drop(held);
    drop(other_held);
    assert!(
        budget.retained_bytes() > baseline_bytes,
        "retired full-analysis initializer query retains its own charged source lease"
    );
    let initializer_text =
        include_str!("../../../kagari-hir/tests/fixtures/editor/static-positions.kgr");
    let offset = initializer_text.find("value + argument").unwrap();
    assert_eq!(initializer_held.editor.source().text(), initializer_text);
    assert_eq!(
        initializer_held.editor.definition_at(offset).unwrap().name,
        "value"
    );
    drop(initializer_held);
    assert_eq!(budget.retained_bytes(), baseline_bytes);
    drop(state);
    assert_eq!(budget.retained_bytes(), 0);

    // Capture admission fits exactly, but a fresh context's independently parsed
    // native sources do not. Failure must drop the database, not cache uncharged data.
    let input = document("untitled:capacity".parse().unwrap(), SOURCE, 1);
    let constrained_budget =
        InputBudget::new(baseline_bytes + input.source.as_ref().unwrap().retained_bytes());
    let mut constrained = AnalysisState::new(root.clone(), constrained_budget.clone(), limits());
    constrained
        .prepare(&store, &CancellationToken::default())
        .unwrap();
    let worker = acquisition::start(root, constrained_budget.clone()).unwrap();
    let captured = capture(&worker.port, &constrained_budget, 1, &[input]);
    let admitted_bytes = constrained_budget.retained_bytes();
    assert!(matches!(
        constrained.snapshot(&captured.contexts[0], &CancellationToken::default()),
        Err(AnalysisFailure::Capacity(_))
    ));
    assert!(constrained.contexts.is_empty());
    assert_eq!(constrained_budget.retained_bytes(), admitted_bytes);
    drop(captured);
    drop(worker.port);
    worker.thread.join().unwrap();
    worker.terminated.blocking_recv().unwrap().unwrap();
    assert_eq!(constrained_budget.retained_bytes(), baseline_bytes);
    drop(constrained);
    assert_eq!(constrained_budget.retained_bytes(), 0);
}

#[test]
fn generated_views_never_replace_installed_contracts_and_changed_views_suspend_dependents() {
    let fixture = Fixture::new();
    let budget = InputBudget::new(256 * 1024 * 1024);
    let root = source_name(&fixture.uri("")).unwrap();
    let worker = acquisition::start(root.clone(), budget.clone()).unwrap();
    let mut state = AnalysisState::new(root, budget.clone(), limits());
    let store = GeneratedDocuments::open(
        &fixture.directory().join("cache"),
        NATIVE_RENDERER_NAMESPACE,
    )
    .unwrap();
    let publications = state
        .prepare(&store, &CancellationToken::default())
        .unwrap();
    for publication in &*publications {
        publication.publish().unwrap();
    }
    let baseline = state
        .environment
        .as_mut()
        .unwrap()
        .signatures(&CancellationToken::default())
        .unwrap();
    let native_file = baseline
        .declaration_snapshot()
        .files()
        .find(|file| {
            file.declarations()
                .iter()
                .any(|declaration| declaration.location.file == file.source().id())
        })
        .unwrap();
    let cursor = native_file
        .declarations()
        .iter()
        .find(|declaration| declaration.location.file == native_file.source().id())
        .unwrap()
        .location
        .range
        .start;
    let generated = publications
        .iter()
        .find(|document| document.source_name() == native_file.source().name())
        .unwrap();
    let alias_directory = fixture.directory().join("native alias 雪 #");
    fixture.link_directory(generated.path().parent().unwrap(), &alias_directory);
    let alias_path = alias_directory.join(generated.path().file_name().unwrap());
    let uri = file_uri(alias_path.to_str().unwrap()).unwrap();
    let exact = document(uri.clone(), generated.text(), 1);
    let original = capture(&worker.port, &budget, 1, from_ref(&exact));
    let clean = state
        .diagnostics(
            &original,
            Some(&original.contexts[0].key),
            from_ref(&exact),
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(clean.issues.is_empty());
    assert_eq!(clean.groups.len(), 1);
    assert_eq!(clean.groups[0].len(), 0);
    assert!(
        state.contexts.is_empty(),
        "generated view does not allocate a script database"
    );
    assert_ne!(exact.source_name, generated.source_name());
    assert_eq!(
        original.document_origins[&exact.source_name],
        generated.source_name()
    );
    let input = FeatureInput {
        serial: 1,
        capture: Arc::new(original.clone()),
        documents: Arc::from([exact.clone()]),
        source_name: exact.source_name.clone(),
        uri: uri.clone(),
        offset: cursor,
        limit: 8 * 1024 * 1024,
    };
    let selected = state
        .editor_for_input(&input, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        selected.editor.source().name(),
        generated.source_name(),
        "alias query uses validated native baseline, not a second script namespace"
    );
    let definition = state
        .definition(&input, &CancellationToken::default())
        .unwrap();
    assert!(
        matches!(definition, Some(GotoDefinitionResponse::Scalar(location)) if location.uri == uri)
    );
    assert!(state.contexts.is_empty());
    let modified = document(uri.clone(), &format!("{}\n", generated.text()), 2);
    let dependent = document("untitled:dependent".parse().unwrap(), SOURCE, 1);
    let documents = [modified, dependent];
    let changed = capture(&worker.port, &budget, 2, &documents);
    let native_issue = state
        .diagnostics(&changed, None, &documents, &CancellationToken::default())
        .unwrap();
    assert_eq!(native_issue.groups.len(), 1);
    assert_eq!(native_issue.groups[0].uri(), &uri);
    assert_eq!(
        native_issue.groups[0].records()[0].diagnostic().code,
        Some(NumberOrString::String("KG_NATIVE_VIEW_MODIFIED".into()))
    );
    let binding = &changed.document_contexts[&documents[1].source_name];
    let suppressed = state
        .diagnostics(
            &changed,
            Some(&binding.context),
            &documents,
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(suppressed.groups.iter().all(|group| group.len() == 0));
    assert_eq!(suppressed.issues.len(), 1);
    assert!(state.contexts.is_empty());
    let mut unavailable = documents[0].clone();
    unavailable.source = None;
    unavailable.unavailable = true;
    let unavailable_result = state
        .diagnostics(
            &changed,
            None,
            &[unavailable],
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(unavailable_result.groups.is_empty());
    assert_eq!(unavailable_result.issues.len(), 1);
    assert!(
        state
            .check_generated_views(&changed, &[document(uri.clone(), generated.text(), 3)])
            .is_ok(),
        "restoring exact bytes restores catalog authority"
    );
    assert!(
        state
            .check_generated_views(&changed, from_ref(&documents[1]))
            .is_ok(),
        "closing modified view restores catalog authority"
    );
    let restored_documents = [
        document(uri.clone(), generated.text(), 3),
        documents[1].clone(),
    ];
    let restored = capture(&worker.port, &budget, 3, &restored_documents);
    state
        .check_generated_views(&restored, &restored_documents)
        .unwrap();
    let binding = &restored.document_contexts[&restored_documents[1].source_name];
    let resumed = state
        .diagnostics(
            &restored,
            Some(&binding.context),
            &restored_documents,
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(resumed.issues.is_empty());
    assert!(
        resumed.groups.iter().any(|group| group
            .records()
            .iter()
            .any(|record| record.diagnostic().code
                == Some(NumberOrString::String("KG_RESOLVE_UNKNOWN_NAME".into())))),
        "restored native alias re-enables real dependent semantic diagnostics"
    );
    let changed_input = FeatureInput {
        capture: Arc::new(changed.clone()),
        documents: Arc::from(documents.clone()),
        ..input.clone()
    };
    assert!(matches!(
        state.hover(&changed_input, &CancellationToken::default()),
        Err(AnalysisFailure::Unavailable(_))
    ));
    state.discard_environment();
    assert!(state.generated_document(generated.source_name()).is_none());
    let failed = state
        .diagnostics(
            &changed,
            Some(&binding.context),
            from_ref(&documents[1]),
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(failed.groups.iter().all(|group| group.len() == 0));
    assert_eq!(failed.issues.len(), 1);
    assert!(failed.issues[0].contains("only syntax diagnostics"));
    assert!(
        state.contexts.is_empty(),
        "failed publication cannot repopulate semantic caches"
    );
    drop(worker.port);
    worker.thread.join().unwrap();
    worker.terminated.blocking_recv().unwrap().unwrap();
}

#[test]
fn typed_worker_keeps_cancelled_completion_and_bounds_queued_admission() {
    let fixture = Fixture::new();
    let worker = worker::start(
        fixture.directory().to_str().unwrap().into(),
        InputBudget::new(1024),
        limits(),
    )
    .unwrap();
    let (entered, entry) = sync_channel(1);
    let (release, released) = sync_channel(1);
    let (first, first_result) = oneshot::channel();
    worker
        .port
        .submit(
            move |_, _| {
                entered.send(()).unwrap();
                released.recv_timeout(Duration::from_secs(5)).unwrap();
                17
            },
            CancellationToken::default(),
            first,
        )
        .unwrap();
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    let (second, second_result) = oneshot::channel();
    worker
        .port
        .submit(|_, cancel| cancel.check(), cancelled, second)
        .unwrap();
    let (third, third_result) = oneshot::channel();
    assert!(matches!(
        worker
            .port
            .submit(|_, _| (), CancellationToken::default(), third),
        Err(worker::SubmitError::Full)
    ));
    assert!(third_result.blocking_recv().is_err());
    release.send(()).unwrap();
    assert_eq!(first_result.blocking_recv().unwrap(), 17);
    assert!(second_result.blocking_recv().unwrap().is_err());
    drop(worker.port);
    worker.thread.join().unwrap();
    worker.terminated.blocking_recv().unwrap().unwrap();
}
