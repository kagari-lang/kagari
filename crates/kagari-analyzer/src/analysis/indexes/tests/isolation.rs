//! Multi-project coverage, cache validity and retained-read integration witnesses.
use crate::{
    analysis::{
        AnalysisFailure, AnalysisState,
        diagnostics::ProjectionLimits,
        features::{DocumentQueryInput, navigation::implementation::ImplementationStep},
    },
    documents::DocumentInput,
    protocol::source_name,
    tests::fixture::Fixture,
    workspace::{
        CaptureRequest,
        acquisition::{self, AcquisitionWorker},
        budget::InputBudget,
    },
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::{publication::SourceStore, source::SourceFile};
use lsp_types::{GotoDefinitionResponse, Location};
use std::{collections::BTreeSet, fs, sync::Arc};
use tokio::sync::oneshot;

const API: &str = include_str!("../../../../tests/fixtures/analysis/index-api.kgr");
const APP: &str = include_str!("../../../../tests/fixtures/analysis/index-app.kgr");

struct World {
    project: Fixture,
    independent: Fixture,
    worker: AcquisitionWorker,
    budget: InputBudget,
    state: AnalysisState,
    generation: u64,
}

impl World {
    fn new() -> Self {
        let project = Fixture::new();
        let independent = Fixture::new();
        for fixture in [&project, &independent] {
            fs::write(fixture.directory().join("first/api/src/lib.kgr"), API).unwrap();
            fs::write(fixture.directory().join("first/app/src/lib.kgr"), APP).unwrap();
        }
        let root = source_name(&project.uri("first")).unwrap();
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
        let store = SourceStore::open(&project.directory().join("cache")).unwrap();
        state.prepare(&store, &Default::default()).unwrap();
        Self {
            project,
            independent,
            worker,
            budget,
            state,
            generation: 0,
        }
    }

    fn finish(self) {
        drop(self.worker.port);
        self.worker.thread.join().unwrap();
        self.worker.terminated.blocking_recv().unwrap().unwrap();
    }

    fn input(&mut self) -> DocumentQueryInput {
        self.generation += 1;
        let uri = self.project.uri("first/api/src/lib.kgr");
        let source_name = source_name(&uri).unwrap();
        let text =
            fs::read_to_string(self.project.directory().join("first/api/src/lib.kgr")).unwrap();
        let document = DocumentInput {
            source: Some(Arc::new(SourceFile::new(&source_name, &text))),
            source_name: source_name.clone(),
            uri: uri.clone(),
            version: self.generation as i32,
            incarnation: 1,
            unavailable: false,
        };
        let request = CaptureRequest::new(
            self.generation,
            vec![self.project.uri("first"), self.independent.uri("first")],
            vec![document.clone()],
            &self.budget,
        )
        .unwrap();
        let (sender, receiver) = oneshot::channel();
        self.worker
            .port
            .submit(request, CancellationToken::default(), sender)
            .unwrap();
        let capture = Arc::new(receiver.blocking_recv().unwrap().unwrap());
        self.state.retain_capture_inputs(&capture);
        self.state.retain_contexts(
            &capture
                .contexts
                .iter()
                .map(|context| context.key.clone())
                .collect(),
        );
        DocumentQueryInput {
            serial: self.generation,
            capture,
            documents: vec![document].into(),
            source_name,
            uri,
            offset: text.find("Read").unwrap(),
            limit: 1024 * 1024,
        }
    }

    fn search(
        &mut self,
        input: &DocumentQueryInput,
    ) -> Result<(Vec<Location>, usize), AnalysisFailure> {
        for steps in 1..=input.capture.contexts.len() + 1 {
            if let ImplementationStep::Complete(Some(GotoDefinitionResponse::Array(locations))) =
                self.state.implementation(input, &Default::default())?
            {
                return Ok((locations, steps));
            }
        }
        panic!("implementation did not finish within its context bound")
    }
}

#[test]
fn unrelated_broken_provider_does_not_poison_search_or_evict_unchanged_scope() {
    let mut world = World::new();
    fs::write(
        world.independent.directory().join("first/app/src/lib.kgr"),
        format!("mod missing;\n{APP}"),
    )
    .unwrap();
    let initial = world.input();
    assert!(
        initial
            .capture
            .contexts
            .iter()
            .any(|context| !context.discovery_complete)
    );
    let (expected, cold_steps) = world.search(&initial).unwrap();
    assert_eq!(expected.len(), 1);
    assert_eq!(expected[0].uri, world.project.uri("first/app/src/lib.kgr"));
    assert_eq!(
        cold_steps, 3,
        "only the two required contexts need index turns"
    );
    assert_eq!(world.search(&initial).unwrap(), (expected.clone(), 1));
    let before = world.state.indexes.retained_bytes();
    assert!(before > 0);

    fs::write(
        world.independent.directory().join("first/app/src/lib.kgr"),
        APP.replace("23", "37"),
    )
    .unwrap();
    let changed = world.input();
    let selected_workspace = &initial.capture.document_contexts[&initial.source_name]
        .context
        .workspace;
    for old in &initial.capture.contexts {
        let new = changed
            .capture
            .contexts
            .iter()
            .find(|new| new.key == old.key)
            .unwrap();
        if &old.key.workspace == selected_workspace {
            assert_eq!(old.input_revision, new.input_revision);
        } else if !old.discovery_complete {
            assert!(new.input_revision > old.input_revision);
        }
    }
    assert_eq!(world.state.indexes.retained_bytes(), before);
    assert_eq!(world.search(&changed).unwrap(), (expected, 1));

    // Without a resolved package graph, directory separation cannot prove that
    // this project is not a reverse consumer of the queried provider.
    fs::write(
        world.independent.directory().join("first/app/Kagari.toml"),
        "invalid manifest",
    )
    .unwrap();
    let unknown = world.input();
    assert!(
        matches!(world.search(&unknown), Err(AnalysisFailure::Unavailable(message)) if message.contains("discovery coverage"))
    );
    world.finish();
}

#[test]
fn required_consumer_gap_and_shared_provider_edits_preserve_exact_old_reads() {
    let mut world = World::new();
    let old = world.input();
    assert!(matches!(
        world
            .state
            .implementation(&old, &Default::default())
            .unwrap(),
        ImplementationStep::Continue
    ));
    fs::write(
        world.project.directory().join("first/app/src/lib.kgr"),
        format!("mod missing;\n{APP}"),
    )
    .unwrap();
    let missing = world.input();
    assert!(
        matches!(world.search(&missing), Err(AnalysisFailure::Unavailable(message)) if message.contains("unavailable declared modules"))
    );
    let (old_answer, _) = world.search(&old).unwrap();
    assert_eq!(old_answer.len(), 1);
    assert_eq!(
        old_answer[0].uri,
        world.project.uri("first/app/src/lib.kgr")
    );
    assert_eq!(world.state.index_cursor_count(), 0);

    fs::remove_file(world.project.directory().join("first/app/src/lib.kgr")).unwrap();
    let absent = world.input();
    assert!(
        matches!(world.search(&absent), Err(AnalysisFailure::Unavailable(message)) if message.contains("unavailable declared modules")),
        "a consumer with no root source still belongs to the selected provider's scope"
    );

    fs::write(world.project.directory().join("first/app/src/lib.kgr"), APP).unwrap();
    // A line inserted into the shared provider changes every consumer's input,
    // even though the app file and its implementation result are unchanged.
    fs::write(
        world.project.directory().join("first/api/src/lib.kgr"),
        format!("// changed provider\n{API}"),
    )
    .unwrap();
    let changed = world.input();
    for context in &old.capture.contexts {
        let next = changed
            .capture
            .contexts
            .iter()
            .find(|next| next.key == context.key)
            .unwrap();
        if context.key.workspace
            == old.capture.document_contexts[&old.source_name]
                .context
                .workspace
        {
            assert!(next.input_revision > context.input_revision);
        } else {
            assert_eq!(next.input_revision, context.input_revision);
        }
    }
    assert_eq!(world.search(&changed).unwrap(), (old_answer.clone(), 3));
    assert_eq!(world.search(&changed).unwrap(), (old_answer.clone(), 1));
    assert_eq!(world.search(&old).unwrap().0, old_answer);

    // New membership changes the required scope even if all existing contexts
    // keep the same inputs. A formerly warm scope must not hide a new implementor.
    fs::write(
        world.project.directory().join("first/Kagari.toml"),
        include_str!("../../../../tests/fixtures/workspace/workspace.toml")
            .replace("\"app\", \"api\"", "\"app\", \"api\", \"consumer\""),
    )
    .unwrap();
    let consumer = world.project.directory().join("first/consumer");
    fs::create_dir_all(consumer.join("src")).unwrap();
    fs::write(
        consumer.join("Kagari.toml"),
        include_str!("../../../../tests/fixtures/workspace/app.toml")
            .replace("name = \"app\"", "name = \"consumer\""),
    )
    .unwrap();
    fs::write(consumer.join("src/lib.kgr"), APP).unwrap();
    let expanded = world.input();
    for context in &changed.capture.contexts {
        assert_eq!(
            expanded
                .capture
                .contexts
                .iter()
                .find(|next| next.key == context.key)
                .unwrap()
                .input_revision,
            context.input_revision
        );
    }
    let (locations, steps) = world.search(&expanded).unwrap();
    assert_eq!(steps, 4);
    assert_eq!(locations.len(), 2);
    assert_eq!(
        locations
            .into_iter()
            .map(|location| location.uri.as_str().to_owned())
            .collect::<BTreeSet<_>>(),
        [
            world
                .project
                .uri("first/app/src/lib.kgr")
                .as_str()
                .to_owned(),
            world
                .project
                .uri("first/consumer/src/lib.kgr")
                .as_str()
                .to_owned()
        ]
        .into()
    );
    world.finish();
}
