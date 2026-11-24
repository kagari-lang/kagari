//! Deterministic interruption after work begins, without thread timing or sleeps.

use crate::{
    analysis::{AnalysisSnapshot, eligibility::BodyEligibility, error::AnalysisError},
    declarations::DeclarationId,
    tests::test_analysis,
    typeck::scalar::ScalarValue,
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::IntegerType;
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Checkpoint {
    BodyNodes,
    Constraints,
    TraitCandidates,
    Constants,
    Publish,
}

struct Request {
    point: Checkpoint,
    remaining: usize,
    visits: usize,
    triggered: bool,
    cancel: CancellationToken,
}

thread_local! {
    static REQUEST: RefCell<Option<Request>> = const { RefCell::new(None) };
}

pub(crate) fn checkpoint(point: Checkpoint) {
    REQUEST.with(|request| {
        let mut request = request.borrow_mut();
        if let Some(request) = request.as_mut()
            && request.point == point
        {
            request.visits += 1;
            if !request.triggered {
                request.remaining -= 1;
                if request.remaining == 0 {
                    request.triggered = true;
                    request.cancel.cancel();
                }
            }
        }
    });
}

struct ResetRequest;

impl Drop for ResetRequest {
    fn drop(&mut self) {
        REQUEST.with(|request| *request.borrow_mut() = None);
    }
}

#[test]
fn interrupted_analysis_returns_no_result_and_preserves_readable_snapshots() {
    let text = format!(
        "const START: i32 = 1 + 2; fn id<T>(x: T) -> T {{ x }} fn main() -> i32 {{ var sum = START; {} id(sum) }}",
        "sum += 1; ".repeat(128),
    );
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("cancel.kgr", text.clone(), SourceLayer::Base)
        .unwrap();
    let original = sources.snapshot();
    let mut db = test_analysis();
    let old = db.snapshot(original, &Default::default()).unwrap();
    assert!(old.file(file).unwrap().result().diagnostics().is_empty());
    let check_start = |snapshot: &AnalysisSnapshot, value| {
        let facts = snapshot.file(file).unwrap().result().facts();
        let start = facts
            .lowered
            .module
            .consts
            .iter()
            .find(|item| item.name == "START")
            .unwrap();
        assert_eq!(
            facts.typed.const_values().get(&start.id),
            Some(&ScalarValue::integer(value, IntegerType::I32).unwrap())
        );
    };
    sources
        .set(
            "cancel.kgr",
            text.replace("1 + 2", "1 + 3"),
            SourceLayer::Overlay,
        )
        .unwrap();

    for point in [
        Checkpoint::BodyNodes,
        Checkpoint::Constraints,
        Checkpoint::TraitCandidates,
        Checkpoint::Constants,
        Checkpoint::Publish,
    ] {
        let cancel = CancellationToken::default();
        let _reset = ResetRequest;
        REQUEST.with(|request| {
            *request.borrow_mut() = Some(Request {
                point,
                remaining: if point == Checkpoint::Publish { 1 } else { 2 },
                visits: 0,
                triggered: false,
                cancel: cancel.clone(),
            })
        });
        let result = db.snapshot(sources.snapshot(), &cancel);
        let request = REQUEST.with(|request| request.borrow_mut().take().unwrap());
        assert!(
            request.triggered,
            "{point:?} never reached its work checkpoint"
        );
        assert!(
            matches!(result, Err(AnalysisError::Cancelled)),
            "{point:?}: {result:?}"
        );
        if point == Checkpoint::BodyNodes {
            assert!(
                request.visits <= 65,
                "node batch failed to stop: {}",
                request.visits
            );
        }
        check_start(&old, 3);
    }
    let fresh = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(fresh.file(file).unwrap().result().diagnostics().is_empty());
    check_start(&fresh, 4);
    assert_ne!(
        old.file(file).unwrap().source().revision(),
        fresh.file(file).unwrap().source().revision()
    );
    let DeclarationId::Definition(owner) = fresh
        .file(file)
        .unwrap()
        .result()
        .facts()
        .declarations
        .iter()
        .find(|declaration| declaration.name == "main")
        .unwrap()
        .id
    else {
        panic!("function definition")
    };
    let previous_body = db
        .body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap();
    sources
        .set(
            "cancel.kgr",
            text.replace("1 + 2", "1 + 4"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let cancel = CancellationToken::default();
    let _reset = ResetRequest;
    REQUEST.with(|request| {
        *request.borrow_mut() = Some(Request {
            point: Checkpoint::Publish,
            remaining: 1,
            visits: 0,
            triggered: false,
            cancel: cancel.clone(),
        })
    });
    let result = db.body(sources.snapshot(), &owner, &cancel);
    assert!(REQUEST.with(|request| request.borrow_mut().take().unwrap().triggered));
    assert!(matches!(result, Err(AnalysisError::Cancelled)));
    let current = db
        .body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(
        previous_body.source().text(),
        text.replace("1 + 2", "1 + 3")
    );
    assert_eq!(current.source().text(), text.replace("1 + 2", "1 + 4"));
    for body in [&previous_body, &current] {
        assert_eq!(body.eligibility(), BodyEligibility::Complete);
        assert!(body.diagnostics().is_empty());
    }
}
