use crate::{
    analysis::work::{self, Phase},
    tests::test_analysis,
    types::TypeId,
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::BuiltinType;

const TEXT: &str = include_str!("../../../tests/fixtures/editor/positions.kgr");

#[test]
fn selected_editor_routes_inline_scopes_native_targets_and_recovered_members() {
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("positions.kgr", TEXT.into(), SourceLayer::Base)
        .unwrap();
    let mut database = test_analysis();
    let position = TEXT.find("prior; }").unwrap();
    let measured = work::measure(|| {
        database
            .editor(
                sources.snapshot(),
                file,
                position,
                &CancellationToken::default(),
            )
            .unwrap()
            .unwrap()
    })
    .unwrap();
    assert_eq!(measured.work.full_snapshot_calls, 0);
    assert_eq!(measured.work.body_checks_started, 1);
    assert_eq!(measured.work.selected_body_calls, 1);
    assert!(measured.work.phase_nanoseconds(Phase::PrepareSignatures) > 0);
    let editor = measured.value;
    assert_eq!(
        editor.type_at(position),
        Some(TypeId::Builtin(BuiltinType::Bool))
    );
    let binding = editor.definition_at(position).unwrap();
    assert_eq!(
        binding.location.range.start,
        TEXT.find("prior = true").unwrap()
    );
    let completion = editor
        .completions(position + 3, 128, &CancellationToken::default())
        .unwrap();
    assert_eq!(completion.replacement.start, position);
    assert_eq!(completion.replacement.end, position + "prior".len());
    assert_eq!(
        completion
            .candidates
            .iter()
            .find(|candidate| candidate.name == "prior")
            .unwrap()
            .ty,
        Some(TypeId::Builtin(BuiltinType::Bool))
    );
    let call = TEXT.find("packet.read()").unwrap() + "packet.".len();
    let declaration = editor.definition_at(call).unwrap();
    assert_eq!(declaration.name, "read");
    let docs = editor.documentation_at(call).unwrap();
    assert_eq!(docs.written_signature, "fn read(self) -> i32;");
    let method = TEXT.find("fn read(self) -> i32 { self.value").unwrap() + 3;
    let header = database
        .editor(
            sources.snapshot(),
            file,
            method,
            &CancellationToken::default(),
        )
        .unwrap()
        .unwrap();
    let docs = header.documentation_at(method).unwrap();
    assert_eq!(docs.written_signature, "fn read(self) -> i32");
    assert_eq!(docs.documentation, "Reads the packet.");
    let incomplete = TEXT.find("packet. }").unwrap() + "packet.".len();
    let selected = database
        .editor(
            sources.snapshot(),
            file,
            incomplete,
            &CancellationToken::default(),
        )
        .unwrap()
        .unwrap();
    let members = selected
        .completions(incomplete, 128, &CancellationToken::default())
        .unwrap();
    for name in ["value", "size", "inherited", "read"] {
        assert!(
            members
                .candidates
                .iter()
                .any(|candidate| candidate.name == name),
            "missing {name}: {members:?}"
        );
    }
    assert!(
        selected
            .completions(incomplete, 1, &CancellationToken::default())
            .is_err()
    );
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(selected.completions(incomplete, 128, &cancelled).is_err());
    let create = TEXT.find("create();").unwrap();
    let native = database
        .editor(
            sources.snapshot(),
            file,
            create,
            &CancellationToken::default(),
        )
        .unwrap()
        .unwrap();
    let declaration = native.definition_at(create).unwrap();
    assert_eq!(declaration.name, "new");
    let target = native
        .signature_snapshot()
        .source(declaration.location.file)
        .unwrap();
    assert_eq!(
        &target.text()[declaration.location.range.start..declaration.location.range.end],
        "new"
    );
    let nested = TEXT.find("42 }").unwrap();
    let inline = database
        .editor(
            sources.snapshot(),
            file,
            nested,
            &CancellationToken::default(),
        )
        .unwrap()
        .unwrap();
    assert_ne!(inline.source().id(), file);
    assert_eq!(inline.source().origin_id(), file);
    assert_eq!(
        inline.type_at(nested),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    let header = TEXT.find("selected(packet").unwrap();
    let measured = work::measure(|| {
        database
            .editor(
                sources.snapshot(),
                file,
                header,
                &CancellationToken::default(),
            )
            .unwrap()
            .unwrap()
    })
    .unwrap();
    assert!(measured.value.body().is_none());
    assert_eq!(measured.work.body_checks_started, 0);
    assert_eq!(measured.work.selected_body_calls, 0);
    assert_eq!(
        editor.type_at(position),
        Some(TypeId::Builtin(BuiltinType::Bool)),
        "earlier result stays immutable"
    );
}

#[test]
fn selected_history_is_bounded_while_explicit_old_results_remain_valid() {
    let mut sources = SourceDatabase::default();
    let mut database = crate::analysis::AnalysisDatabase::default();
    let text = |edited: usize| -> String {
        (0..18)
            .map(|function| {
                format!(
                    "fn f{function}() -> i32 {{ {} }}\n",
                    if function == edited { 9 } else { 1 }
                )
            })
            .collect()
    };
    let first = text(0);
    let file = sources
        .set("bounded.kgr", first.clone(), SourceLayer::Base)
        .unwrap();
    let old = database
        .editor(
            sources.snapshot(),
            file,
            first.find("{ 9").unwrap() + 2,
            &CancellationToken::default(),
        )
        .unwrap()
        .unwrap();
    for function in 1..18 {
        let current = text(function);
        sources
            .set("bounded.kgr", current.clone(), SourceLayer::Overlay)
            .unwrap();
        let offset = current.find(&format!("fn f{function}()")).unwrap()
            + format!("fn f{function}() -> i32 {{ ").len();
        let selected = database
            .editor(
                sources.snapshot(),
                file,
                offset,
                &CancellationToken::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            selected.type_at(offset),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(database.body_cache.len() <= 16);
    }
    assert_eq!(old.source().text(), first);
    let retained = database.retained_sources();
    let revisions: std::collections::BTreeSet<_> =
        retained.iter().map(|source| source.revision()).collect();
    assert_eq!(revisions.len(), 16);
    assert!(
        !revisions.contains(&old.source().revision()),
        "an explicit old query owns its lease independently of the retired cache entry"
    );
}

#[test]
fn body_reuse_refreshes_foreign_sites_and_checks_only_referenced_callable_contracts() {
    use kagari_common::identity::{ModuleIdentity, PackageId};
    let library = include_str!("../../../tests/fixtures/editor/library.kgr");
    let consumer = include_str!("../../../tests/fixtures/editor/consumer.kgr");
    let unaffected = include_str!("../../../tests/fixtures/editor/unaffected.kgr");
    let mut sources = SourceDatabase::default();
    let mut files = Vec::new();
    for (module, text) in [
        ("library", library),
        ("consumer", consumer),
        ("unaffected", unaffected),
    ] {
        let name = format!("{module}.kgr");
        sources
            .bind_module(
                &name,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![module.into()],
                },
            )
            .unwrap();
        files.push(sources.set(&name, text.into(), SourceLayer::Base).unwrap());
    }
    let mut database = test_analysis();
    let old = database
        .snapshot(sources.snapshot(), &CancellationToken::default())
        .unwrap();
    let call = consumer.find("alias()").unwrap();
    let old_location = old.definition_at(files[1], call).unwrap().location;
    let shifted = format!("// moved 😀\n{}", library.replacen("7", "9", 1));
    sources
        .set("library.kgr", shifted.clone(), SourceLayer::Overlay)
        .unwrap();
    let measured = work::measure(|| {
        database
            .snapshot(sources.snapshot(), &CancellationToken::default())
            .unwrap()
    })
    .unwrap();
    assert_eq!(measured.work.body_checks_started, 1);
    assert_eq!(
        measured
            .value
            .file(files[1])
            .unwrap()
            .result()
            .facts()
            .typed
            .checked_bodies(),
        0
    );
    assert_eq!(
        measured
            .value
            .file(files[1])
            .unwrap()
            .result()
            .facts()
            .typed
            .reused_bodies(),
        1
    );
    let current_location = measured
        .value
        .definition_at(files[1], call)
        .unwrap()
        .location;
    assert_ne!(old_location, current_location);
    assert_eq!(
        old.definition_at(files[1], call).unwrap().location,
        old_location
    );
    let fresh = test_analysis()
        .snapshot(sources.snapshot(), &CancellationToken::default())
        .unwrap();
    assert_eq!(
        fresh.definition_at(files[1], call).unwrap().location,
        current_location
    );
    let unused = shifted
        .replace("unused() -> i32", "unused() -> bool")
        .replace("    2", "    true");
    sources
        .set("library.kgr", unused.clone(), SourceLayer::Overlay)
        .unwrap();
    let measured = work::measure(|| {
        database
            .snapshot(sources.snapshot(), &CancellationToken::default())
            .unwrap()
    })
    .unwrap();
    assert_eq!(
        measured
            .value
            .file(files[1])
            .unwrap()
            .result()
            .facts()
            .typed
            .checked_bodies(),
        0,
        "unused foreign callable changes do not recheck this consumer"
    );
    assert_eq!(
        measured
            .value
            .file(files[1])
            .unwrap()
            .result()
            .facts()
            .typed
            .reused_bodies(),
        1
    );
    assert!(
        measured
            .value
            .file(files[1])
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    let used = unused
        .replace("number() -> i32", "number() -> bool")
        .replace("    9", "    true");
    sources
        .set("library.kgr", used.clone(), SourceLayer::Overlay)
        .unwrap();
    let current = database
        .snapshot(sources.snapshot(), &CancellationToken::default())
        .unwrap();
    let fresh = test_analysis()
        .snapshot(sources.snapshot(), &CancellationToken::default())
        .unwrap();
    let facts = current.file(files[1]).unwrap();
    assert_eq!(facts.result().facts().typed.checked_bodies(), 1);
    assert!(!facts.result().diagnostics().is_empty());
    assert_eq!(
        facts.result().diagnostics(),
        fresh.file(files[1]).unwrap().result().diagnostics()
    );
    assert_eq!(
        facts.type_at(call),
        Some(TypeId::Builtin(BuiltinType::Bool))
    );
    // A new conflicting binding changes namespace selection even when an older
    // successful function identity still exists in the context's append-only table.
    sources
        .set(
            "library.kgr",
            used.replace("unused()", "number()"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let ambiguous = database
        .snapshot(sources.snapshot(), &CancellationToken::default())
        .unwrap();
    assert!(ambiguous.definition_at(files[1], call).is_none());
    assert!(
        !ambiguous
            .file(files[1])
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
}
