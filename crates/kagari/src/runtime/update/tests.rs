use crate::{
    engine::{
        KagariEngine,
        source::{ArtifactOptions, CheckedModule, PreparationOptions, analysis::SourceAnalysis},
    },
    error::{EmbeddingError, RuntimeFailureKind},
    program::PreparedProgram,
    runtime::Runtime,
    update::Update,
};
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use kagari_contract::source::AuthoredKind;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_package::{modules::VirtualSources, project::ProjectInputs};
use kagari_runtime::{
    module::Program,
    native::function_handle::Function,
    source::{SourceDeclaration, SourceQuery},
};
use kagari_source::{
    identity::FileId,
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer, SourceSnapshot},
};
use std::sync::Arc;

fn engine() -> KagariEngine {
    let mut builder = KagariEngine::builder().unwrap();
    builder.source_root("/source-updates").unwrap();
    builder.build().unwrap()
}

fn check(sources: SourceSnapshot, root: FileId) -> CheckedModule {
    // Ordinary patch tests need a finite provider closure. The separate foundation
    // control keeps the standard library's unknown protected edges observable.
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

fn prepare(engine: &KagariEngine, text: &str) -> PreparedProgram {
    engine
        .prepare(&checked_source(text), Default::default())
        .unwrap()
}

fn call(runtime: &Runtime, program: &Program, name: &str) -> i32 {
    runtime
        .execute_typed::<(), i32>(program, name, (), &Default::default())
        .unwrap()
}

fn declaration(runtime: &Runtime, program: &Program, name: &str) -> SourceDeclaration {
    let snapshot = runtime.source_snapshot(program).unwrap();
    let matches = snapshot
        .declarations()
        .iter()
        .filter(|item| item.name() == name)
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "ambiguous test declaration {name}");
    matches[0].clone()
}

fn text(runtime: &Runtime, program: &Program, declaration: &SourceDeclaration) -> String {
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
    let old = runtime.source_snapshot(&program).unwrap();
    let answer = declaration(&runtime, &program, "answer");
    let token = runtime.runtime().publication_token(&program).unwrap();
    let retained = runtime.publications.current(token).unwrap();
    let old_capsule = Arc::downgrade(retained.compilation.as_ref().unwrap());
    drop(retained);
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
    runtime.update(&program, update).unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 42);
    assert_eq!(call(&runtime, &program, "bump"), 2);
    assert!(
        old_capsule.upgrade().is_none(),
        "publication must not accumulate compilation capsules"
    );
    assert!(
        matches!(old.function(&answer), SourceQuery::Available(source) if source.text.contains("rules::value()"))
    );
    let successful = text(&runtime, &program, &answer);
    let published = runtime.runtime().publication_token(&program).unwrap();
    let mut invalid = Update::new();
    invalid.replace_function("answer", "pub fn answer() -> bool { true }");
    assert!(matches!(
        runtime.update(&program, invalid),
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
    runtime.update(&program, next).unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 43);
    assert_eq!(
        captured.module(checked.module_identity()).unwrap().text(),
        root
    );
}

#[test]
fn opaque_private_method_and_named_local_targets_preserve_context_and_locator_identity() {
    let source = "pub trait Bound {} fn hidden() -> i32 { 1 } struct CapsuleItem {} impl CapsuleItem { fn capsule_read(self) -> i32 { hidden() } } pub fn answer() -> i32 { CapsuleItem {}.capsule_read() } pub fn named() -> i32 { fn capsule_local() -> i32 { 3 } capsule_local() } mod inner { use crate::Bound; pub fn unused<T>(v: T) -> i32 where T: Bound { 5 } } pub use crate::inner::unused as generic_alias;";
    let engine = engine();
    let prepared = prepare(&engine, source);
    let mut runtime = engine.runtime(Default::default());
    let program = runtime.load_program(&prepared, Default::default()).unwrap();
    let hidden = declaration(&runtime, &program, "hidden");
    let method = declaration(&runtime, &program, "capsule_read");
    let local = declaration(&runtime, &program, "capsule_local");
    let generic = declaration(&runtime, &program, "unused");
    let mut update = Update::new();
    update.replace_function(
        &hidden,
        "fn hidden() -> i32 { /* π changes later byte offsets */ 10 }",
    );
    update.replace_function(&method, "fn capsule_read(self) -> i32 { hidden() + 2 }");
    update.replace_function(&local, "fn capsule_local() -> i32 { 9 }");
    update.replace_function(
        "generic_alias",
        "pub fn unused<T>(v: T) -> i32 where T: Bound { 6 }",
    );
    runtime.update(&program, update).unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 12);
    assert_eq!(call(&runtime, &program, "named"), 9);
    assert!(text(&runtime, &program, &generic).contains("{ 6 }"));
    let mut changed_bounds = Update::new();
    changed_bounds.replace_function("generic_alias", "pub fn unused<T>(v: T) -> i32 { 99 }");
    assert!(matches!(
        runtime.update(&program, changed_bounds),
        Err(EmbeddingError::ReloadValidation { .. })
    ));
    assert!(text(&runtime, &program, &generic).contains("where T: Bound { 6 }"));
    let mut next = Update::new();
    next.replace_function(hidden, "fn hidden() -> i32 { 20 }");
    runtime.update(&program, next).unwrap();
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
    runtime.update(&program, update).unwrap();
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.call(&old_closure, ()).unwrap(), 2);
    let fresh = runtime.call(&factory, ()).unwrap();
    assert_eq!(runtime.call(&fresh, ()).unwrap(), 100);
}

#[test]
fn malformed_duplicate_overlapping_and_foreign_updates_leave_source_unchanged() {
    let engine = engine();
    let source = "pub fn answer() -> i32 { fn nested_target() -> i32 { 1 } nested_target() } pub use crate::answer as alias; struct NotAFunction {}";
    let prepared = prepare(&engine, source);
    let mut runtime = engine.runtime(Default::default());
    let program = runtime.load_program(&prepared, Default::default()).unwrap();
    let answer = declaration(&runtime, &program, "answer");
    let nested = declaration(&runtime, &program, "nested_target");
    let structure = declaration(&runtime, &program, "NotAFunction");
    assert_eq!(structure.kind(), Some(AuthoredKind::Declaration));
    let mut foreign_runtime = engine.runtime(Default::default());
    let foreign_program = foreign_runtime
        .load_program(&prepared, Default::default())
        .unwrap();
    let foreign = declaration(&foreign_runtime, &foreign_program, "answer");
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
        .replace_function(&answer, source);
    failures.push(overlap);
    let mut nested_overlap = Update::new();
    nested_overlap
        .replace_function(&answer, source)
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
        assert!(runtime.update(&program, update).is_err());
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
    runtime.update(&program, valid).unwrap();
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
    let guard = runtime.runtime().begin_publication(&program).unwrap();
    assert!(matches!(
        runtime.update(&program, Update::new()),
        Err(EmbeddingError::Runtime {
            kind: RuntimeFailureKind::ExecutionPhaseViolation,
            ..
        })
    ));
    drop(guard);
    let mut excessive = Update::new();
    for _ in 0..1025 {
        excessive.replace_file("app.kgr", "");
    }
    assert!(
        matches!(runtime.update(&program, excessive), Err(EmbeddingError::Source { message }) if message.contains("1024"))
    );
    let mut oversized = Update::new();
    oversized.replace_file("app.kgr", " ".repeat(64 * 1024 * 1024 + 1));
    assert!(
        matches!(runtime.update(&program, oversized), Err(EmbeddingError::Source { message }) if message.contains("64 MiB"))
    );
    let request = CancellationToken::default();
    request.cancel();
    let mut cancelled = Update::new().with_cancellation(request);
    cancelled.replace_function("answer", "pub fn answer() -> i32 { 9 }");
    assert!(matches!(
        runtime.update(&program, cancelled),
        Err(EmbeddingError::Cancelled)
    ));
    assert_eq!(call(&runtime, &program, "answer"), 1);
    let mut fresh = Update::new();
    fresh.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    runtime.update(&program, fresh).unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 2);
}

#[test]
fn artifact_source_is_opt_in_and_low_level_or_artifact_reload_never_reuses_old_capsules() {
    let engine = engine();
    let checked = checked_source("pub fn answer() -> i32 { 1 }");
    let direct = engine.prepare(&checked, Default::default()).unwrap();
    assert!(direct.bytecode().source_package().is_some());
    let stripped = engine.emit_artifact(&checked, Default::default()).unwrap();
    let stripped =
        PreparedProgram::from_artifact(stripped, &Default::default(), &Default::default()).unwrap();
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
    let artifact = PreparedProgram::from_artifact(
        crate::BytecodeArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(artifact.bytecode().source_package().is_some());
    for low_level in [false, true] {
        let mut runtime = engine.runtime(Default::default());
        let program = runtime.load_program(&direct, Default::default()).unwrap();
        if low_level {
            let candidate = runtime
                .runtime()
                .stage_reload_verified_program(&program, None, artifact.bytecode().clone())
                .unwrap();
            runtime.runtime().publish_staged_reload(candidate).unwrap();
        } else {
            runtime
                .reload_program(&program, &artifact, Default::default())
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
            matches!(runtime.update(&program, update), Err(EmbeddingError::Source { message }) if message.contains("MissingCompilationContext"))
        );
        assert_eq!(call(&runtime, &program, "answer"), 1);
        runtime
            .reload_program(&program, &direct, Default::default())
            .unwrap();
        let mut update = Update::new();
        update.replace_function("answer", "pub fn answer() -> i32 { 3 }");
        runtime.update(&program, update).unwrap();
        assert_eq!(call(&runtime, &program, "answer"), 3);
    }
}

#[test]
fn generic_alias_retargeting_compares_binders_bounds_and_results_without_instances() {
    let source = "pub trait Bound {} pub trait Other {} pub fn first<T>(x: T) -> i32 where T: Bound { 1 } pub fn second<U>(x: U) -> i32 where U: Bound { 2 } pub fn other_bound<V>(x: V) -> i32 where V: Other { 3 } pub fn other_result<W>(x: W) -> bool where W: Bound { true } pub fn answer() -> i32 { 1 } pub use crate::first as alias;";
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
    runtime.update(&program, update).unwrap();
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
        let error = runtime.update(&program, update).unwrap_err();
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
    runtime.update(&program, update).unwrap();
    assert!(text(&runtime, &program, &first).contains("{ 1 }"));
    assert!(text(&runtime, &program, &second).contains("{ 20 }"));
}

#[test]
fn standard_foundation_protection_rejects_body_updates_without_changing_source() {
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
    let token = runtime.runtime().publication_token(&program).unwrap();
    let mut update = Update::new();
    update.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    let error = runtime.update(&program, update).unwrap_err();
    assert!(
        matches!(error, EmbeddingError::ReloadValidation { ref message, .. }
        if message.contains("protected declaration") && message.contains("unknown dependency edge")),
        "{error:?}"
    );
    assert_eq!(
        runtime.runtime().publication_token(&program).unwrap(),
        token
    );
    assert!(text(&runtime, &program, &answer).contains("{ 1 }"));
    assert_eq!(call(&runtime, &program, "answer"), 1);
}

#[test]
fn rejected_initial_publication_releases_its_precommit_capsule() {
    let engine = engine();
    let prepared = prepare(&engine, "pub fn answer() -> i32 { 1 }");
    let retained = prepared.retained_inputs();
    let capsule = Arc::downgrade(retained.compilation.as_ref().unwrap());
    let mut runtime = engine.runtime(Default::default());
    let candidate = runtime
        .runtime_mut()
        .stage_load_verified_program("rejected", prepared.bytecode().clone())
        .unwrap();
    drop(prepared);
    let association = runtime
        .publications
        .stage(candidate.publication_token(), retained)
        .unwrap();
    let mut foreign = engine.runtime(Default::default());
    assert!(
        foreign
            .runtime_mut()
            .publish_staged_load(candidate)
            .is_err()
    );
    drop(association);
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
    let retained = stale.retained_inputs();
    let capsule = Arc::downgrade(retained.compilation.as_ref().unwrap());
    let candidate = runtime
        .runtime()
        .stage_reload_verified_program(&program, None, stale.bytecode().clone())
        .unwrap();
    drop(stale);
    let mut update = Update::new();
    update.replace_function("answer", "pub fn answer() -> i32 { 2 }");
    runtime.update(&program, update).unwrap();
    let token = runtime.runtime().publication_token(&program).unwrap();
    let guard = runtime.runtime().begin_publication(&program).unwrap();
    let association = runtime
        .publications
        .stage(candidate.publication_token(), retained)
        .unwrap();
    assert!(guard.publish(candidate).is_err());
    drop(association);
    assert!(capsule.upgrade().is_none());
    assert_eq!(
        runtime.runtime().publication_token(&program).unwrap(),
        token
    );
    assert!(text(&runtime, &program, &answer).contains("{ 2 }"));
    let mut next = Update::new();
    next.replace_function("answer", "pub fn answer() -> i32 { 3 }");
    runtime.update(&program, next).unwrap();
    assert_eq!(call(&runtime, &program, "answer"), 3);
}

#[cfg(feature = "native")]
#[test]
fn source_updates_retain_requested_native_input_without_retaining_prepared_caches() {
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
    runtime.update(&program, update).unwrap();
    let current = runtime.prepared_program(&program).unwrap();
    assert!(current.has_native_input());
    assert_eq!(call(&runtime, &program, "answer"), 2);
}
