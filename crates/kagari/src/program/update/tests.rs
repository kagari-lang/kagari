use crate::{
    engine::{
        KagariEngine,
        source::{ArtifactOptions, CheckedModule, PreparationOptions, analysis::SourceAnalysis},
    },
    error::EmbeddingError,
    program::{Compilation, Program, update::SourceEditTarget},
    runtime::{ReloadOptions, Runtime},
    update::Update,
};
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use kagari_contract::source::AuthoredKind;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_package::{modules::VirtualSources, project::ProjectInputs};
use kagari_runtime::error::RuntimeErrorKind;
use kagari_runtime::reload::ReloadValidationError;
use kagari_runtime::{
    module::InstalledProgram,
    native::function_handle::Function,
    source::{SourceDeclaration, SourceQuery},
};
use kagari_source::{
    identity::FileId,
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer, SourceSnapshot},
};
use kagari_stdlib as stdlib;
use rstest::rstest;
use std::{sync::Arc, thread};

fn engine() -> KagariEngine {
    let mut builder = KagariEngine::builder().unwrap();
    builder.install_all(stdlib::modules().unwrap()).unwrap();
    builder.source_root("/source-updates").unwrap();
    builder.build().unwrap()
}

fn check(sources: SourceSnapshot, root: FileId) -> CheckedModule {
    // Explicit source snapshots exercise detached compilation independently of the
    // default engine's installed providers, covered by the foundation test below.
    let mut analysis = AnalysisDatabase::new("/source-updates").unwrap();
    let cancel = CancellationToken::default();
    let snapshot = analysis.snapshot(sources, &cancel).unwrap();
    SourceAnalysis::new(snapshot, root, None)
        .unwrap()
        .check(&cancel)
        .unwrap()
}

fn checked_source(text: &str) -> CheckedModule {
    let mut sources = SourceDatabase::new("/source-updates").unwrap();
    let root = sources
        .set("app.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    check(sources.snapshot(), root)
}

fn prepare(engine: &KagariEngine, text: &str) -> Compilation {
    engine
        .prepare(&checked_source(text), Default::default())
        .unwrap()
}

fn publish_update(
    runtime: &mut Runtime,
    program: &InstalledProgram,
    prepared: &mut Compilation,
    update: Update,
) -> Result<(), EmbeddingError> {
    let cancellation = update.cancel.clone();
    let candidate = prepared.prepare_update(update)?;
    runtime.replace_program(
        program,
        candidate.program(),
        ReloadOptions {
            cancellation,
            ..Default::default()
        },
    )?;
    *prepared = candidate;
    Ok(())
}

fn call(runtime: &mut Runtime, program: &InstalledProgram, name: &str) -> i32 {
    runtime
        .execute_typed::<(), i32>(program, name, (), &Default::default())
        .unwrap()
}

fn declaration(runtime: &Runtime, program: &InstalledProgram, name: &str) -> SourceDeclaration {
    let snapshot = runtime.source_snapshot(program).unwrap();
    let matches = snapshot
        .declarations()
        .iter()
        .filter(|item| item.name() == name)
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "ambiguous test declaration {name}");
    matches[0].clone()
}

fn edit_target(base: &Compilation, name: &str) -> SourceEditTarget {
    let targets = base.source_declarations().unwrap();
    let matches = targets
        .into_iter()
        .filter(|target| target.name() == name)
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "ambiguous test edit target {name}");
    matches.into_iter().next().unwrap()
}

fn text(runtime: &Runtime, program: &InstalledProgram, declaration: &SourceDeclaration) -> String {
    match runtime
        .source_snapshot(program)
        .unwrap()
        .function(declaration)
    {
        SourceQuery::Available(source) => source.text.clone(),
        unavailable => panic!("expected exact source, got {unavailable:?}"),
    }
}

#[test]
fn complete_package_batch_adds_modules_and_preserves_state_and_failed_baseline() {
    let cancel = CancellationToken::default();
    let root = "static var COUNT: i32 = 0; pub fn bump() -> i32 { COUNT += 1; COUNT } pub fn answer() -> i32 { rules::value() }";
    let sources = VirtualSources::new([
        ("Kagari.toml".into(), Arc::from("[package]\nname = 'app'\nnamespace = 'tests/source-batches'\nversion = '1.0.0'\n[dependencies]\nrules = { path = 'rules' }")),
        ("src/lib.kgr".into(), Arc::from(root)),
        ("rules/Kagari.toml".into(), Arc::from("[package]\nname = 'rules'\nnamespace = 'tests/source-rules'\nversion = '1.0.0'")),
        ("rules/src/lib.kgr".into(), Arc::from("pub fn value() -> i32 { 40 }")),
    ], &cancel).unwrap();
    let project = ProjectInputs::load(&sources, "", &Default::default(), &cancel).unwrap();
    let engine = engine();
    let input = project.programs().values().next().unwrap();
    let mut sources = SourceDatabase::new("/source-updates").unwrap();
    let captured = input.source_snapshot(&mut sources, &cancel).unwrap();
    let root_module = ModuleIdentity {
        package: input.root().family.package_id(),
        path: Vec::new(),
    };
    let checked = check(
        captured.clone(),
        captured.module(&root_module).unwrap().id(),
    );
    let mut extra = checked.module_identity().clone();
    extra.path.push("extra".into());
    let mut prepared = engine.prepare(&checked, Default::default()).unwrap();
    let mut runtime = engine.runtime(Default::default());
    let program = runtime
        .load_program(prepared.program(), Default::default())
        .unwrap();
    let mut recipient = engine.runtime(Default::default());
    let recipient_program = recipient
        .load_program(prepared.program(), Default::default())
        .unwrap();
    let old = runtime.source_snapshot(&program).unwrap();
    let answer = declaration(&runtime, &program, "answer");
    let old_capsule = Arc::downgrade(prepared.compilation.as_ref().unwrap());
    assert_eq!(call(&mut runtime, &program, "bump"), 1);
    assert_eq!(call(&mut runtime, &program, "answer"), 40);
    let mut update = Update::new();
    update.replace_file("src/./lib.kgr", "mod extra; static var COUNT: i32 = 0; pub fn bump() -> i32 { COUNT += 1; COUNT } pub fn answer() -> i32 { rules::new_value() + extra::bonus() }");
    update.replace_file(
        "rules/src/lib.kgr",
        "pub fn value() -> i32 { 40 } pub fn new_value() -> i32 { 41 }",
    );
    update.add_file(extra, "src/extra.kgr", "pub fn bonus() -> i32 { 1 }");
    let base = prepared.clone();
    let origin = thread::current().id();
    let preparation = thread::spawn(move || {
        assert_ne!(thread::current().id(), origin);
        base.prepare_update(update).unwrap()
    });
    // The recipient can execute its published version while the worker owns
    // detached preparation. One returned product serves both local publishers.
    assert_eq!(call(&mut runtime, &program, "answer"), 40);
    let candidate = preparation.join().unwrap();
    runtime
        .replace_program(&program, candidate.program(), Default::default())
        .unwrap();
    let recipient_before = recipient.publication_token(&recipient_program).unwrap();
    let published_before = runtime.publication_token(&program).unwrap();
    let local_cancellation = CancellationToken::default();
    local_cancellation.cancel();
    assert!(matches!(
        recipient.replace_program(
            &recipient_program,
            candidate.program(),
            ReloadOptions {
                cancellation: local_cancellation,
                ..Default::default()
            }
        ),
        Err(ReloadValidationError::Runtime(ref error)) if error.kind() == RuntimeErrorKind::Cancelled
    ));
    assert_eq!(
        recipient.publication_token(&recipient_program).unwrap(),
        recipient_before
    );
    assert_eq!(
        runtime.publication_token(&program).unwrap(),
        published_before
    );
    assert_eq!(call(&mut recipient, &recipient_program, "answer"), 40);
    recipient
        .replace_program(&recipient_program, candidate.program(), Default::default())
        .unwrap();
    prepared = candidate;
    assert_eq!(call(&mut runtime, &program, "answer"), 42);
    assert_eq!(call(&mut runtime, &program, "bump"), 2);
    assert_eq!(call(&mut recipient, &recipient_program, "answer"), 42);
    assert_eq!(call(&mut recipient, &recipient_program, "bump"), 1);
    runtime.collect_garbage().unwrap();
    recipient.collect_garbage().unwrap();
    assert!(
        old_capsule.upgrade().is_none(),
        "installed executable/type provenance must not retain compiler inputs"
    );
    assert!(
        matches!(old.function(&answer), SourceQuery::Available(source) if source.text.contains("rules::value()"))
    );
    let successful = text(&runtime, &program, &answer);
    let published = runtime.publication_token(&program).unwrap();
    let mut invalid = Update::new();
    invalid.replace_function("answer", "pub fn answer() -> bool { true }");
    assert!(matches!(
        publish_update(&mut runtime, &program, &mut prepared, invalid),
        Err(EmbeddingError::ReloadValidation { .. })
    ));
    assert_eq!(runtime.publication_token(&program).unwrap(), published);
    assert_eq!(text(&runtime, &program, &answer), successful);
    assert_eq!(call(&mut runtime, &program, "bump"), 3);
    let mut next = Update::new();
    next.replace_function(
        "answer",
        "pub fn answer() -> i32 { rules::new_value() + extra::bonus() + 1 }",
    );
    publish_update(&mut runtime, &program, &mut prepared, next).unwrap();
    assert_eq!(call(&mut runtime, &program, "answer"), 43);
    assert_eq!(
        captured.module(checked.module_identity()).unwrap().text(),
        root
    );
    drop(runtime);
    drop(recipient);
    assert!(
        old_capsule.upgrade().is_none(),
        "candidate base identity must not own old payloads"
    );
}

#[test]
fn opaque_private_method_and_named_local_targets_preserve_context_and_locator_identity() {
    let source =
        include_str!("../../../tests/fixtures/embedding/source-updates/private-targets.kgr");
    let engine = engine();
    let mut prepared = prepare(&engine, source);
    let mut runtime = engine.runtime(Default::default());
    let program = runtime
        .load_program(prepared.program(), Default::default())
        .unwrap();
    let generic = declaration(&runtime, &program, "unused");
    let mut update = Update::new();
    update.replace_function(
        edit_target(&prepared, "hidden"),
        "fn hidden() -> i32 { /* π changes later byte offsets */ 10 }",
    );
    update.replace_function(
        edit_target(&prepared, "capsule_read"),
        "fn capsule_read(self) -> i32 { hidden() + 2 }",
    );
    update.replace_function(
        edit_target(&prepared, "capsule_local"),
        "fn capsule_local() -> i32 { 9 }",
    );
    update.replace_function(
        "generic_alias",
        "pub fn unused<T>(v: T) -> i32 where T: Bound { 6 }",
    );
    publish_update(&mut runtime, &program, &mut prepared, update).unwrap();
    assert_eq!(call(&mut runtime, &program, "answer"), 12);
    assert_eq!(call(&mut runtime, &program, "named"), 9);
    assert!(text(&runtime, &program, &generic).contains("{ 6 }"));
    let mut changed_bounds = Update::new();
    changed_bounds.replace_function("generic_alias", "pub fn unused<T>(v: T) -> i32 { 99 }");
    assert!(matches!(
        publish_update(&mut runtime, &program, &mut prepared, changed_bounds),
        Err(EmbeddingError::ReloadValidation { .. })
    ));
    assert!(text(&runtime, &program, &generic).contains("where T: Bound { 6 }"));
    let mut next = Update::new();
    next.replace_function(
        edit_target(&prepared, "hidden"),
        "fn hidden() -> i32 { 20 }",
    );
    publish_update(&mut runtime, &program, &mut prepared, next).unwrap();
    assert_eq!(call(&mut runtime, &program, "answer"), 22);
    assert!(
        runtime
            .bind_function::<(), i32>(&program.root(), "hidden")
            .is_err()
    );
}

#[test]
fn retained_old_closure_demands_recompile_generic_applications_removed_from_new_source_calls() {
    let engine = engine();
    let source = "pub fn retained<T>(value: T) -> i32 { 1 } pub fn factory() -> fn() -> i32 { || retained(7i32) }";
    let mut prepared = prepare(&engine, source);
    let mut runtime = engine.runtime(Default::default());
    let program = runtime
        .load_program(prepared.program(), Default::default())
        .unwrap();
    let factory = runtime
        .bind_function::<(), Function<(), i32>>(&program.root(), "factory")
        .unwrap();
    let old_closure = runtime.call(&factory, ()).unwrap();
    assert_eq!(runtime.call(&old_closure, ()).unwrap(), 1);
    let mut update = Update::new();
    update.replace_function("retained", "pub fn retained<T>(value: T) -> i32 { 2 }");
    update.replace_function("factory", "pub fn factory() -> fn() -> i32 { || 100 }");
    publish_update(&mut runtime, &program, &mut prepared, update).unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.call(&old_closure, ()).unwrap(), 2);
    let fresh = runtime.call(&factory, ()).unwrap();
    assert_eq!(runtime.call(&fresh, ()).unwrap(), 100);
}

#[test]
fn malformed_duplicate_overlapping_and_foreign_updates_leave_source_unchanged() {
    let engine = engine();
    let source =
        include_str!("../../../tests/fixtures/embedding/source-updates/invalid-batches.kgr");
    let mut prepared = prepare(&engine, source);
    let mut runtime = engine.runtime(Default::default());
    let program = runtime
        .load_program(prepared.program(), Default::default())
        .unwrap();
    let answer = declaration(&runtime, &program, "answer");
    let nested = edit_target(&prepared, "nested_target");
    let structure = edit_target(&prepared, "NotAFunction");
    assert_eq!(structure.kind(), AuthoredKind::Declaration);
    let foreign = edit_target(&prepare(&engine, source), "answer");
    let token = runtime.publication_token(&program).unwrap();
    let mut failures = vec![Update::new()];
    for definition in [
        "{ 2 }",
        "pub fn answer() -> i32 { true }",
        "pub fn renamed() -> i32 { 2 }",
        "pub fn answer() -> i32 { fn nested_target() -> i32 { 1 } nested_target() } pub fn injected() -> i32 { 9 }",
        "pub fn answer( {",
    ] {
        let mut update = Update::new();
        update.replace_function("answer", definition);
        failures.push(update);
    }
    let mut aliases = Update::new();
    aliases
        .replace_function("answer", source)
        .replace_function("alias", source);
    failures.push(aliases);
    let mut overlap = Update::new();
    overlap
        .replace_file("app.kgr", source)
        .replace_function(edit_target(&prepared, "answer"), source);
    failures.push(overlap);
    let mut nested_overlap = Update::new();
    nested_overlap
        .replace_function(edit_target(&prepared, "answer"), source)
        .replace_function(nested, "fn nested_target() -> i32 { 2 }");
    failures.push(nested_overlap);
    for target in [structure, foreign] {
        let mut update = Update::new();
        update.replace_function(target, "pub fn answer() -> i32 { 2 }");
        failures.push(update);
    }
    let mut unknown = Update::new();
    unknown.replace_function("does_not_exist", source);
    failures.push(unknown);
    let mut names = Update::new();
    names
        .replace_file("app.kgr", source)
        .replace_file("./app.kgr", source);
    failures.push(names);
    for update in failures {
        assert!(publish_update(&mut runtime, &program, &mut prepared, update).is_err());
        assert_eq!(runtime.publication_token(&program).unwrap(), token);
        assert_eq!(call(&mut runtime, &program, "answer"), 1);
        assert!(text(&runtime, &program, &answer).contains("nested_target()"));
    }
    let mut valid = Update::new();
    valid.replace_function(
        "alias",
        "pub fn answer() -> i32 { fn nested_target() -> i32 { 1 } nested_target() + 1 }",
    );
    publish_update(&mut runtime, &program, &mut prepared, valid).unwrap();
    assert_eq!(call(&mut runtime, &program, "answer"), 2);
}

#[test]
fn admission_bounds_and_request_cancellation_precede_compilation() {
    let engine = engine();
    let cancelled_preparation = CancellationToken::default();
    let mut options = PreparationOptions::default();
    options.lowering.cancel = cancelled_preparation.clone();
    let mut prepared = engine
        .prepare(&checked_source("pub fn answer() -> i32 { 1 }"), options)
        .unwrap();
    cancelled_preparation.cancel();
    let mut runtime = engine.runtime(Default::default());
    let program = runtime
        .load_program(prepared.program(), Default::default())
        .unwrap();
    let guard = runtime.begin_publication(&program).unwrap();
    let mut detached = Update::new();
    detached.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    let candidate = prepared.prepare_update(detached).unwrap();
    assert!(candidate.program().expects_base(prepared.program()));
    drop(guard);
    drop(candidate);
    let mut excessive = Update::new();
    for _ in 0..1025 {
        excessive.replace_file("app.kgr", "");
    }
    assert!(
        matches!(publish_update(&mut runtime, &program, &mut prepared, excessive), Err(EmbeddingError::Source { message }) if message.contains("1024"))
    );
    let mut oversized = Update::new();
    oversized.replace_file("app.kgr", " ".repeat(64 * 1024 * 1024 + 1));
    assert!(
        matches!(publish_update(&mut runtime, &program, &mut prepared, oversized), Err(EmbeddingError::Source { message }) if message.contains("64 MiB"))
    );
    let request = CancellationToken::default();
    request.cancel();
    let mut cancelled = Update::new().with_cancellation(request);
    cancelled.replace_function("answer", "pub fn answer() -> i32 { 9 }");
    assert!(matches!(
        publish_update(&mut runtime, &program, &mut prepared, cancelled),
        Err(EmbeddingError::Cancelled)
    ));
    assert_eq!(call(&mut runtime, &program, "answer"), 1);
    let mut fresh = Update::new();
    fresh.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    publish_update(&mut runtime, &program, &mut prepared, fresh).unwrap();
    assert_eq!(call(&mut runtime, &program, "answer"), 2);
}

#[rstest]
#[case::sdk_reload(false)]
#[case::core_reload(true)]
fn artifact_source_is_opt_in_and_low_level_or_artifact_reload_never_reuses_old_capsules(
    #[case] low_level: bool,
) {
    let engine = engine();
    let checked = checked_source("pub fn answer() -> i32 { 1 }");
    let mut direct = engine.prepare(&checked, Default::default()).unwrap();
    assert!(direct.program().bytecode().source_package().is_some());
    let stripped = engine.emit_artifact(&checked, Default::default()).unwrap();
    let stripped =
        Program::from_artifact(stripped, &Default::default(), &Default::default()).unwrap();
    assert!(stripped.bytecode().source_package().is_none());
    let artifact = engine
        .emit_artifact(
            &checked,
            ArtifactOptions {
                retain_source: true,
                ..Default::default()
            },
        )
        .unwrap();
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let artifact = Compilation::from_artifact(
        crate::BytecodeArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(artifact.program().bytecode().source_package().is_some());
    let mut runtime = engine.runtime(Default::default());
    let program = runtime
        .load_program(direct.program(), Default::default())
        .unwrap();
    if low_level {
        let candidate = runtime
            .stage_reload_verified_program(
                &program,
                None,
                artifact.program().clone().into_verified(),
            )
            .unwrap();
        runtime.publish_staged_reload(candidate).unwrap();
    } else {
        let candidate = direct
            .program()
            .prepare_replacement(artifact.program(), &Default::default())
            .unwrap();
        runtime
            .replace_program(&program, &candidate, Default::default())
            .unwrap();
    }
    assert!(
        runtime
            .source_snapshot(&program)
            .unwrap()
            .source_package()
            .is_some()
    );
    let mut update = Update::new();
    update.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    assert!(
        matches!(artifact.prepare_update(update), Err(EmbeddingError::Source { message })
        if message.contains("MissingCompilationContext"))
    );
    // Retaining old preparation explicitly cannot give it authority over a different current product.
    let mut update = Update::new();
    update.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    let candidate = direct.prepare_update(update).unwrap();
    assert!(matches!(
        runtime.replace_program(&program, candidate.program(), Default::default()),
        Err(ReloadValidationError::Runtime(ref error)) if error.kind() == RuntimeErrorKind::StaleHandle
    ));
    assert_eq!(call(&mut runtime, &program, "answer"), 1);
    let current = runtime.program(&program).unwrap();
    let restored = current
        .prepare_replacement(direct.program(), &Default::default())
        .unwrap();
    runtime
        .replace_program(&program, &restored, Default::default())
        .unwrap();
    // The canonical executable keeps its prepared base/history; use the explicit inputs with that exact product.
    direct.program = restored;
    let mut update = Update::new();
    update.replace_function("answer", "pub fn answer() -> i32 { 3 }");
    publish_update(&mut runtime, &program, &mut direct, update).unwrap();
    assert_eq!(call(&mut runtime, &program, "answer"), 3);
}

#[test]
fn generic_alias_retargeting_compares_binders_bounds_and_results_without_instances() {
    let source = include_str!("../../../tests/fixtures/embedding/source-updates/generic-alias.kgr");
    let engine = engine();
    let mut prepared = prepare(&engine, source);
    let mut runtime = engine.runtime(Default::default());
    let program = runtime
        .load_program(prepared.program(), Default::default())
        .unwrap();
    let first = declaration(&runtime, &program, "first");
    let second = declaration(&runtime, &program, "second");
    assert!(
        runtime
            .bind_function::<(i32,), i32>(&program.root(), "alias")
            .is_err()
    );
    let redirected = source.replace("crate::first as alias", "crate::second as alias");
    let mut update = Update::new();
    update.replace_file("app.kgr", &redirected);
    publish_update(&mut runtime, &program, &mut prepared, update).unwrap();
    let token = runtime.publication_token(&program).unwrap();
    for incompatible in ["other_bound", "other_result"] {
        let mut update = Update::new();
        update.replace_file(
            "app.kgr",
            redirected.replace(
                "crate::second as alias",
                &format!("crate::{incompatible} as alias"),
            ),
        );
        let error = publish_update(&mut runtime, &program, &mut prepared, update).unwrap_err();
        assert_eq!(
            error.code(),
            "KG_RELOAD_PUBLIC_EXPORT_MISMATCH",
            "{error:?}"
        );
        assert_eq!(runtime.publication_token(&program).unwrap(), token);
    }
    let mut update = Update::new();
    update.replace_function(
        "alias",
        "pub fn second<U>(x: U) -> i32 where U: Bound { 20 }",
    );
    publish_update(&mut runtime, &program, &mut prepared, update).unwrap();
    assert_eq!(
        text(&runtime, &program, &first),
        "pub fn first<T>(x: T) -> i32 where T: Bound {\n    1\n}"
    );
    assert!(text(&runtime, &program, &second).contains("{ 20 }"));
}

#[test]
fn standard_foundation_accepts_body_updates_and_advances_live_handles_and_source() {
    let engine = engine();
    let mut prepared = engine
        .compile(
            SourceFile::new("app.kgr", "pub fn answer() -> i32 { 1 }"),
            Default::default(),
        )
        .unwrap();
    let mut runtime = engine.runtime(Default::default());
    let program = runtime
        .load_program(prepared.program(), Default::default())
        .unwrap();
    let answer = declaration(&runtime, &program, "answer");
    let function = runtime
        .bind_function::<(), i32>(&program.root(), "answer")
        .unwrap();
    let original = runtime.source_snapshot(&program).unwrap();
    let token = runtime.publication_token(&program).unwrap();
    let mut update = Update::new();
    update.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    publish_update(&mut runtime, &program, &mut prepared, update).unwrap();
    assert_ne!(runtime.publication_token(&program).unwrap(), token);
    assert!(text(&runtime, &program, &answer).contains("{ 2 }"));
    assert!(
        matches!(original.function(&answer), SourceQuery::Available(source) if source.text.contains("{ 1 }"))
    );
    assert_eq!(runtime.call(&function, ()).unwrap(), 2);
    assert_eq!(call(&mut runtime, &program, "answer"), 2);
}

#[test]
fn rejected_initial_publication_releases_its_precommit_capsule() {
    let engine = engine();
    let prepared = prepare(&engine, "pub fn answer() -> i32 { 1 }");
    let capsule = Arc::downgrade(prepared.compilation.as_ref().unwrap());
    let mut runtime = engine.runtime(Default::default());
    let candidate = runtime
        .stage_load_verified_program("rejected", prepared.program().clone().into_verified())
        .unwrap();
    drop(prepared);
    let mut foreign = engine.runtime(Default::default());
    assert!(foreign.publish_staged_load(candidate).is_err());
    runtime.collect_garbage().unwrap();
    assert!(capsule.upgrade().is_none());
    // Abandonment must release both SDK ownership and core admission/storage.
    let prepared = prepare(&engine, "pub fn answer() -> i32 { 2 }");
    let program = runtime
        .load_program(prepared.program(), Default::default())
        .unwrap();
    assert_eq!(call(&mut runtime, &program, "answer"), 2);
}

#[test]
fn stale_publication_releases_candidate_capsule_and_preserves_current_source() {
    let engine = engine();
    let mut prepared = prepare(&engine, "pub fn answer() -> i32 { 1 }");
    let mut runtime = engine.runtime(Default::default());
    let program = runtime
        .load_program(prepared.program(), Default::default())
        .unwrap();
    let answer = declaration(&runtime, &program, "answer");
    let stale = prepare(&engine, "pub fn answer() -> i32 { 99 }");
    let capsule = Arc::downgrade(stale.compilation.as_ref().unwrap());
    let candidate = runtime
        .stage_reload_verified_program(&program, None, stale.program().clone().into_verified())
        .unwrap();
    drop(stale);
    let mut update = Update::new();
    update.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    publish_update(&mut runtime, &program, &mut prepared, update).unwrap();
    let token = runtime.publication_token(&program).unwrap();
    let guard = runtime.begin_publication(&program).unwrap();
    assert!(guard.publish(candidate).is_err());
    runtime.collect_garbage().unwrap();
    assert!(capsule.upgrade().is_none());
    assert_eq!(runtime.publication_token(&program).unwrap(), token);
    assert!(text(&runtime, &program, &answer).contains("{ 2 }"));
    let mut next = Update::new();
    next.replace_function("answer", "pub fn answer() -> i32 { 3 }");
    publish_update(&mut runtime, &program, &mut prepared, next).unwrap();
    assert_eq!(call(&mut runtime, &program, "answer"), 3);
}

#[cfg(feature = "native")]
#[test]
fn source_updates_retain_requested_native_input_in_the_shared_product() {
    let engine = engine();
    let mut prepared = engine
        .prepare(
            &checked_source("pub fn answer() -> i32 { 1 }"),
            PreparationOptions {
                retain_native_input: true,
                ..Default::default()
            },
        )
        .unwrap();
    let mut runtime = engine.runtime(Default::default());
    let program = runtime
        .load_program(prepared.program(), Default::default())
        .unwrap();
    let mut update = Update::new();
    update.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    publish_update(&mut runtime, &program, &mut prepared, update).unwrap();
    let current = runtime.program(&program).unwrap();
    assert!(prepared.has_native_input());
    assert!(
        current
            .bytecode()
            .same_version(prepared.program().bytecode())
    );
    assert_eq!(call(&mut runtime, &program, "answer"), 2);
}
