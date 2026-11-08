//! B03/B04/B07 component samples with exact provider scope and retirement oracles.
use crate::{
    ScopeCheckpoint,
    analysis::{
        AnalysisState,
        diagnostics::ProjectionLimits,
        features::{DocumentQueryInput, navigation::implementation::ImplementationStep},
        verification::project,
    },
    documents::DocumentInput,
    protocol::source_name,
    workspace::{
        CaptureRequest, WorkspaceCapture,
        acquisition::{self, AcquisitionWorker},
        budget::InputBudget,
        origins::file_uri,
    },
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::native::render::NATIVE_RENDERER_NAMESPACE;
use kagari_source::{generated::GeneratedDocuments, source::SourceFile};
use lsp_types::{GotoDefinitionResponse, Location, Uri};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process,
    sync::Arc,
    time::Instant,
};
use tokio::sync::oneshot;

const API: &str = include_str!("../../../tests/fixtures/analysis/retention-api.kgr");
const APP: &str = include_str!("../../../tests/fixtures/analysis/retention-app.kgr");

struct World {
    projects: Vec<PathBuf>,
    worker: AcquisitionWorker,
    state: AnalysisState,
    budget: InputBudget,
    baseline: usize,
    generation: u64,
    serial: u64,
    sample: usize,
}

pub(crate) fn run(mut checkpoint: impl FnMut(ScopeCheckpoint)) {
    for unrelated in [0, 7, 31] {
        let mut world = World::new(unrelated);
        for sample in 0..3 {
            world.sample = sample;
            let (original, acquisition) = world.input(API);
            let (row, expected) = world.measure("cold", &original, acquisition);
            assert_eq!(row.indexed_contexts, 2);
            checkpoint(row);
            let (row, answer) = world.measure("warm", &original, 0);
            assert_eq!(answer, expected);
            assert_eq!(row.indexed_contexts, 0);
            checkpoint(row);

            let phase = if let Some(independent) = world.projects.get(1) {
                fs::write(
                    independent.join("app/src/lib.kgr"),
                    APP.replace("23", &(sample + 70).to_string()),
                )
                .unwrap();
                "unrelated-edit"
            } else {
                "same-input-capture"
            };
            let (unrelated_input, acquisition) = world.input(API);
            let (row, answer) = world.measure(phase, &unrelated_input, acquisition);
            assert_eq!(answer, expected);
            assert_eq!(row.indexed_contexts, 0);
            checkpoint(row);

            let provider = format!("// selected provider edit {sample}\n{API}");
            let (changed, acquisition) = world.input(&provider);
            let (row, answer) = world.measure("provider-edit", &changed, acquisition);
            assert_eq!(answer, expected);
            assert_eq!(row.indexed_contexts, 2);
            checkpoint(row);
            let (row, answer) = world.measure("retained-old", &original, 0);
            assert_eq!(answer, expected);
            checkpoint(row);

            drop(changed);
            drop(unrelated_input);
            world.clear();
            assert!(world.budget.retained_bytes() > world.baseline);
            assert_eq!(world.state.indexes.retained_bytes(), 0);
            checkpoint(world.retention("held-only"));
            drop(original);
            assert_eq!(world.budget.retained_bytes(), world.baseline);
            checkpoint(world.retention("released"));
        }
        drop(world.worker.port);
        world.worker.thread.join().unwrap();
        world.worker.terminated.blocking_recv().unwrap().unwrap();
    }
}

impl World {
    fn new(unrelated: usize) -> Self {
        let root = env!("CARGO_MANIFEST_DIR").to_owned();
        let directory = Path::new(&root)
            .join("../../target/lsp-scope")
            .join(process::id().to_string())
            .join(unrelated.to_string());
        let projects = (0..=unrelated)
            .map(|index| project::create(&directory.join(index.to_string()), APP, API))
            .collect();
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
        state.prepare(&store, &Default::default()).unwrap();
        let baseline = budget.retained_bytes();
        Self {
            projects,
            worker,
            state,
            budget,
            baseline,
            generation: 0,
            serial: 0,
            sample: 0,
        }
    }

    fn capture(&mut self, roots: Vec<Uri>, documents: Vec<DocumentInput>) -> Arc<WorkspaceCapture> {
        self.generation += 1;
        let (sender, receiver) = oneshot::channel();
        self.worker
            .port
            .submit(
                CaptureRequest::new(self.generation, roots, documents, &self.budget).unwrap(),
                CancellationToken::default(),
                sender,
            )
            .unwrap();
        Arc::new(receiver.blocking_recv().unwrap().unwrap())
    }

    fn input(&mut self, api: &str) -> (DocumentQueryInput, u128) {
        let uri = file_uri(self.projects[0].join("api/src/lib.kgr").to_str().unwrap()).unwrap();
        let source_name = source_name(&uri).unwrap();
        let document = DocumentInput {
            source: Some(Arc::new(SourceFile::new(&source_name, api))),
            source_name: source_name.clone(),
            uri: uri.clone(),
            version: self.generation as i32 + 1,
            incarnation: 1,
            unavailable: false,
        };
        let roots = self
            .projects
            .iter()
            .map(|directory| file_uri(directory.to_str().unwrap()).unwrap())
            .collect();
        let started = Instant::now();
        let capture = self.capture(roots, vec![document.clone()]);
        let acquisition = started.elapsed().as_nanos();
        assert_eq!(capture.contexts.len(), self.projects.len() * 2);
        assert!(capture.errors.is_empty());
        self.state.retain_contexts(
            &capture
                .contexts
                .iter()
                .map(|context| context.key.clone())
                .collect(),
        );
        self.state.retain_capture_inputs(&capture);
        (
            DocumentQueryInput {
                serial: 0,
                capture,
                documents: vec![document].into(),
                source_name,
                uri,
                offset: api.find("Read").unwrap(),
                limit: 1024 * 1024,
            },
            acquisition,
        )
    }

    fn measure(
        &mut self,
        phase: &'static str,
        input: &DocumentQueryInput,
        acquisition: u128,
    ) -> (ScopeCheckpoint, Vec<Location>) {
        self.serial += 1;
        let mut input = input.clone();
        input.serial = self.serial;
        let before = self.state.implementation.work;
        let started = Instant::now();
        for _ in 0..=input.capture.contexts.len() {
            if let ImplementationStep::Complete(Some(GotoDefinitionResponse::Array(locations))) =
                self.state
                    .implementation(&input, &Default::default())
                    .unwrap()
            {
                let query_ns = started.elapsed().as_nanos();
                assert_eq!(locations.len(), 1);
                assert_eq!(
                    locations[0].uri,
                    file_uri(self.projects[0].join("app/src/lib.kgr").to_str().unwrap()).unwrap()
                );
                let after = self.state.implementation.work;
                let mut row = self.retention(phase);
                row.capture_ns = acquisition;
                row.query_ns = query_ns;
                row.scope_selection_ns = after.selection_ns - before.selection_ns;
                row.examined_contexts = after.candidates - before.candidates;
                row.selected_contexts = after.selected - before.selected;
                row.indexed_contexts = after.indexed - before.indexed;
                assert_eq!(row.examined_contexts, input.capture.contexts.len());
                assert_eq!(row.selected_contexts, 2);
                return (row, locations);
            }
        }
        panic!("scope search exceeded its bounded context turns")
    }

    fn clear(&mut self) {
        let capture = self.capture(vec![], vec![]);
        self.state.retain_requests(&BTreeSet::new());
        self.state.retain_contexts(&BTreeSet::new());
        self.state.retain_capture_inputs(&capture);
    }

    fn retention(&self, phase: &'static str) -> ScopeCheckpoint {
        ScopeCheckpoint {
            phase,
            sample: self.sample,
            unrelated_projects: self.projects.len() - 1,
            capture_ns: 0,
            query_ns: 0,
            scope_selection_ns: 0,
            examined_contexts: 0,
            selected_contexts: 0,
            indexed_contexts: 0,
            source_input_bytes: self.budget.retained_bytes(),
            index_row_bytes: self.state.indexes.retained_bytes(),
        }
    }
}
