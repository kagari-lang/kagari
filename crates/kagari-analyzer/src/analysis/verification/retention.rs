//! B07 source/index/identity ownership probe; allocation and RSS sampling are external.

use crate::{
    RetentionCheckpoint,
    analysis::{
        AnalysisState,
        diagnostics::ProjectionLimits,
        features::{FeatureInput, QueryStep},
    },
    documents::DocumentInput,
    protocol::source_name,
    workspace::{
        CaptureRequest, WorkspaceCapture,
        acquisition::{self, AcquisitionPort},
        budget::InputBudget,
        origins::file_uri,
    },
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::native::render::NATIVE_RENDERER_NAMESPACE;
use kagari_source::{generated::GeneratedDocuments, source::SourceFile};
use lsp_types::Uri;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process,
    sync::Arc,
};
use tokio::sync::oneshot;

const APP: &str = include_str!("../../../tests/fixtures/analysis/retention-app.kgr");
const API: &str = include_str!("../../../tests/fixtures/analysis/retention-api.kgr");

struct EditInput<'a> {
    provider: &'a Path,
    app: &'a str,
    api: &'a str,
    version: i32,
    incarnation: u64,
    body: usize,
}

pub(crate) fn run(mut checkpoint: impl FnMut(RetentionCheckpoint)) {
    let root = env!("CARGO_MANIFEST_DIR").to_owned();
    let directory = Path::new(&root)
        .join("../../target/lsp-retention")
        .join(process::id().to_string());
    let providers = [
        project(&directory.join("first")),
        project(&directory.join("replacement")),
    ];
    let budget = InputBudget::new(256 * 1024 * 1024);
    let worker = acquisition::start(root.clone(), budget.clone()).unwrap();
    let mut state = AnalysisState::new(
        root,
        budget.clone(),
        ProjectionLimits {
            uris: 1024,
            diagnostics: 16_384,
            bytes: 16 * 1024 * 1024,
        },
    );
    let store =
        GeneratedDocuments::open(&directory.join("native"), NATIVE_RENDERER_NAMESPACE).unwrap();
    for document in state.prepare(&store, &Default::default()).unwrap().iter() {
        document.publish().unwrap();
    }
    let baseline = budget.retained_bytes();
    checkpoint(sample(&state, &budget, "baseline", 0));
    let mut generation = 0;
    let mut incarnation = 1;
    let mut values = [10101, 20202, 30303, 40404];
    let initial = input(
        &worker.port,
        &budget,
        &mut generation,
        EditInput {
            provider: &providers[0],
            app: APP,
            api: API,
            version: 1,
            incarnation,
            body: 0,
        },
    );
    let held = state
        .editor_for_input(&initial, &Default::default())
        .unwrap();
    let held_type = held.editor.type_at(initial.offset).unwrap();
    let held_declaration = held.editor.definition_at(initial.offset).unwrap().clone();
    let mut retained_initial = Some(initial);
    let mut sample_500 = 0;
    for edit in 1..=1000 {
        if edit % 125 == 0 {
            drop(capture(
                &worker.port,
                &budget,
                &mut generation,
                vec![],
                vec![],
            ));
            state.retain_contexts(&BTreeSet::new());
            incarnation += 1;
        }
        let body = (edit / 100) % values.len();
        values[body] = edit as i32;
        let mut text = APP.to_owned();
        for (marker, value) in ["10101", "20202", "30303", "40404"].into_iter().zip(values) {
            text = text.replace(marker, &value.to_string());
        }
        text = text.replace("fn renamed()", &format!("fn renamed_{edit}()"));
        let api = if (edit / 17) % 2 == 1 {
            API.replace("scalar() -> i32", "scalar() -> i64")
        } else {
            API.to_owned()
        };
        let provider = (edit / 250) % providers.len();
        let input = input(
            &worker.port,
            &budget,
            &mut generation,
            EditInput {
                provider: &providers[provider],
                app: &text,
                api: &api,
                version: edit as i32 + 1,
                incarnation,
                body,
            },
        );
        let live = input
            .capture
            .contexts
            .iter()
            .map(|context| context.key.clone())
            .collect();
        state.retain_input_generation(input.capture.generation);
        state.retain_contexts(&live);
        let answer = state.hover(&input, &Default::default()).unwrap();
        assert!(!answer.value.is_null());
        if edit % 64 == 0 || matches!(edit, 100 | 500 | 1000) {
            let mut search = input.clone();
            search.offset = text.find("api::Read").unwrap() + "api::".len();
            let mut complete = false;
            for _ in 0..=input.capture.contexts.len() + 1 {
                match state.implementation(&search, &Default::default()).unwrap() {
                    QueryStep::Continue => {}
                    QueryStep::Complete(result) => {
                        assert_eq!(result.value.as_array().unwrap().len(), 1);
                        complete = true;
                        break;
                    }
                }
            }
            assert!(complete);
        }
        assert_eq!(
            held.editor
                .type_at(retained_initial.as_ref().unwrap().offset),
            Some(held_type.clone())
        );
        assert_eq!(
            held.editor
                .definition_at(retained_initial.as_ref().unwrap().offset),
            Some(&held_declaration)
        );
        if matches!(edit, 100 | 500 | 1000) {
            let current = sample(&state, &budget, "edits", edit);
            if edit == 500 {
                sample_500 = current.input_bytes;
            }
            if edit == 1000 {
                assert!(
                    current.input_bytes <= sample_500 + 128 * 1024,
                    "source retention must plateau across repeated context/provider cycles"
                );
            }
            checkpoint(current);
        }
    }
    state.retain_contexts(&BTreeSet::new());
    state.retain_requests(&BTreeSet::new());
    drop(capture(
        &worker.port,
        &budget,
        &mut generation,
        vec![],
        vec![],
    ));
    assert!(
        budget.retained_bytes() > baseline,
        "explicit old query still owns charged sources"
    );
    checkpoint(sample(&state, &budget, "held_only", 1000));
    drop(retained_initial.take());
    drop(held);
    assert_eq!(
        budget.retained_bytes(),
        baseline,
        "all request/capture/index leases must retire"
    );
    checkpoint(sample(&state, &budget, "released", 1000));
    drop(worker.port);
    worker.thread.join().unwrap();
    worker.terminated.blocking_recv().unwrap().unwrap();
}

fn sample(
    state: &AnalysisState,
    budget: &InputBudget,
    phase: &'static str,
    edits: usize,
) -> RetentionCheckpoint {
    RetentionCheckpoint {
        phase,
        edits,
        input_bytes: budget.retained_bytes(),
        index_bytes: state.indexes.retained_bytes(),
        definition_records: state
            .contexts
            .values()
            .map(|context| context.database.definition_count())
            .sum(),
        contexts: state.contexts.len(),
    }
}

fn project(directory: &Path) -> PathBuf {
    for (name, text) in [
        (
            "Kagari.toml",
            include_str!("../../../tests/fixtures/workspace/workspace.toml"),
        ),
        (
            "app/Kagari.toml",
            include_str!("../../../tests/fixtures/workspace/app.toml"),
        ),
        (
            "api/Kagari.toml",
            include_str!("../../../tests/fixtures/workspace/api.toml"),
        ),
        ("app/src/lib.kgr", APP),
        ("api/src/lib.kgr", API),
    ] {
        let path = directory.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    directory.to_owned()
}

fn input(
    port: &AcquisitionPort,
    budget: &InputBudget,
    generation: &mut u64,
    edit: EditInput<'_>,
) -> FeatureInput {
    let EditInput {
        provider,
        app,
        api,
        version,
        incarnation,
        body,
    } = edit;
    let uri = file_uri(provider.join("app/src/lib.kgr").to_str().unwrap()).unwrap();
    let document = |uri: Uri, text: &str| {
        let source_name = source_name(&uri).unwrap();
        DocumentInput {
            source: Some(Arc::new(SourceFile::new(&source_name, text))),
            source_name,
            uri,
            version,
            incarnation,
            unavailable: false,
        }
    };
    let documents = vec![
        document(uri.clone(), app),
        document(
            file_uri(provider.join("api/src/lib.kgr").to_str().unwrap()).unwrap(),
            api,
        ),
    ];
    let capture = capture(
        port,
        budget,
        generation,
        vec![file_uri(provider.to_str().unwrap()).unwrap()],
        documents.clone(),
    );
    let name = ["first", "second", "third", "fourth"][body];
    let start = app.find(&format!("fn {name}()")).unwrap();
    let offset = start + app[start..].find("    local\n").unwrap() + 4;
    FeatureInput {
        serial: *generation,
        capture,
        documents: documents.into(),
        source_name: source_name(&uri).unwrap(),
        uri,
        offset,
        limit: 1024 * 1024,
    }
}

fn capture(
    port: &AcquisitionPort,
    budget: &InputBudget,
    generation: &mut u64,
    roots: Vec<Uri>,
    documents: Vec<DocumentInput>,
) -> Arc<WorkspaceCapture> {
    *generation += 1;
    let (sender, receiver) = oneshot::channel();
    port.submit(
        CaptureRequest::new(*generation, roots, documents, budget).unwrap(),
        CancellationToken::default(),
        sender,
    )
    .unwrap();
    Arc::new(receiver.blocking_recv().unwrap().unwrap())
}
