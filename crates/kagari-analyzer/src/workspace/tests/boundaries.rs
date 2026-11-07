use crate::{
    protocol::source_name,
    tests::fixture::Fixture,
    workspace::{
        CaptureRequest, MAX_INPUT_BYTES, budget::InputBudget, capture::CaptureState,
        tests::document_at,
    },
};
use kagari_common::cancellation::CancellationToken;
use std::{env, fs};

const API: &str = include_str!("../../../tests/fixtures/workspace/api.kgr");
const OTHER: &str = include_str!("../../../tests/fixtures/workspace/other.kgr");
const APP: &str = include_str!("../../../tests/fixtures/workspace/app.kgr");
const APP_MANIFEST: &str = include_str!("../../../tests/fixtures/workspace/app.toml");

fn state(budget: &InputBudget) -> CaptureState {
    CaptureState::new(
        env::current_dir().unwrap().to_str().unwrap().into(),
        budget.clone(),
    )
}

#[test]
fn nested_packages_are_discovered_only_from_open_files_and_never_widen_approved_roots() {
    let fixture = Fixture::new();
    let budget = InputBudget::new(MAX_INPUT_BYTES);
    let mut state = state(&budget);
    let cancel = CancellationToken::default();
    let empty = state
        .capture(
            CaptureRequest::new(1, vec![fixture.uri("")], Vec::new(), &budget).unwrap(),
            &cancel,
        )
        .unwrap();
    assert!(
        empty.contexts.is_empty(),
        "a folder without a root manifest must not be crawled"
    );
    let opened = state
        .capture(
            CaptureRequest::new(
                2,
                vec![fixture.uri("")],
                vec![document_at(fixture.uri("second/src/lib.kgr"), OTHER, 1)],
                &budget,
            )
            .unwrap(),
            &cancel,
        )
        .unwrap();
    assert!(opened.errors.is_empty());
    assert_eq!(opened.contexts.len(), 1);
    assert!(opened.contexts[0].complete);
    assert_eq!(opened.contexts[0].sources.files().count(), 1);
    assert_eq!(
        opened.contexts[0].sources.files().next().unwrap().text(),
        OTHER
    );

    // Supply a valid local namespace so the rejected ../api edge reaches the
    // approved-root boundary, not an unrelated missing-namespace diagnostic.
    let manifest = APP_MANIFEST.replace(
        "version = \"1.0.0\"",
        "version = \"1.0.0\"\nnamespace = \"analyzer/escape\"",
    );
    fs::write(fixture.directory().join("first/app/Kagari.toml"), manifest).unwrap();
    let confined = state
        .capture(
            CaptureRequest::new(
                3,
                vec![fixture.uri("first/app")],
                vec![document_at(fixture.uri("first/app/src/lib.kgr"), APP, 1)],
                &budget,
            )
            .unwrap(),
            &cancel,
        )
        .unwrap();
    assert!(
        confined
            .errors
            .iter()
            .any(|issue| issue.message.contains("escapes the input root")),
        "{:?}",
        confined.errors
    );
    assert_eq!(confined.contexts.len(), 1);
    assert!(!confined.contexts[0].semantic_available);
    assert_eq!(
        confined.contexts[0].sources.files().next().unwrap().text(),
        APP
    );
    assert!(
        confined.contexts[0]
            .origins
            .values()
            .all(|origin| origin.uri.as_ref() == Some(&fixture.uri("first/app/src/lib.kgr")))
    );
}

#[test]
fn conflicting_physical_aliases_withhold_semantics_until_the_buffers_agree() {
    let fixture = Fixture::new();
    fixture.link_directory(
        &fixture.directory().join("first/api"),
        &fixture.directory().join("outside_api_alias"),
    );
    let budget = InputBudget::new(MAX_INPUT_BYTES);
    let mut state = state(&budget);
    let original = fixture.uri("first/api/src/lib.kgr");
    let alias = fixture.uri("outside_api_alias/src/lib.kgr");
    let changed = API.replace('1', "2");
    let roots = vec![fixture.uri("first")];
    // An admitted buffer remains authoritative when its lexical alias is outside
    // the configured root but its canonical file is a member of that root.
    let outside = state
        .capture(
            CaptureRequest::new(
                1,
                roots.clone(),
                vec![document_at(alias.clone(), &changed, 1)],
                &budget,
            )
            .unwrap(),
            &CancellationToken::default(),
        )
        .unwrap();
    let physical = fixture
        .directory()
        .join("first/api/src/lib.kgr")
        .canonicalize()
        .unwrap();
    let physical = physical.to_str().unwrap();
    let mut projections = 0;
    for context in &outside.contexts {
        for (file, origin) in &context.origins {
            if origin.provider_key == physical {
                projections += 1;
                assert_eq!(
                    context.sources.file(*file).unwrap().text(),
                    changed,
                    "every canonical projection must use the admitted outside-alias overlay"
                );
            }
        }
    }
    assert!(
        projections >= 2,
        "API root and dependent application both contain the source"
    );
    let conflicted = state
        .capture(
            CaptureRequest::new(
                2,
                roots.clone(),
                vec![
                    document_at(original.clone(), API, 1),
                    document_at(alias.clone(), &changed, 1),
                ],
                &budget,
            )
            .unwrap(),
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(conflicted.contexts.iter().any(|context| {
        context
            .errors
            .iter()
            .any(|issue| issue.message.contains("conflicting source text"))
    }));
    assert!(
        conflicted
            .contexts
            .iter()
            .filter(|context| context
                .errors
                .iter()
                .any(|issue| issue.message.contains("conflicting source text")))
            .all(|context| !context.semantic_available)
    );
    assert!(
        !conflicted
            .document_contexts
            .contains_key(&source_name(&original).unwrap())
    );
    assert!(
        !conflicted
            .document_contexts
            .contains_key(&source_name(&alias).unwrap())
    );

    let resolved = state
        .capture(
            CaptureRequest::new(
                3,
                roots,
                vec![
                    document_at(original.clone(), API, 2),
                    document_at(alias.clone(), API, 2),
                ],
                &budget,
            )
            .unwrap(),
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(resolved.errors.is_empty(), "{:?}", resolved.errors);
    assert!(
        resolved
            .contexts
            .iter()
            .all(|context| context.complete && context.semantic_available)
    );
    let first = &resolved.document_contexts[&source_name(&original).unwrap()];
    let second = &resolved.document_contexts[&source_name(&alias).unwrap()];
    assert_eq!(first.context, second.context);
    assert_eq!(first.file, second.file);
    let owner = resolved
        .contexts
        .iter()
        .find(|context| context.key == first.context)
        .unwrap();
    assert_eq!(owner.sources.file(first.file).unwrap().text(), API);
    assert_eq!(
        owner.origins[&first.file].module.package,
        owner.root.package
    );
}
