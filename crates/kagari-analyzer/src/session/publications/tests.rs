use super::*;
use crate::session::outbox::Outbox;
use kagari_common::identity::ModuleIdentity;
use kagari_source::source::SourceFile;
use lsp_types::{Diagnostic, Position, Range};
use serde_json::from_value;

fn document(version: i32, incarnation: u64) -> DocumentInput {
    DocumentInput {
        uri: "untitled:one".parse().unwrap(),
        source_name: "untitled:one".into(),
        version,
        incarnation,
        source: Some(Arc::new(SourceFile::new("untitled:one", ""))),
        unavailable: false,
    }
}

fn owner(name: &str) -> ContributionOwner {
    ContributionOwner::Context(ContextKey {
        workspace: "workspace".into(),
        root: ModuleIdentity::single_file(name),
        epoch: 1,
    })
}

fn group(uri: &str, messages: &[&str]) -> DiagnosticGroup {
    DiagnosticGroup::new(
        uri.parse().unwrap(),
        messages
            .iter()
            .map(|message| {
                Diagnostic::new_simple(
                    Range::new(Position::new(1, 2), Position::new(1, 3)),
                    (*message).into(),
                )
            })
            .collect(),
    )
    .unwrap()
}

fn receive(publications: &mut Publications) -> PublishDiagnosticsParams {
    let publication = publications.next(false).unwrap().unwrap();
    assert_eq!(
        publication.notification.method,
        "textDocument/publishDiagnostics"
    );
    from_value(publication.notification.params).unwrap()
}

#[test]
fn stale_generations_and_reopened_versions_cannot_replace_current_diagnostics() {
    let mut publications = Publications::new(&SessionLimits::default());
    publications.set_version_support(true);
    publications.invalidate(1, &[document(7, 1)]).unwrap();
    publications
        .replace(
            1,
            owner("one"),
            "root one".into(),
            vec![group("untitled:one", &["old"])],
        )
        .unwrap();
    let old = receive(&mut publications);
    assert_eq!(old.version, Some(7));
    publications.invalidate(2, &[document(8, 1)]).unwrap();
    assert!(
        !publications
            .replace(
                1,
                owner("one"),
                "root one".into(),
                vec![group("untitled:one", &["late"])]
            )
            .unwrap()
    );
    let clear = receive(&mut publications);
    assert_eq!(clear.version, Some(8));
    assert!(clear.diagnostics.is_empty());
    publications
        .replace(
            2,
            owner("one"),
            "root one".into(),
            vec![group("untitled:one", &["current"])],
        )
        .unwrap();
    publications.invalidate(3, &[]).unwrap();
    publications.invalidate(4, &[document(1, 2)]).unwrap();
    assert!(
        !publications
            .replace(
                2,
                owner("one"),
                "root one".into(),
                vec![group("untitled:one", &["closed incarnation"])]
            )
            .unwrap()
    );
    assert!(
        publications.next(false).unwrap().is_none(),
        "never-granted stale intent needs no clear"
    );
    publications
        .replace(
            4,
            owner("one"),
            "root one".into(),
            vec![group("untitled:one", &["reopened"])],
        )
        .unwrap();
    publications.set_version_support(false);
    let current = receive(&mut publications);
    assert_eq!(current.version, None);
    assert_eq!(current.diagnostics[0].message, "reopened");
}

#[test]
fn granted_frame_is_irrevocable_but_newer_output_cannot_be_followed_by_old_output() {
    let limits = SessionLimits::default();
    let mut publications = Publications::new(&limits);
    let mut outbox = Outbox::new(limits);
    publications.set_version_support(true);
    publications.invalidate(1, &[document(1, 1)]).unwrap();
    publications
        .replace(
            1,
            owner("one"),
            "root one".into(),
            vec![group("untitled:one", &["before"])],
        )
        .unwrap();
    let before = publications.next(false).unwrap().unwrap();
    let frame = outbox
        .grant_publication(
            before.notification.into(),
            before.charge,
            before.publication_serial,
        )
        .unwrap();
    publications.invalidate(2, &[document(2, 1)]).unwrap();
    publications
        .replace(
            2,
            owner("one"),
            "root one".into(),
            vec![group("untitled:one", &["after"])],
        )
        .unwrap();
    assert!(outbox.acknowledge(frame.sequence + 1).is_err());
    assert!(!outbox.is_empty());
    assert_eq!(outbox.acknowledge(frame.sequence).unwrap(), None);
    let after = receive(&mut publications);
    assert_eq!(after.version, Some(2));
    assert_eq!(after.diagnostics[0].message, "after");
    assert!(
        !publications
            .replace(
                1,
                owner("one"),
                "root one".into(),
                vec![group("untitled:one", &["late"])]
            )
            .unwrap()
    );
    assert!(publications.next(false).unwrap().is_none());
    publications.shutdown();
    assert_eq!(publications.retained_bytes(), 0);
}

#[test]
fn roots_merge_exact_duplicates_preserve_differences_and_never_relabel_stale_contributions() {
    let mut publications = Publications::new(&SessionLimits::default());
    publications.invalidate(1, &[]).unwrap();
    for root in ["a", "b"] {
        publications
            .replace(
                1,
                owner(root),
                format!("root {root}"),
                vec![group("untitled:shared", &["same"])],
            )
            .unwrap();
    }
    let merged = receive(&mut publications);
    assert_eq!(merged.diagnostics.len(), 1);
    assert_eq!(merged.diagnostics[0].message, "same");
    publications
        .replace(
            1,
            owner("b"),
            "root b".into(),
            vec![group("untitled:shared", &["different"])],
        )
        .unwrap();
    let differing = receive(&mut publications);
    let messages: BTreeSet<_> = differing
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();
    assert_eq!(
        messages,
        BTreeSet::from(["same\nContexts: root a", "different\nContexts: root b"])
    );
    publications.invalidate(2, &[]).unwrap();
    publications
        .replace(
            2,
            owner("a"),
            "root a".into(),
            vec![group("untitled:shared", &["fresh"])],
        )
        .unwrap();
    let fresh = receive(&mut publications);
    assert_eq!(fresh.diagnostics.len(), 1);
    assert_eq!(fresh.diagnostics[0].message, "fresh");
    publications
        .replace(2, owner("a"), "root a".into(), vec![])
        .unwrap();
    assert!(receive(&mut publications).diagnostics.is_empty());
    assert_eq!(publications.retained_bytes(), 0);
}

#[test]
fn reliable_fairness_is_bounded_and_cleared_uri_metadata_is_forgotten() {
    let limits = SessionLimits {
        publication_uris: 1,
        ..SessionLimits::default()
    };
    let mut publications = Publications::new(&limits);
    publications.invalidate(1, &[]).unwrap();
    publications
        .replace(
            1,
            owner("one"),
            "one".into(),
            vec![group("untitled:initial-clean", &[])],
        )
        .unwrap();
    assert!(receive(&mut publications).diagnostics.is_empty());
    assert_eq!(
        publications.retained_bytes(),
        0,
        "explicit clean completion leaves no tombstone"
    );
    for generation in 2..=13 {
        let uri = format!("untitled:document-{generation}");
        publications.invalidate(generation, &[]).unwrap();
        publications
            .replace(
                generation,
                owner("one"),
                "one".into(),
                vec![group(&uri, &["error"])],
            )
            .unwrap();
        for _ in 0..8 {
            assert!(publications.next(true).unwrap().is_none());
            publications.reliable_granted();
        }
        assert!(publications.next(true).unwrap().is_some());
        publications
            .replace(generation, owner("one"), "one".into(), vec![])
            .unwrap();
        assert!(receive(&mut publications).diagnostics.is_empty());
        assert_eq!(publications.retained_bytes(), 0);
        assert!(publications.next(false).unwrap().is_none());
    }
}

#[test]
fn count_and_byte_overflow_are_atomic_and_large_frames_fail_visibly() {
    for limits in [
        SessionLimits {
            publication_uris: 1,
            ..SessionLimits::default()
        },
        SessionLimits {
            publication_diagnostics: 1,
            ..SessionLimits::default()
        },
        SessionLimits {
            publication_bytes: 2048,
            ..SessionLimits::default()
        },
    ] {
        let mut publications = Publications::new(&limits);
        publications.invalidate(1, &[]).unwrap();
        publications
            .replace(
                1,
                owner("a"),
                "a".into(),
                vec![group("untitled:one", &["accepted"])],
            )
            .unwrap();
        let before = publications.retained_bytes();
        let extra = if limits.publication_bytes == 2048 {
            "x".repeat(4096)
        } else {
            "extra".into()
        };
        assert!(
            publications
                .replace(
                    1,
                    owner("b"),
                    "b".into(),
                    vec![group("untitled:two", &[&extra])]
                )
                .is_err()
        );
        assert_eq!(publications.retained_bytes(), before);
        assert_eq!(
            receive(&mut publications).diagnostics[0].message,
            "accepted"
        );
        assert!(publications.next(false).unwrap().is_none());
    }
    let mut publications = Publications::new(&SessionLimits {
        result_bytes: 128,
        ..SessionLimits::default()
    });
    publications.invalidate(1, &[]).unwrap();
    publications
        .replace(
            1,
            owner("a"),
            "a".into(),
            vec![group("untitled:one", &["cannot fit"])],
        )
        .unwrap();
    assert!(publications.next(false).is_err());
    assert_eq!(publications.intents.len(), 1);
    publications.shutdown();
    assert!(publications.next(false).unwrap().is_none());
    assert!(
        !publications
            .replace(1, owner("a"), "a".into(), vec![])
            .unwrap()
    );
    assert_eq!(publications.retained_bytes(), 0);
}
