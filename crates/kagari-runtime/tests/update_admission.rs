//! Protected publication and historical-demand contracts, independent of execution liveness.
use kagari_common::identity::map::DefinitionContext;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::updates::{BodyRecord, CallRequirement, RootKind};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime,
    module::{LoadedModule, VerifiedProgram, state::StaticState},
    native::module::NativeModule,
    reload::{
        ReloadValidationError, validate_reload_artifact_candidate, validate_reload_candidate,
    },
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::{catalog, modules};
use programs::{
    bytecode, source_code, source_free_artifact, source_free_program_results,
    source_free_reload_results,
};
use std::sync::Arc;

#[path = "support/update_programs.rs"]
mod programs;

fn rejected_without_effects(runtime: &Runtime, loaded: &LoadedModule, candidate: &str) {
    let history = loaded.retained_demands().metadata().records().clone();
    let count = runtime.modules().loaded_count();
    let code = bytecode(candidate, None);
    let artifact = source_free_artifact(code.clone());
    let verified = VerifiedProgram::new(code.clone(), &DefinitionContext::new().unwrap()).unwrap();
    for result in source_free_reload_results(loaded, candidate)
        .into_iter()
        .chain([
            runtime
                .stage_reload_program(loaded, "exports", code)
                .map(|_| ()),
            runtime
                .stage_reload_artifact(loaded, "exports", artifact, &Default::default())
                .map(|_| ()),
            runtime
                .stage_reload_verified_program(loaded, "exports", verified)
                .map(|_| ()),
        ])
    {
        assert!(
            matches!(result, Err(ReloadValidationError::UpdateEvidence(_))),
            "{result:?}"
        );
    }
    let current = runtime.modules().latest("exports").unwrap();
    assert_eq!(current.key(), loaded.key());
    assert_eq!(current.retained_demands().metadata().records(), &history);
    assert_eq!(runtime.modules().loaded_count(), count);
    assert_eq!(runtime.resources().counters().peak_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
    for member in loaded.members() {
        assert!(
            runtime
                .module_instance_snapshot(&member)
                .unwrap()
                .module_slots
                .iter()
                .all(|slot| matches!(slot, StaticState::Uninitialized(_)))
        );
    }
}

#[test]
fn protected_lazy_initializer_freezes_expression_and_transitive_helper_before_use() {
    let source = r#"
        fn leaf() -> i32 { 3 }
        fn helper() -> i32 { leaf() }
        static var STATE: i32 = helper() + (1 + 2);
        pub fn run() -> i32 { 7 }
        fn main() {}
    "#;
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    assert!(
        loaded
            .verified_program()
            .update_evidence()
            .roots
            .iter()
            .any(|root| root.kind == RootKind::StaticInitializer)
    );
    for candidate in [
        source.replace("leaf() -> i32 { 3 }", "leaf() -> i32 { 4 }"),
        source.replace("helper() + (1 + 2)", "helper() + 3"),
    ] {
        rejected_without_effects(&runtime, &loaded, &candidate);
    }
    // A genuinely unrelated change is allowed when the protected closure is known.
    let candidate = source
        .replace("run() -> i32 { 7 }", "run() -> i32 { 8 }")
        .replace(
            "helper() + (1 + 2)",
            "helper() /* unchanged tokens */ + ( 1 + 2 )",
        );
    let staged = runtime
        .stage_reload_verified_program(&loaded, "exports", source_code(&candidate))
        .unwrap();
    let published = runtime.publish_staged_reload(staged).unwrap();
    assert!(
        runtime
            .module_instance_snapshot(&published)
            .unwrap()
            .module_slots
            .iter()
            .all(|slot| matches!(slot, StaticState::Uninitialized(_)))
    );
}

#[test]
fn protected_unknown_function_value_rejects_executable_additions() {
    let source = r#"
        fn leaf() -> i32 { 3 }
        fn helper() -> i32 { val call: fn() -> i32 = || leaf(); call() }
        static var STATE: i32 = helper();
        fn main() {}
    "#;
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    rejected_without_effects(
        &runtime,
        &loaded,
        &format!("{source} fn added() -> i32 {{ 9 }}"),
    );
}

#[test]
fn historical_demands_survive_old_code_collection_and_failed_candidates() {
    let source = "pub fn helper() -> i32 { 1 } pub fn run() -> i32 { helper() } fn main() {}";
    let mut runtime = Runtime::default();
    let old = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    let old_key = old.key();
    let code = old
        .verified_program()
        .modules()
        .iter()
        .map(Arc::downgrade)
        .collect::<Vec<_>>();
    let replacement = source.replace("run() -> i32 { helper() }", "run() -> i32 { 2 }");
    let staged = runtime
        .stage_reload_verified_program(&old, "exports", source_code(&replacement))
        .unwrap();
    let current = runtime.publish_staged_reload(staged).unwrap();
    drop(old);
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(old_key).is_none());
    assert!(code.iter().all(|module| module.upgrade().is_none()));
    let baseline = current.retained_demands().metadata().records().clone();
    let abandoned_source = format!("{replacement} pub fn abandoned_only() -> i32 {{ helper() }}");
    let abandoned = runtime
        .stage_reload_verified_program(&current, "exports", source_code(&abandoned_source))
        .unwrap();
    drop(abandoned);
    assert_eq!(current.retained_demands().metadata().records(), &baseline);
    let stale_source = format!("{replacement} pub fn stale_only() -> i32 {{ helper() }}");
    let stale = runtime
        .stage_reload_verified_program(&current, "exports", source_code(&stale_source))
        .unwrap();
    let next = replacement.replace("run() -> i32 { 2 }", "run() -> i32 { 3 }");
    let staged = runtime
        .stage_reload_verified_program(&current, "exports", source_code(&next))
        .unwrap();
    let latest = runtime.publish_staged_reload(staged).unwrap();
    assert!(matches!(
        runtime.publish_staged_reload(stale),
        Err(ReloadValidationError::ModuleNotActive { .. })
    ));
    let history = latest.retained_demands().metadata();
    assert!(!history.records().bodies.iter().any(|body| {
        history
            .definitions()
            .resolve(body.origin.declaration)
            .ok()
            .and_then(|view| view.last())
            .is_some_and(|part| matches!(part.name, "abandoned_only" | "stale_only"))
    }));
    assert!(history.records().bodies.iter().any(|body| body.calls.iter().any(|call| {
        matches!(call, CallRequirement::Direct(target) if history.definitions().resolve(target.declaration)
            .ok().and_then(|view| view.last()).is_some_and(|part| part.name == "helper"))
    })));
    let baseline = history.records().clone();
    let incompatible = next
        .replace("helper() -> i32", "helper() -> i64")
        .replace("{ 1 }", "{ 1i64 }");
    assert!(
        runtime
            .stage_reload_verified_program(&latest, "exports", source_code(&incompatible))
            .is_err()
    );
    assert_eq!(
        runtime
            .modules()
            .latest("exports")
            .unwrap()
            .retained_demands()
            .metadata()
            .records(),
        &baseline
    );
    assert_eq!(
        runtime.modules().latest("exports").unwrap().key(),
        latest.key()
    );
    let fresh = runtime
        .load_verified_program("exports", source_code(&next))
        .unwrap();
    let history = fresh.retained_demands().metadata();
    assert!(!history.records().bodies.iter().any(|body| body.calls.iter().any(|call| {
        matches!(call, CallRequirement::Direct(target) if history.definitions().resolve(target.declaration)
            .ok().and_then(|view| view.last()).is_some_and(|part| part.name == "helper"))
    })));
}

fn specialization(program: &VerifiedProgram) -> &BodyRecord {
    program
        .update_evidence()
        .bodies
        .iter()
        .find(|body| {
            body.requirement.origin.arguments.len() == 1
                && body
                    .requirement
                    .origin
                    .arguments
                    .iter()
                    .all(|argument| argument.is_concrete())
                && program
                    .definitions()
                    .resolve(body.requirement.origin.declaration)
                    .ok()
                    .and_then(|view| view.last())
                    .is_some_and(|part| part.name == "generic")
        })
        .expect("the candidate must materialize the retained concrete application")
}

fn retained_candidate(baseline: &LoadedModule, source: &str) -> VerifiedProgram {
    let code = bytecode(source, Some(baseline));
    for result in source_free_program_results(baseline, code.clone()) {
        result.unwrap();
    }
    let program = VerifiedProgram::new(code, &DefinitionContext::new().unwrap()).unwrap();
    assert!(!program.update_evidence().bodies.iter().any(|body| body.requirement.calls.iter().any(|call| {
        matches!(call, CallRequirement::Direct(target) if program.definitions().resolve(target.declaration)
            .ok().and_then(|view| view.last()).is_some_and(|part| part.name == "generic"))
    })), "only historical calls should demand the specialization");
    program
}

#[test]
fn source_free_candidates_require_historical_specializations_after_code_collection() {
    let source =
        "fn generic<T>(value: T) -> i32 { 11 } pub fn run() -> i32 { generic(7) } fn main() {}";
    let mut runtime = Runtime::default();
    let first = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    let first_key = first.key();
    let old_code = first
        .verified_program()
        .modules()
        .iter()
        .map(Arc::downgrade)
        .collect::<Vec<_>>();
    let first_instructions = specialization(first.verified_program())
        .instructions
        .clone();
    let second_source = source
        .replace("run() -> i32 { generic(7) }", "run() -> i32 { 9 }")
        .replace("-> i32 { 11 }", "-> i32 { 12 }");
    rejected_without_effects(&runtime, &first, &second_source);
    let second_code = retained_candidate(&first, &second_source);
    assert_ne!(
        specialization(&second_code).instructions,
        first_instructions,
        "retained demand must compile the candidate body, not reuse the old specialization"
    );
    let second_instructions = specialization(&second_code).instructions.clone();
    let staged = runtime
        .stage_reload_verified_program(&first, "exports", second_code)
        .unwrap();
    let second = runtime.publish_staged_reload(staged).unwrap();
    drop(first);
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(first_key).is_none());
    assert!(old_code.iter().all(|module| module.upgrade().is_none()));

    // Generation two has no source call to generic<i32>, and generation one's
    // executable graph has gone. Only the installation envelope can retain it.
    let third_source = second_source.replace("-> i32 { 12 }", "-> i32 { 13 }");
    rejected_without_effects(&runtime, &second, &third_source);
    let third_code = retained_candidate(&second, &third_source);
    assert_ne!(
        specialization(&third_code).instructions,
        second_instructions
    );
    let staged = runtime
        .stage_reload_verified_program(&second, "exports", third_code)
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
}

fn standard_code(text: &str) -> VerifiedProgram {
    let mut source = SourceDatabase::default();
    let root = source
        .set("exports.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(catalog::shared().unwrap());
    let checked = analysis
        .snapshot(source.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let code = lower_program_to_bytecode(&mir, &Default::default())
        .unwrap()
        .into_unverified();
    VerifiedProgram::new(code, &DefinitionContext::new().unwrap()).unwrap()
}

fn standard_runtime() -> Runtime {
    let mut runtime = Runtime::default();
    NativeModule::install_all(&modules().unwrap(), &mut runtime).unwrap();
    runtime
}

#[test]
fn protected_native_unknown_accepts_unchanged_inventory_but_not_additions() {
    let source = "use std::hash::Hash; static var STATE: i64 = 7.hash(); fn main() {}";
    let mut runtime = standard_runtime();
    let loaded = runtime
        .load_verified_program("exports", standard_code(source))
        .unwrap();
    assert!(
        loaded
            .verified_program()
            .modules()
            .iter()
            .any(|module| !module.native_imports.is_empty())
    );
    let staged = runtime
        .stage_reload_verified_program(&loaded, "exports", loaded.verified_program().clone())
        .unwrap();
    let current = runtime.publish_staged_reload(staged).unwrap();
    let history = current.retained_demands().metadata().records().clone();
    let source = format!("{source} pub fn added() -> i32 {{ 1 }}");
    let error = runtime
        .stage_reload_verified_program(&current, "exports", standard_code(&source))
        .unwrap_err();
    assert!(
        matches!(&error, ReloadValidationError::UpdateEvidence(message) if message.contains("unknown dependency edge")),
        "{error}"
    );
    let latest = runtime.modules().latest("exports").unwrap();
    assert_eq!(latest.key(), current.key());
    assert_eq!(latest.retained_demands().metadata().records(), &history);
    assert_eq!(runtime.resources().counters().peak_call_depth, 0);
    assert!(
        runtime
            .module_instance_snapshot(&current)
            .unwrap()
            .module_slots
            .iter()
            .all(|slot| matches!(slot, StaticState::Uninitialized(_)))
    );
}

#[test]
fn unused_generic_key_protocols_are_protected_without_live_containers() {
    let source = r#"
        use std::hash::Hash;
        struct Key<T> { val value: T }
        fn hash_helper() -> i64 { 1i64 }
        impl<T> PartialEq for Key<T> { fn eq(self, other: Self) -> bool { true } }
        impl<T> Eq for Key<T> {}
        impl<T> Hash for Key<T> { fn hash(self) -> i64 { hash_helper() } }
        fn main() {}
    "#;
    let mut runtime = standard_runtime();
    let loaded = runtime
        .load_verified_program("exports", standard_code(source))
        .unwrap();
    let facts = loaded.verified_program();
    assert!(facts.update_evidence().roots.iter().any(|root| {
        root.kind == RootKind::KeyHash
            && facts
                .definitions()
                .resolve(root.declaration.declaration)
                .is_ok_and(|view| view.module() == &loaded.bytecode.identity)
    }));
    for candidate in [
        source.replace(
            "hash_helper() -> i64 { 1i64 }",
            "hash_helper() -> i64 { 2i64 }",
        ),
        source.replace("-> bool { true }", "-> bool { false }"),
    ] {
        let result =
            runtime.stage_reload_verified_program(&loaded, "exports", standard_code(&candidate));
        assert!(
            matches!(result, Err(ReloadValidationError::UpdateEvidence(_))),
            "{result:?}"
        );
        assert_eq!(
            runtime.modules().latest("exports").unwrap().key(),
            loaded.key()
        );
        assert_eq!(runtime.resources().counters().peak_call_depth, 0);
    }
}
