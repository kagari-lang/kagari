//! Local initializer and historical-demand contracts, independent of execution liveness.
use crate::{
    Runtime,
    module::{ExecutableModule, Program, VerifiedProgram, state::StaticState},
    reload::ReloadValidationError,
    test_support::{
        update_programs::{
            bytecode, source_code, source_free_artifact, source_free_program_results,
            source_free_reload_results,
        },
        update_publication::publish_without_execution,
    },
};
use kagari_common::identity::map::DefinitionContext;
use kagari_contract::updates::{BodyRecord, CallRequirement};

use std::sync::Arc;

fn rejected_without_effects(
    runtime: &Runtime,
    loaded_program: &Program,
    candidate: &str,
    expected_error: Option<&str>,
) {
    let loaded = runtime.select_program(loaded_program).unwrap();
    let history = loaded.retained_demands().metadata().records().clone();
    let count = runtime.modules().loaded_count();
    let code = bytecode(candidate, None);
    let artifact = source_free_artifact(code.clone());
    let verified = VerifiedProgram::new(code.clone(), &DefinitionContext::new().unwrap()).unwrap();
    for result in source_free_reload_results(&loaded, candidate)
        .into_iter()
        .chain([
            runtime
                .stage_reload_program(loaded_program, None, code)
                .map(|_| ()),
            runtime
                .stage_reload_artifact(loaded_program, None, artifact, &Default::default())
                .map(|_| ()),
            runtime
                .stage_reload_verified_program(loaded_program, None, verified)
                .map(|_| ()),
        ])
    {
        assert!(
            matches!(&result, Err(ReloadValidationError::UpdateEvidence(_))),
            "{result:?}"
        );
        if let Some(expected) = expected_error {
            let error = result.unwrap_err();
            assert!(
                matches!(&error, ReloadValidationError::UpdateEvidence(message) if message == expected),
                "{error}"
            );
        }
    }
    let current = runtime.select_program(loaded_program).unwrap();
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
fn lazy_initializer_freezes_expression_but_allows_transitive_helper_updates() {
    let source = r#"
        fn leaf() -> i32 { 3 }
        fn helper() -> i32 { leaf() }
        static var STATE: i32 = helper() + (1 + 2);
        pub fn run() -> i32 { 7 }
        fn main() {}
    "#;
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let facts = loaded.verified_program();
    assert_eq!(facts.update_evidence().initializers.len(), 1);
    let initializer = &facts.update_evidence().initializers[0];
    assert!(
        facts
            .definitions()
            .resolve(initializer.declaration)
            .unwrap()
            .last()
            .is_some_and(|part| part.name == "STATE")
    );
    rejected_without_effects(
        &runtime,
        &loaded_program,
        &source.replace("helper() + (1 + 2)", "helper() + 3"),
        Some(&format!(
            "static initializer {}::STATE changed",
            loaded.bytecode.identity
        )),
    );
    // Direct expression tokens stay fixed; neither direct nor transitive helpers do.
    let candidate = source
        .replace("leaf() -> i32 { 3 }", "leaf() -> i32 { 4 }")
        .replace(
            "helper() -> i32 { leaf() }",
            "helper() -> i32 { leaf() + 1 }",
        )
        .replace("run() -> i32 { 7 }", "run() -> i32 { 8 }")
        .replace(
            "helper() + (1 + 2)",
            "helper() /* unchanged tokens */ + ( 1 + 2 )",
        );
    for result in source_free_reload_results(&loaded, &candidate) {
        result.unwrap();
    }
    publish_without_execution(&mut runtime, &loaded_program, source_code(&candidate));
}

#[test]
fn initializer_function_value_allows_helper_updates_and_executable_additions() {
    let source = r#"
        fn leaf() -> i32 { 3 }
        fn helper() -> i32 { val call: fn() -> i32 = || leaf(); call() }
        static var STATE: i32 = helper();
        fn main() {}
    "#;
    let mut runtime = Runtime::default();
    let loaded_program = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    let candidate = format!(
        "{} fn added() -> i32 {{ 9 }}",
        source.replace("|| leaf()", "|| leaf() + 1")
    );
    let loaded = runtime.select_program(&loaded_program).unwrap();
    for result in source_free_reload_results(&loaded, &candidate) {
        result.unwrap();
    }
    publish_without_execution(&mut runtime, &loaded_program, source_code(&candidate));
}

#[test]
fn historical_demands_survive_old_code_collection_and_failed_candidates() {
    let source = "pub fn helper() -> i32 { 1 } pub fn run() -> i32 { helper() } fn main() {}";
    let mut runtime = Runtime::default();
    let old_program = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    let old = runtime.select_program(&old_program).unwrap();
    let old_key = old.key();
    let code = old
        .verified_program()
        .modules()
        .iter()
        .map(Arc::downgrade)
        .collect::<Vec<_>>();
    let replacement = source.replace("run() -> i32 { helper() }", "run() -> i32 { 2 }");
    let staged = runtime
        .stage_reload_verified_program(&old_program, None, source_code(&replacement))
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();

    let current = runtime.select_program(&old_program).unwrap();
    drop(old);
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(old_key).is_none());
    assert!(code.iter().all(|module| module.upgrade().is_none()));
    let baseline = current.retained_demands().metadata().records().clone();
    let abandoned_source = format!("{replacement} pub fn abandoned_only() -> i32 {{ helper() }}");
    let abandoned = runtime
        .stage_reload_verified_program(&old_program, None, source_code(&abandoned_source))
        .unwrap();
    drop(abandoned);
    assert_eq!(current.retained_demands().metadata().records(), &baseline);
    let stale_source = format!("{replacement} pub fn stale_only() -> i32 {{ helper() }}");
    let stale = runtime
        .stage_reload_verified_program(&old_program, None, source_code(&stale_source))
        .unwrap();
    let next = replacement.replace("run() -> i32 { 2 }", "run() -> i32 { 3 }");
    let staged = runtime
        .stage_reload_verified_program(&old_program, None, source_code(&next))
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();

    let latest = runtime.select_program(&old_program).unwrap();
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
            .stage_reload_verified_program(&old_program, None, source_code(&incompatible))
            .is_err()
    );
    assert_eq!(
        runtime
            .select_program(&old_program)
            .unwrap()
            .retained_demands()
            .metadata()
            .records(),
        &baseline
    );
    assert_eq!(
        runtime.select_program(&old_program).unwrap().key(),
        latest.key()
    );
    let fresh_program = runtime
        .load_verified_program("exports", source_code(&next))
        .unwrap();
    let fresh = runtime.select_program(&fresh_program).unwrap();
    assert_ne!(fresh.installation_id(), latest.installation_id());
    assert_eq!(
        runtime.select_program(&old_program).unwrap().key(),
        latest.key()
    );
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

fn retained_candidate(baseline: &ExecutableModule, source: &str) -> VerifiedProgram {
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
    let first_program = runtime
        .load_verified_program("exports", source_code(source))
        .unwrap();
    let first = runtime.select_program(&first_program).unwrap();
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
    rejected_without_effects(&runtime, &first_program, &second_source, None);
    let second_code = retained_candidate(&first, &second_source);
    assert_ne!(
        specialization(&second_code).instructions,
        first_instructions,
        "retained demand must compile the candidate body, not reuse the old specialization"
    );
    let second_instructions = specialization(&second_code).instructions.clone();
    let staged = runtime
        .stage_reload_verified_program(&first_program, None, second_code)
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();

    let second = runtime.select_program(&first_program).unwrap();
    drop(first);
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(first_key).is_none());
    assert!(old_code.iter().all(|module| module.upgrade().is_none()));

    // Generation two has no source call to generic<i32>, and generation one's
    // executable graph has gone. Only the installation envelope can retain it.
    let third_source = second_source.replace("-> i32 { 12 }", "-> i32 { 13 }");
    rejected_without_effects(&runtime, &first_program, &third_source, None);
    let third_code = retained_candidate(&second, &third_source);
    assert_ne!(
        specialization(&third_code).instructions,
        second_instructions
    );
    let staged = runtime
        .stage_reload_verified_program(&first_program, None, third_code)
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
}
