//! Fresh-world semantic oracle over real capture, selected queries and relation indexes.
pub(crate) mod retention;

use crate::{
    analysis::{
        AnalysisFailure, AnalysisState,
        diagnostics::ProjectionLimits,
        environment::prepare,
        features::{FeatureInput, FeatureOutput, QueryStep},
    },
    documents::DocumentInput,
    protocol::source_name,
    workspace::{
        CaptureRequest, WorkspaceCapture,
        acquisition::{self, AcquisitionWorker},
        budget::InputBudget,
    },
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::native::render::NATIVE_RENDERER_NAMESPACE;
use kagari_source::{generated::GeneratedDocuments, source::SourceFile};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeSet, path::Path, process, sync::Arc};
use tokio::sync::oneshot;

const BASE: &str = include_str!("../../tests/fixtures/analysis/fuzz-base.kgr");
const LIMIT: usize = 1024;
const ACTIONS: usize = 3;

thread_local! {
    static WORLDS: RefCell<Option<Worlds>> = const { RefCell::new(None) };
}

struct Worlds {
    reused: AnalysisState,
    fresh: AnalysisState,
    acquisition: AcquisitionWorker,
    budget: InputBudget,
    baseline_bytes: usize,
    generation: u64,
}

#[derive(Default)]
struct Edits {
    boolean: bool,
    wide: bool,
    override_body: bool,
    unresolved: bool,
    provider: bool,
}

#[derive(Debug, PartialEq)]
enum Answer {
    Complete(Value),
    Cancelled,
    Capacity,
    Unavailable,
}

/// Each action mutates a small concrete program, never production validation policy.
pub(crate) fn check(input: &[u8]) {
    if input.len() > LIMIT {
        return;
    }
    WORLDS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let worlds = slot.get_or_insert_with(Worlds::new);
        worlds.run(input);
    });
}

impl Worlds {
    fn new() -> Self {
        let root = env!("CARGO_MANIFEST_DIR").to_owned();
        let budget = InputBudget::new(256 * 1024 * 1024);
        let acquisition = acquisition::start(root.clone(), budget.clone()).unwrap();
        let directory = Path::new(&root)
            .join("../../target/fuzz-analysis")
            .join(process::id().to_string());
        let store = GeneratedDocuments::open(&directory, NATIVE_RENDERER_NAMESPACE).unwrap();
        let limits = ProjectionLimits {
            uris: 1024,
            diagnostics: 16_384,
            bytes: 16 * 1024 * 1024,
        };
        let mut reused = AnalysisState::new(root.clone(), budget.clone(), limits);
        let mut fresh = AnalysisState::new(root, budget.clone(), limits);
        for state in [&mut reused, &mut fresh] {
            // This oracle covers intrinsic types and its source-defined Read trait.
            // The empty closed provider generation uses ordinary validation and leases.
            let environment = prepare(
                &state.source_root,
                &store,
                Vec::new(),
                &CancellationToken::default(),
            )
            .unwrap();
            let lease = budget.retain(environment.sources.iter().cloned()).unwrap();
            for document in environment.publications.iter() {
                document.publish().unwrap();
            }
            state.environment = Some(environment);
            state.environment_lease = Some(lease);
        }
        let baseline_bytes = budget.retained_bytes();
        Self {
            reused,
            fresh,
            acquisition,
            budget,
            baseline_bytes,
            generation: 0,
        }
    }

    fn capture(&mut self, documents: Vec<DocumentInput>) -> Arc<WorkspaceCapture> {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("fuzz generation exhausted");
        let request =
            CaptureRequest::new(self.generation, vec![], documents, &self.budget).unwrap();
        let (sender, receiver) = oneshot::channel();
        self.acquisition
            .port
            .submit(request, CancellationToken::default(), sender)
            .unwrap();
        Arc::new(receiver.blocking_recv().unwrap().unwrap())
    }

    fn reset(state: &mut AnalysisState) {
        state.retain_contexts(&BTreeSet::new());
        state.retain_requests(&BTreeSet::new());
    }

    fn run(&mut self, input: &[u8]) {
        Self::reset(&mut self.reused);
        Self::reset(&mut self.fresh);
        let mut edits = Edits::default();
        let mut retained = Vec::new();
        let mut incarnation = 1u64;
        let mut version = 1i32;
        let initial = self.step(&edits, version, incarnation);
        retained.push(initial);
        for record in input.chunks(2).take(ACTIONS) {
            let value = record.get(1).copied().unwrap_or(1);
            match record[0] % 8 {
                0 => edits.boolean = value & 1 != 0,
                1 => edits.wide = value & 1 != 0,
                2 => edits.override_body = value & 1 != 0,
                3 => {
                    edits.provider = !edits.provider;
                    incarnation += 1;
                }
                4 => {
                    drop(self.capture(vec![]));
                    incarnation += 1;
                    version = 0;
                }
                5 => {
                    let token = CancellationToken::default();
                    token.cancel();
                    let previous = &retained.last().expect("initial capture").0;
                    assert_eq!(
                        answer(self.reused.hover(previous, &token)),
                        Answer::Cancelled
                    );
                    assert_eq!(
                        answer(self.fresh.definition(previous, &token)),
                        Answer::Cancelled
                    );
                }
                6 => edits.unresolved = value & 1 != 0,
                7 => Self::reset(&mut self.reused),
                _ => unreachable!(),
            }
            version += 1;
            let current = self.step(&edits, version, incarnation);
            retained.push(current);
        }
        // An admitted older capture must keep its own answers after newer edits,
        // context retirement or provider replacement; it cannot read current text.
        let old_count = retained.len().saturating_sub(1).max(1);
        let previous = &retained[input.last().copied().unwrap_or(0) as usize % old_count];
        Self::reset(&mut self.fresh);
        assert_eq!(
            queries(&mut self.reused, &previous.0),
            previous.1,
            "retained reused result changed"
        );
        assert_eq!(
            queries(&mut self.fresh, &previous.0),
            previous.1,
            "retained fresh-world result changed"
        );
        drop(retained);
        Self::reset(&mut self.reused);
        Self::reset(&mut self.fresh);
        drop(self.capture(vec![]));
        assert_eq!(
            self.budget.retained_bytes(),
            self.baseline_bytes,
            "source/index/request history leaked after every owner was retired"
        );
    }

    fn step(
        &mut self,
        edits: &Edits,
        version: i32,
        incarnation: u64,
    ) -> (FeatureInput, Vec<Answer>) {
        let mut text = BASE.to_owned();
        if edits.boolean {
            text = text.replace("local = 31", "local = true");
        }
        if edits.wide {
            text = text.replace("scalar() -> i32", "scalar() -> i64");
        }
        if edits.override_body {
            text = text.replace(
                "impl Read for Packet {}",
                "impl Read for Packet { fn read(self) -> i32 { 41 } }",
            );
        }
        if edits.unresolved {
            text = text
                .replace("scalar() -> i32", "scalar() -> Missing")
                .replace("scalar() -> i64", "scalar() -> Missing");
        }
        let uri = if edits.provider {
            "untitled:analysis-other"
        } else {
            "untitled:analysis-primary"
        }
        .parse()
        .unwrap();
        let source_name = source_name(&uri).unwrap();
        let document = DocumentInput {
            source: Some(Arc::new(SourceFile::new(&source_name, &text))),
            source_name: source_name.clone(),
            uri: uri.clone(),
            version,
            incarnation,
            unavailable: false,
        };
        let documents: Arc<[DocumentInput]> = vec![document.clone()].into();
        let capture = self.capture(vec![document]);
        let input = FeatureInput {
            serial: self.generation,
            capture,
            documents,
            source_name,
            uri,
            offset: text.find("local +").unwrap(),
            limit: 1024 * 1024,
        };
        Self::reset(&mut self.fresh);
        let reused = queries(&mut self.reused, &input);
        let fresh = queries(&mut self.fresh, &input);
        assert_eq!(
            reused, fresh,
            "reused and fresh semantic answers disagree for {text}"
        );
        assert!(
            matches!(&reused[0], Answer::Complete(value) if !value.is_null()),
            "known local type must have a hover"
        );
        assert!(
            matches!(&reused[1], Answer::Complete(value) if !value.is_null()),
            "known local binding must have a source definition"
        );
        assert!(
            matches!(&reused[2], Answer::Complete(value) if !value.is_null()),
            "receiver completion must produce a real answer"
        );
        let expected = if edits.boolean { "bool" } else { "i32" };
        assert!(
            matches!(&reused[0], Answer::Complete(value) if value.to_string().contains(expected)),
            "a known literal's local type must match its authored value"
        );
        assert!(
            matches!(&reused[2], Answer::Complete(value) if value["items"].as_array().is_some_and(|items| items.iter().any(|item| item["label"] == "read"))),
            "the checked receiver must retain its Read member"
        );
        if !edits.unresolved {
            assert!(
                matches!(&reused[3], Answer::Complete(Value::Array(locations)) if locations.len() == 1),
                "complete implementation scope must resolve one override or inherited body"
            );
        } else {
            assert_eq!(
                reused[3],
                Answer::Unavailable,
                "unknown signature relations cannot produce exhaustive empty success"
            );
        }
        (input, reused)
    }
}

fn queries(state: &mut AnalysisState, input: &FeatureInput) -> Vec<Answer> {
    state.retain_input_generation(input.capture.generation);
    let live = state
        .contexts
        .keys()
        .cloned()
        .chain(
            input
                .capture
                .contexts
                .iter()
                .map(|context| context.key.clone()),
        )
        .collect();
    state.retain_contexts(&live);
    let token = CancellationToken::default();
    let text = input.documents[0].source.as_ref().unwrap().text();
    let mut result = vec![
        answer(state.hover(input, &token)),
        answer(state.definition(input, &token)),
    ];
    let mut at = input.clone();
    at.offset = text.rfind("value.").unwrap() + "value.".len();
    result.push(answer(state.completion(&at, &token)));
    at.offset = text.find("read(self)").unwrap();
    let mut complete = None;
    for _ in 0..=input.capture.contexts.len() + 1 {
        match state.implementation(&at, &token) {
            Ok(QueryStep::Continue) => {}
            Ok(QueryStep::Complete(output)) => {
                complete = Some(Answer::Complete(output.value));
                break;
            }
            Err(error) => {
                complete = Some(answer(Err(error)));
                break;
            }
        }
    }
    result.push(complete.expect("index did not terminate after its bounded context coverage"));
    at.offset = text.rfind("scalar()").unwrap();
    result.push(answer(state.hover(&at, &token)));
    result
}

fn answer(result: Result<FeatureOutput, AnalysisFailure>) -> Answer {
    match result {
        Ok(output) => Answer::Complete(output.value),
        Err(AnalysisFailure::Cancelled) => Answer::Cancelled,
        Err(AnalysisFailure::Capacity(_)) => Answer::Capacity,
        Err(AnalysisFailure::Unavailable(_)) => Answer::Unavailable,
    }
}
