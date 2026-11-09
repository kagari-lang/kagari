use crate::{
    engine::{
        KagariEngine,
        source::{ArtifactOptions, CheckedModule, PreparationOptions, analysis::SourceAnalysis},
    },
    error::{EmbeddingError, RuntimeFailureKind},
    program::{Program, update::SourceEditTarget},
    runtime::{ReloadOptions, Runtime},
    update::Update,
};
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use kagari_contract::source::AuthoredKind;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_package::{modules::VirtualSources, project::ProjectInputs};
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
use rstest::rstest;
use std::{sync::Arc, thread};

fn engine() -> KagariEngine {
    let mut builder = KagariEngine::builder().unwrap();
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

fn prepare(engine: &KagariEngine, text: &str) -> Program {
    engine
        .prepare(&checked_source(text), Default::default())
        .unwrap()
}

fn call(runtime: &Runtime, program: &InstalledProgram, name: &str) -> i32 {
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

fn edit_target(runtime: &Runtime, program: &InstalledProgram, name: &str) -> SourceEditTarget {
    let base = runtime.program(program).unwrap();
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
    let prepared = engine.prepare(&checked, Default::default()).unwrap();
    let mut runtime = engine.runtime(Default::default());
    let program = runtime.load_program(&prepared, Default::default()).unwrap();
    let mut recipient = engine.runtime(Default::default());
    let recipient_program = recipient
        .load_program(&prepared, Default::default())
        .unwrap();
    let old = runtime.source_snapshot(&program).unwrap();
    let answer = declaration(&runtime, &program, "answer");
    let old_capsule = Arc::downgrade(prepared.state.compilation.as_ref().unwrap());
    drop(prepared);
    assert_eq!(call(&runtime, &program, "bump"), 1);
    assert_eq!(call(&runtime, &program, "answer"), 40);
    let mut update = Update::new();
    update.replace_file("src/./lib.kgr", "mod extra; static var COUNT: i32 = 0; pub fn bump() -> i32 { COUNT += 1; COUNT } pub fn answer() -> i32 { rules::new_value() + extra::bonus() }");
    update.replace_file(
        "rules/src/lib.kgr",
        "pub fn value() -> i32 { 40 } pub fn new_value() -> i32 { 41 }",
    );
    update.add_file(extra, "src/extra.kgr", "pub fn bonus() -> i32 { 1 }");
    let base = runtime.program(&program).unwrap();
    let origin = thread::current().id();
    let preparation = thread::spawn(move || {
        assert_ne!(thread::current().id(), origin);
        base.prepare_update(update).unwrap()
    });
    // The recipient can execute its published version while the worker owns
    // detached preparation. One returned product serves both local publishers.
    assert_eq!(call(&runtime, &program, "answer"), 40);
    let candidate = preparation.join().unwrap();
    runtime
        .replace_program(&program, &candidate, Default::default())
        .unwrap();
    let recipient_before = recipient
        .runtime()
        .publication_token(&recipient_program)
        .unwrap();
    let published_before = runtime.runtime().publication_token(&program).unwrap();
    let local_cancellation = CancellationToken::default();
    local_cancellation.cancel();
    assert!(matches!(
        recipient.replace_program(
            &recipient_program,
            &candidate,
            ReloadOptions {
                cancellation: local_cancellation,
                ..Default::default()
            }
        ),
        Err(EmbeddingError::Cancelled)
    ));
    assert_eq!(
        recipient
            .runtime()
            .publication_token(&recipient_program)
            .unwrap(),
        recipient_before
    );
    assert_eq!(
        runtime.runtime().publication_token(&program).unwrap(),
        published_before
    );
    assert_eq!(call(&recipient, &recipient_program, "answer"), 40);
    recipient
        .replace_program(&recipient_program, &candidate, Default::default())
        .unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 42);
    assert_eq!(call(&runtime, &program, "bump"), 2);
    assert_eq!(call(&recipient, &recipient_program, "answer"), 42);
    assert_eq!(call(&recipient, &recipient_program, "bump"), 1);
    runtime.runtime().collect_garbage().unwrap();
    recipient.runtime().collect_garbage().unwrap();
    assert!(
        old_capsule.upgrade().is_some(),
        "persistent static type provenance retains the initial whole Program (R13)"
    );
    assert!(
        matches!(old.function(&answer), SourceQuery::Available(source) if source.text.contains("rules::value()"))
    );
    let successful = text(&runtime, &program, &answer);
    let published = runtime.runtime().publication_token(&program).unwrap();
    let mut invalid = Update::new();
    invalid.replace_function("answer", "pub fn answer() -> bool { true }");
    assert!(matches!(
        {
            let request = invalid;
            let cancellation = request.cancel.clone();
            runtime
                .program(&program)
                .and_then(|base| base.prepare_update(request))
                .and_then(|candidate| {
                    runtime.replace_program(
                        &program,
                        &candidate,
                        ReloadOptions {
                            cancellation,
                            ..Default::default()
                        },
                    )
                })
        },
        Err(EmbeddingError::ReloadValidation { .. })
    ));
    assert_eq!(
        runtime.runtime().publication_token(&program).unwrap(),
        published
    );
    assert_eq!(text(&runtime, &program, &answer), successful);
    assert_eq!(call(&runtime, &program, "bump"), 3);
    let mut next = Update::new();
    next.replace_function(
        "answer",
        "pub fn answer() -> i32 { rules::new_value() + extra::bonus() + 1 }",
    );
    {
        let request = next;
        let cancellation = request.cancel.clone();
        runtime
            .program(&program)
            .and_then(|base| base.prepare_update(request))
            .and_then(|candidate| {
                runtime.replace_program(
                    &program,
                    &candidate,
                    ReloadOptions {
                        cancellation,
                        ..Default::default()
                    },
                )
            })
    }
    .unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 43);
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
    let prepared = prepare(&engine, source);
    let mut runtime = engine.runtime(Default::default());
    let program = runtime.load_program(&prepared, Default::default()).unwrap();
    let generic = declaration(&runtime, &program, "unused");
    let mut update = Update::new();
    update.replace_function(
        edit_target(&runtime, &program, "hidden"),
        "fn hidden() -> i32 { /* π changes later byte offsets */ 10 }",
    );
    update.replace_function(
        edit_target(&runtime, &program, "capsule_read"),
        "fn capsule_read(self) -> i32 { hidden() + 2 }",
    );
    update.replace_function(
        edit_target(&runtime, &program, "capsule_local"),
        "fn capsule_local() -> i32 { 9 }",
    );
    update.replace_function(
        "generic_alias",
        "pub fn unused<T>(v: T) -> i32 where T: Bound { 6 }",
    );
    {
        let request = update;
        let cancellation = request.cancel.clone();
        runtime
            .program(&program)
            .and_then(|base| base.prepare_update(request))
            .and_then(|candidate| {
                runtime.replace_program(
                    &program,
                    &candidate,
                    ReloadOptions {
                        cancellation,
                        ..Default::default()
                    },
                )
            })
    }
    .unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 12);
    assert_eq!(call(&runtime, &program, "named"), 9);
    assert!(text(&runtime, &program, &generic).contains("{ 6 }"));
    let mut changed_bounds = Update::new();
    changed_bounds.replace_function("generic_alias", "pub fn unused<T>(v: T) -> i32 { 99 }");
    assert!(matches!(
        {
            let request = changed_bounds;
            let cancellation = request.cancel.clone();
            runtime
                .program(&program)
                .and_then(|base| base.prepare_update(request))
                .and_then(|candidate| {
                    runtime.replace_program(
                        &program,
                        &candidate,
                        ReloadOptions {
                            cancellation,
                            ..Default::default()
                        },
                    )
                })
        },
        Err(EmbeddingError::ReloadValidation { .. })
    ));
    assert!(text(&runtime, &program, &generic).contains("where T: Bound { 6 }"));
    let mut next = Update::new();
    next.replace_function(
        edit_target(&runtime, &program, "hidden"),
        "fn hidden() -> i32 { 20 }",
    );
    {
        let request = next;
        let cancellation = request.cancel.clone();
        runtime
            .program(&program)
            .and_then(|base| base.prepare_update(request))
            .and_then(|candidate| {
                runtime.replace_program(
                    &program,
                    &candidate,
                    ReloadOptions {
                        cancellation,
                        ..Default::default()
                    },
                )
            })
    }
    .unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 22);
    assert!(
        runtime
            .function::<(), i32>(&program.root(), "hidden")
            .is_err()
    );
}

#[test]
fn retained_old_closure_demands_recompile_generic_applications_removed_from_new_source_calls() {
    let engine = engine();
    let source = "pub fn retained<T>(value: T) -> i32 { 1 } pub fn factory() -> fn() -> i32 { || retained(7i32) }";
    let prepared = prepare(&engine, source);
    let mut runtime = engine.runtime(Default::default());
    let program = runtime.load_program(&prepared, Default::default()).unwrap();
    let factory = runtime
        .function::<(), Function<(), i32>>(&program.root(), "factory")
        .unwrap();
    let old_closure = runtime.call(&factory, ()).unwrap();
    assert_eq!(runtime.call(&old_closure, ()).unwrap(), 1);
    let mut update = Update::new();
    update.replace_function("retained", "pub fn retained<T>(value: T) -> i32 { 2 }");
    update.replace_function("factory", "pub fn factory() -> fn() -> i32 { || 100 }");
    {
        let request = update;
        let cancellation = request.cancel.clone();
        runtime
            .program(&program)
            .and_then(|base| base.prepare_update(request))
            .and_then(|candidate| {
                runtime.replace_program(
                    &program,
                    &candidate,
                    ReloadOptions {
                        cancellation,
                        ..Default::default()
                    },
                )
            })
    }
    .unwrap();
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.call(&old_closure, ()).unwrap(), 2);
    let fresh = runtime.call(&factory, ()).unwrap();
    assert_eq!(runtime.call(&fresh, ()).unwrap(), 100);
}

#[test]
fn malformed_duplicate_overlapping_and_foreign_updates_leave_source_unchanged() {
    let engine = engine();
    let source =
        include_str!("../../../tests/fixtures/embedding/source-updates/invalid-batches.kgr");
    let prepared = prepare(&engine, source);
    let mut runtime = engine.runtime(Default::default());
    let program = runtime.load_program(&prepared, Default::default()).unwrap();
    let answer = declaration(&runtime, &program, "answer");
    let nested = edit_target(&runtime, &program, "nested_target");
    let structure = edit_target(&runtime, &program, "NotAFunction");
    assert_eq!(structure.kind(), AuthoredKind::Declaration);
    let mut foreign_runtime = engine.runtime(Default::default());
    let foreign_program = foreign_runtime
        .load_program(&prepare(&engine, source), Default::default())
        .unwrap();
    let foreign = edit_target(&foreign_runtime, &foreign_program, "answer");
    let token = runtime.runtime().publication_token(&program).unwrap();
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
        .replace_function(edit_target(&runtime, &program, "answer"), source);
    failures.push(overlap);
    let mut nested_overlap = Update::new();
    nested_overlap
        .replace_function(edit_target(&runtime, &program, "answer"), source)
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
        assert!(
            {
                let request = update;
                let cancellation = request.cancel.clone();
                runtime
                    .program(&program)
                    .and_then(|base| base.prepare_update(request))
                    .and_then(|candidate| {
                        runtime.replace_program(
                            &program,
                            &candidate,
                            ReloadOptions {
                                cancellation,
                                ..Default::default()
                            },
                        )
                    })
            }
            .is_err()
        );
        assert_eq!(
            runtime.runtime().publication_token(&program).unwrap(),
            token
        );
        assert_eq!(call(&runtime, &program, "answer"), 1);
        assert!(text(&runtime, &program, &answer).contains("nested_target()"));
    }
    let mut valid = Update::new();
    valid.replace_function(
        "alias",
        "pub fn answer() -> i32 { fn nested_target() -> i32 { 1 } nested_target() + 1 }",
    );
    {
        let request = valid;
        let cancellation = request.cancel.clone();
        runtime
            .program(&program)
            .and_then(|base| base.prepare_update(request))
            .and_then(|candidate| {
                runtime.replace_program(
                    &program,
                    &candidate,
                    ReloadOptions {
                        cancellation,
                        ..Default::default()
                    },
                )
            })
    }
    .unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 2);
}

#[test]
fn admission_bounds_and_request_cancellation_precede_compilation() {
    let engine = engine();
    let cancelled_preparation = CancellationToken::default();
    let mut options = PreparationOptions::default();
    options.lowering.cancel = cancelled_preparation.clone();
    let prepared = engine
        .prepare(&checked_source("pub fn answer() -> i32 { 1 }"), options)
        .unwrap();
    cancelled_preparation.cancel();
    let mut runtime = engine.runtime(Default::default());
    let program = runtime.load_program(&prepared, Default::default()).unwrap();
    let guard = runtime.runtime_mut().begin_publication(&program).unwrap();
    let mut detached = Update::new();
    detached.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    let candidate = prepared.prepare_update(detached).unwrap();
    assert!(candidate.expects_base(&prepared));
    drop(guard);
    drop(candidate);
    let mut excessive = Update::new();
    for _ in 0..1025 {
        excessive.replace_file("app.kgr", "");
    }
    assert!(
        matches!({ let request = excessive; let cancellation = request.cancel.clone(); runtime.program(&program).and_then(|base| base.prepare_update(request)).and_then(|candidate| runtime.replace_program(&program, &candidate, ReloadOptions { cancellation, ..Default::default() })) }, Err(EmbeddingError::Source { message }) if message.contains("1024"))
    );
    let mut oversized = Update::new();
    oversized.replace_file("app.kgr", " ".repeat(64 * 1024 * 1024 + 1));
    assert!(
        matches!({ let request = oversized; let cancellation = request.cancel.clone(); runtime.program(&program).and_then(|base| base.prepare_update(request)).and_then(|candidate| runtime.replace_program(&program, &candidate, ReloadOptions { cancellation, ..Default::default() })) }, Err(EmbeddingError::Source { message }) if message.contains("64 MiB"))
    );
    let request = CancellationToken::default();
    request.cancel();
    let mut cancelled = Update::new().with_cancellation(request);
    cancelled.replace_function("answer", "pub fn answer() -> i32 { 9 }");
    assert!(matches!(
        {
            let request = cancelled;
            let cancellation = request.cancel.clone();
            runtime
                .program(&program)
                .and_then(|base| base.prepare_update(request))
                .and_then(|candidate| {
                    runtime.replace_program(
                        &program,
                        &candidate,
                        ReloadOptions {
                            cancellation,
                            ..Default::default()
                        },
                    )
                })
        },
        Err(EmbeddingError::Cancelled)
    ));
    assert_eq!(call(&runtime, &program, "answer"), 1);
    let mut fresh = Update::new();
    fresh.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    {
        let request = fresh;
        let cancellation = request.cancel.clone();
        runtime
            .program(&program)
            .and_then(|base| base.prepare_update(request))
            .and_then(|candidate| {
                runtime.replace_program(
                    &program,
                    &candidate,
                    ReloadOptions {
                        cancellation,
                        ..Default::default()
                    },
                )
            })
    }
    .unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 2);
}

#[rstest]
#[case::sdk_reload(false)]
#[case::core_reload(true)]
fn artifact_source_is_opt_in_and_low_level_or_artifact_reload_never_reuses_old_capsules(
    #[case] low_level: bool,
) {
    let engine = engine();
    let checked = checked_source("pub fn answer() -> i32 { 1 }");
    let direct = engine.prepare(&checked, Default::default()).unwrap();
    assert!(direct.bytecode().source_package().is_some());
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
    let artifact = Program::from_artifact(
        crate::BytecodeArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(artifact.bytecode().source_package().is_some());
    let mut runtime = engine.runtime(Default::default());
    let program = runtime.load_program(&direct, Default::default()).unwrap();
    if low_level {
        let candidate = runtime
            .runtime()
            .stage_reload_verified_program(&program, None, artifact.bytecode().clone())
            .unwrap();
        runtime
            .runtime_mut()
            .publish_staged_reload(candidate)
            .unwrap();
    } else {
        runtime
            .program(&program)
            .and_then(|base| base.prepare_replacement(&artifact, &Default::default()))
            .and_then(|candidate| runtime.replace_program(&program, &candidate, Default::default()))
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
    let error = runtime
        .program(&program)
        .and_then(|base| base.prepare_update(update))
        .unwrap_err();
    if low_level {
        assert!(
            matches!(error, EmbeddingError::Runtime { kind: RuntimeFailureKind::UnsupportedExecution, message, .. } if message.contains("MissingCompilationContext"))
        );
    } else {
        assert!(
            matches!(error, EmbeddingError::Source { message } if message.contains("MissingCompilationContext"))
        );
    }
    assert_eq!(call(&runtime, &program, "answer"), 1);
    if low_level {
        let candidate = runtime
            .runtime()
            .stage_reload_verified_program(&program, None, direct.clone().into_verified())
            .unwrap();
        runtime
            .runtime_mut()
            .publish_staged_reload(candidate)
            .unwrap();
    } else {
        let base = runtime.program(&program).unwrap();
        let candidate = base
            .prepare_replacement(&direct, &Default::default())
            .unwrap();
        runtime
            .replace_program(&program, &candidate, Default::default())
            .unwrap();
    }
    let mut update = Update::new();
    update.replace_function("answer", "pub fn answer() -> i32 { 3 }");
    {
        let request = update;
        let cancellation = request.cancel.clone();
        runtime
            .program(&program)
            .and_then(|base| base.prepare_update(request))
            .and_then(|candidate| {
                runtime.replace_program(
                    &program,
                    &candidate,
                    ReloadOptions {
                        cancellation,
                        ..Default::default()
                    },
                )
            })
    }
    .unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 3);
}

#[test]
fn generic_alias_retargeting_compares_binders_bounds_and_results_without_instances() {
    let source = include_str!("../../../tests/fixtures/embedding/source-updates/generic-alias.kgr");
    let engine = engine();
    let prepared = prepare(&engine, source);
    let mut runtime = engine.runtime(Default::default());
    let program = runtime.load_program(&prepared, Default::default()).unwrap();
    let first = declaration(&runtime, &program, "first");
    let second = declaration(&runtime, &program, "second");
    assert!(
        runtime
            .function::<(i32,), i32>(&program.root(), "alias")
            .is_err()
    );
    let redirected = source.replace("crate::first as alias", "crate::second as alias");
    let mut update = Update::new();
    update.replace_file("app.kgr", &redirected);
    {
        let request = update;
        let cancellation = request.cancel.clone();
        runtime
            .program(&program)
            .and_then(|base| base.prepare_update(request))
            .and_then(|candidate| {
                runtime.replace_program(
                    &program,
                    &candidate,
                    ReloadOptions {
                        cancellation,
                        ..Default::default()
                    },
                )
            })
    }
    .unwrap();
    let token = runtime.runtime().publication_token(&program).unwrap();
    for incompatible in ["other_bound", "other_result"] {
        let mut update = Update::new();
        update.replace_file(
            "app.kgr",
            redirected.replace(
                "crate::second as alias",
                &format!("crate::{incompatible} as alias"),
            ),
        );
        let error = {
            let request = update;
            let cancellation = request.cancel.clone();
            runtime
                .program(&program)
                .and_then(|base| base.prepare_update(request))
                .and_then(|candidate| {
                    runtime.replace_program(
                        &program,
                        &candidate,
                        ReloadOptions {
                            cancellation,
                            ..Default::default()
                        },
                    )
                })
        }
        .unwrap_err();
        assert_eq!(
            error.code(),
            "KG_RELOAD_PUBLIC_EXPORT_MISMATCH",
            "{error:?}"
        );
        assert_eq!(
            runtime.runtime().publication_token(&program).unwrap(),
            token
        );
    }
    let mut update = Update::new();
    update.replace_function(
        "alias",
        "pub fn second<U>(x: U) -> i32 where U: Bound { 20 }",
    );
    {
        let request = update;
        let cancellation = request.cancel.clone();
        runtime
            .program(&program)
            .and_then(|base| base.prepare_update(request))
            .and_then(|candidate| {
                runtime.replace_program(
                    &program,
                    &candidate,
                    ReloadOptions {
                        cancellation,
                        ..Default::default()
                    },
                )
            })
    }
    .unwrap();
    assert_eq!(
        text(&runtime, &program, &first),
        "pub fn first<T>(x: T) -> i32 where T: Bound {\n    1\n}"
    );
    assert!(text(&runtime, &program, &second).contains("{ 20 }"));
}

#[test]
fn standard_foundation_accepts_body_updates_and_advances_live_handles_and_source() {
    let engine = engine();
    let prepared = engine
        .compile_to_program(
            SourceFile::new("app.kgr", "pub fn answer() -> i32 { 1 }"),
            Default::default(),
        )
        .unwrap();
    let mut runtime = engine.runtime(Default::default());
    let program = runtime.load_program(&prepared, Default::default()).unwrap();
    let answer = declaration(&runtime, &program, "answer");
    let function = runtime
        .function::<(), i32>(&program.root(), "answer")
        .unwrap();
    let original = runtime.source_snapshot(&program).unwrap();
    let token = runtime.runtime().publication_token(&program).unwrap();
    let mut update = Update::new();
    update.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    {
        let request = update;
        let cancellation = request.cancel.clone();
        runtime
            .program(&program)
            .and_then(|base| base.prepare_update(request))
            .and_then(|candidate| {
                runtime.replace_program(
                    &program,
                    &candidate,
                    ReloadOptions {
                        cancellation,
                        ..Default::default()
                    },
                )
            })
    }
    .unwrap();
    assert_ne!(
        runtime.runtime().publication_token(&program).unwrap(),
        token
    );
    assert!(text(&runtime, &program, &answer).contains("{ 2 }"));
    assert!(
        matches!(original.function(&answer), SourceQuery::Available(source) if source.text.contains("{ 1 }"))
    );
    assert_eq!(runtime.call(&function, ()).unwrap(), 2);
    assert_eq!(call(&runtime, &program, "answer"), 2);
}

#[test]
fn rejected_initial_publication_releases_its_precommit_capsule() {
    let engine = engine();
    let prepared = prepare(&engine, "pub fn answer() -> i32 { 1 }");
    let capsule = Arc::downgrade(prepared.state.compilation.as_ref().unwrap());
    let mut runtime = engine.runtime(Default::default());
    let candidate = runtime
        .runtime_mut()
        .stage_load_verified_program("rejected", prepared.clone().into_verified())
        .unwrap();
    drop(prepared);
    let mut foreign = engine.runtime(Default::default());
    assert!(
        foreign
            .runtime_mut()
            .publish_staged_load(candidate)
            .is_err()
    );
    runtime.runtime().collect_garbage().unwrap();
    assert!(capsule.upgrade().is_none());
    // Abandonment must release both SDK ownership and core admission/storage.
    let prepared = prepare(&engine, "pub fn answer() -> i32 { 2 }");
    let program = runtime.load_program(&prepared, Default::default()).unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 2);
}

#[test]
fn stale_publication_releases_candidate_capsule_and_preserves_current_source() {
    let engine = engine();
    let prepared = prepare(&engine, "pub fn answer() -> i32 { 1 }");
    let mut runtime = engine.runtime(Default::default());
    let program = runtime.load_program(&prepared, Default::default()).unwrap();
    let answer = declaration(&runtime, &program, "answer");
    let stale = prepare(&engine, "pub fn answer() -> i32 { 99 }");
    let capsule = Arc::downgrade(stale.state.compilation.as_ref().unwrap());
    let candidate = runtime
        .runtime()
        .stage_reload_verified_program(&program, None, stale.clone().into_verified())
        .unwrap();
    drop(stale);
    let mut update = Update::new();
    update.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    {
        let request = update;
        let cancellation = request.cancel.clone();
        runtime
            .program(&program)
            .and_then(|base| base.prepare_update(request))
            .and_then(|candidate| {
                runtime.replace_program(
                    &program,
                    &candidate,
                    ReloadOptions {
                        cancellation,
                        ..Default::default()
                    },
                )
            })
    }
    .unwrap();
    let token = runtime.runtime().publication_token(&program).unwrap();
    let guard = runtime.runtime_mut().begin_publication(&program).unwrap();
    assert!(guard.publish(candidate).is_err());
    runtime.runtime().collect_garbage().unwrap();
    assert!(capsule.upgrade().is_none());
    assert_eq!(
        runtime.runtime().publication_token(&program).unwrap(),
        token
    );
    assert!(text(&runtime, &program, &answer).contains("{ 2 }"));
    let mut next = Update::new();
    next.replace_function("answer", "pub fn answer() -> i32 { 3 }");
    {
        let request = next;
        let cancellation = request.cancel.clone();
        runtime
            .program(&program)
            .and_then(|base| base.prepare_update(request))
            .and_then(|candidate| {
                runtime.replace_program(
                    &program,
                    &candidate,
                    ReloadOptions {
                        cancellation,
                        ..Default::default()
                    },
                )
            })
    }
    .unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 3);
}

#[cfg(feature = "native")]
#[test]
fn source_updates_retain_requested_native_input_in_the_shared_product() {
    let engine = engine();
    let prepared = engine
        .prepare(
            &checked_source("pub fn answer() -> i32 { 1 }"),
            PreparationOptions {
                retain_native_input: true,
                ..Default::default()
            },
        )
        .unwrap();
    let mut runtime = engine.runtime(Default::default());
    let program = runtime.load_program(&prepared, Default::default()).unwrap();
    drop(prepared);
    let mut update = Update::new();
    update.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    {
        let request = update;
        let cancellation = request.cancel.clone();
        runtime
            .program(&program)
            .and_then(|base| base.prepare_update(request))
            .and_then(|candidate| {
                runtime.replace_program(
                    &program,
                    &candidate,
                    ReloadOptions {
                        cancellation,
                        ..Default::default()
                    },
                )
            })
    }
    .unwrap();
    let current = runtime.program(&program).unwrap();
    assert!(current.has_native_input());
    assert_eq!(call(&runtime, &program, "answer"), 2);
}
