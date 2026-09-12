use std::sync::Arc;

use kagari_common::{SourceFile, source_database::SourceLayer};
use kagari_embed::{ArtifactOptions, CompileOptions, EmbeddingError, KagariEngine};
use kagari_hir::analysis::CancellationToken;
use kagari_runtime::LanguageProfile;

#[test]
fn reused_literal_and_pattern_facts_emit_the_same_artifact_as_fresh_analysis() {
    let engine = KagariEngine::default();
    let token = CancellationToken::default();
    let unchanged = "fn b() -> i32 { match 2147483647 { 2147483647 => -2147483648, _ => 0 } }";
    let id = engine
        .set_source(
            "memory://reuse.kgr",
            format!("fn a() -> i32 {{ 1 }} {unchanged}"),
            SourceLayer::Base,
        )
        .unwrap();
    engine
        .analyze(engine.source_snapshot(), Default::default(), &token)
        .unwrap();
    let edited = format!("fn a() -> i32 {{ 10 + 20 + 30 }} {unchanged}");
    engine
        .set_source("memory://reuse.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let snapshot = engine.source_snapshot();
    let analysis = engine
        .analyze(snapshot.clone(), Default::default(), &token)
        .unwrap();
    assert_eq!(
        analysis
            .file(id)
            .unwrap()
            .result()
            .facts()
            .typed
            .reused_bodies,
        1
    );
    let reused = engine
        .compile_snapshot(snapshot, id, Default::default(), &token)
        .unwrap();
    let fresh_engine = KagariEngine::default();
    let fresh = fresh_engine
        .compile_source(
            SourceFile::new("memory://reuse.kgr", edited),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        engine
            .emit_bytecode(&reused, Default::default())
            .unwrap()
            .to_bytes()
            .unwrap(),
        fresh_engine
            .emit_bytecode(&fresh, Default::default())
            .unwrap()
            .to_bytes()
            .unwrap()
    );
}

#[test]
fn compilation_and_tools_share_overlay_revision_and_profile() {
    let engine = KagariEngine::default();
    let token = CancellationToken::default();
    let id = engine
        .set_source(
            "memory://main.kgr",
            "fn main() -> i32 { 1 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let original = engine.source_snapshot();
    engine
        .set_source(
            "memory://main.kgr",
            "fn main() -> i32 { 2 }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let edited = engine.source_snapshot();
    let current = engine
        .analyze(edited.clone(), LanguageProfile::default(), &token)
        .unwrap();
    let old = engine
        .analyze(original.clone(), LanguageProfile::default(), &token)
        .unwrap();
    assert!(old.revision() < current.revision());
    let again = engine
        .analyze(edited, LanguageProfile::default(), &token)
        .unwrap();
    assert!(
        Arc::ptr_eq(current.file(id).unwrap(), again.file(id).unwrap()),
        "an older query must not replace the current cache"
    );

    // Supplying new base text cannot silently bypass an editor's overlay.
    let checked = engine
        .compile_source(
            SourceFile::new("memory://main.kgr", "fn main() -> i32 { 3 }"),
            CompileOptions::default(),
        )
        .unwrap();
    let artifact = engine
        .emit_bytecode(&checked, ArtifactOptions::default())
        .unwrap();
    let from_snapshot = engine
        .compile_snapshot(
            engine.source_snapshot(),
            id,
            CompileOptions::default(),
            &token,
        )
        .unwrap();
    assert_eq!(
        artifact.to_bytes().unwrap(),
        engine
            .emit_bytecode(&from_snapshot, ArtifactOptions::default())
            .unwrap()
            .to_bytes()
            .unwrap()
    );
    assert!(
        engine
            .source_snapshot()
            .file(id)
            .unwrap()
            .text()
            .contains("{ 2 }")
    );
    engine.close_overlay("memory://main.kgr").unwrap();
    assert!(
        engine
            .source_snapshot()
            .file(id)
            .unwrap()
            .text()
            .contains("{ 3 }")
    );
    assert!(original.file(id).unwrap().text().contains("{ 1 }"));

    token.cancel();
    assert!(matches!(
        engine.compile_snapshot(original, id, CompileOptions::default(), &token),
        Err(EmbeddingError::Cancelled)
    ));
}

#[test]
fn profile_changes_do_not_reuse_a_previously_accepted_result() {
    let engine = KagariEngine::default();
    let id = engine
        .set_source(
            "memory://reflect.kgr",
            "fn main() { type_of(1); }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let token = CancellationToken::default();
    let permissive = LanguageProfile {
        allow_reflection: true,
        ..LanguageProfile::default()
    };
    let allowed = engine
        .analyze(engine.source_snapshot(), permissive, &token)
        .unwrap();
    let restricted = engine
        .analyze(engine.source_snapshot(), LanguageProfile::default(), &token)
        .unwrap();
    assert!(!Arc::ptr_eq(
        allowed.file(id).unwrap(),
        restricted.file(id).unwrap()
    ));
    assert!(
        restricted.file(id).unwrap().result().diagnostics().len()
            > allowed.file(id).unwrap().result().diagnostics().len()
    );
}

#[test]
fn diagnostics_carry_the_source_revision_that_produced_them() {
    let engine = KagariEngine::default();
    let id = engine
        .set_source(
            "memory://bad.kgr",
            "fn main() { missing; }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = engine.source_snapshot();
    let Err(EmbeddingError::Diagnostics { diagnostics }) = engine.compile_snapshot(
        snapshot.clone(),
        id,
        CompileOptions::default(),
        &CancellationToken::default(),
    ) else {
        panic!("expected source diagnostics")
    };
    let span = diagnostics
        .iter()
        .find_map(|diagnostic| diagnostic.span)
        .unwrap();
    assert!(snapshot.contains(span));
    engine
        .set_source(
            "memory://bad.kgr",
            "fn main() {}".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    assert!(!engine.source_snapshot().contains(span));
}
