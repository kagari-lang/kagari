use crate::{
    Runtime,
    module::{
        ExecutableModule, ModuleEpochRetention, VerifiedProgram,
        dispatch::{CallRoute, CallRouteKind},
    },
    reload::{
        ReloadValidationError, validate_reload_artifact_candidate, validate_reload_candidate,
    },
};
use kagari_bytecode::module::CallableTarget;
use std::sync::Arc;

// Share source/artifact construction with the namespace and integration suites;
// the helper contains no tests and avoids a second fixture producer here.
#[allow(clippy::duplicate_mod)]
#[path = "../../../tests/support/update_programs.rs"]
mod programs;

fn route(module: &ExecutableModule, name: &str) -> CallRoute {
    let (owner, function) = module.exported_function(name).unwrap();
    owner
        .bind_call_route(CallableTarget::Script(function))
        .unwrap()
}

const ALIASES: &str = r#"
    pub fn a() -> i32 { 1 }
    pub fn b() -> i32 { 2 }
    pub use crate::a as selected;
    fn main() {}
"#;

#[test]
fn ordinary_binding_survives_alias_retarget_and_old_code_collection() {
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("exports", programs::source_code(ALIASES))
        .unwrap();
    let old = runtime.select_program(&program).unwrap();
    let old_descriptor = Arc::downgrade(&old.program);
    let bound = route(&old, "selected");
    let token = old.publication_token();
    assert_eq!(bound.kind(), CallRouteKind::Named);
    let candidate = ALIASES
        .replace("crate::a as selected", "crate::b as selected")
        .replace("{ 1 }", "{ 10 }");
    for result in programs::source_free_reload_results(&old, &candidate) {
        result.unwrap();
    }
    let staged = runtime
        .stage_reload_verified_program(&program, None, programs::source_code(&candidate))
        .unwrap();
    let published_token = staged.publication_token();
    assert_ne!(token, published_token);
    runtime.publish_staged_reload(staged).unwrap();
    let current = runtime.select_program(&program).unwrap();
    let current_a = route(&current, "a");
    let fresh_alias = route(&current, "selected");
    assert_eq!(
        bound.declaration().unwrap(),
        current_a.declaration().unwrap()
    );
    assert_ne!(
        bound.declaration().unwrap(),
        fresh_alias.declaration().unwrap()
    );
    drop(old);
    runtime.collect_garbage().unwrap();
    assert!(
        old_descriptor.upgrade().is_none(),
        "metadata binding must not own old code"
    );
    let selected = runtime.select_call_route(&bound).unwrap();
    assert_eq!(selected.publication_token(), published_token);
    assert_eq!(
        selected.target(),
        runtime.select_call_route(&current_a).unwrap().target()
    );
    assert_ne!(
        selected.target(),
        runtime.select_call_route(&fresh_alias).unwrap().target()
    );
    let original = bound.application().unwrap().declaration;
    assert_eq!(
        selected.mapping().map(original).unwrap(),
        current_a.application().unwrap().declaration
    );
    assert!(Runtime::default().select_call_route(&bound).is_err());
    let independent = runtime
        .load_verified_program("independent", programs::source_code(ALIASES))
        .unwrap();
    let independent = runtime.select_program(&independent).unwrap();
    assert!(
        selected
            .mapping_from(independent.publication_definitions())
            .is_err()
    );
}

#[test]
fn staged_selection_and_abandonment_do_not_advance_identity_history() {
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("exports", programs::source_code(ALIASES))
        .unwrap();
    let old = runtime.select_program(&program).unwrap();
    let old_route = route(&old, "a");
    let candidate = format!("{ALIASES} pub fn added() -> i32 {{ 3 }}");
    let abandoned = runtime
        .stage_reload_verified_program(&program, None, programs::source_code(&candidate))
        .unwrap();
    let abandoned_token = abandoned.publication_token();
    let escaped = route(abandoned.module(), "added");
    assert!(runtime.select_call_route(&escaped).is_err());
    {
        let _session = runtime.begin_candidate_initialization(&abandoned).unwrap();
        assert_eq!(
            runtime
                .select_call_route(&escaped)
                .unwrap()
                .publication_token(),
            abandoned_token
        );
        assert!(runtime.select_call_route(&old_route).is_err());
    }
    drop(abandoned);
    assert_eq!(
        runtime.publication_token(&program).unwrap(),
        old.publication_token()
    );
    let staged = runtime
        .stage_reload_verified_program(&program, None, programs::source_code(&candidate))
        .unwrap();
    let added = route(staged.module(), "added");
    assert_ne!(abandoned_token, staged.publication_token());
    assert_ne!(escaped.declaration().unwrap(), added.declaration().unwrap());
    assert_eq!(
        old_route.declaration().unwrap(),
        route(staged.module(), "a").declaration().unwrap()
    );
    let stale = runtime
        .stage_reload_verified_program(&program, None, programs::source_code(&candidate))
        .unwrap();
    let stale_route = route(stale.module(), "added");
    runtime.publish_staged_reload(staged).unwrap();
    assert!(runtime.publish_staged_reload(stale).is_err());
    assert!(runtime.select_call_route(&stale_route).is_err());
    assert!(runtime.select_call_route(&escaped).is_err());
    assert!(runtime.select_call_route(&added).is_ok());

    let next = format!(
        "{} pub fn later() -> i32 {{ 4 }}",
        candidate.replace("{ 3 }", "{ 30 }")
    );
    let next = runtime
        .stage_reload_verified_program(&program, None, programs::source_code(&next))
        .unwrap();
    assert_eq!(
        route(next.module(), "added").declaration().unwrap(),
        added.declaration().unwrap()
    );
    let next_token = next.publication_token();
    runtime.publish_staged_reload(next).unwrap();
    assert_eq!(
        runtime
            .select_call_route(&old_route)
            .unwrap()
            .publication_token(),
        next_token
    );
    assert_eq!(
        runtime
            .select_call_route(&added)
            .unwrap()
            .publication_token(),
        next_token
    );
}

#[test]
fn retained_generated_body_and_ordinary_inner_route_select_different_publications() {
    let source = r#"
        pub fn a() -> i32 { 1 }
        fn main() -> i32 { val call: fn() -> i32 = || a(); call() }
    "#;
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("exports", programs::source_code(source))
        .unwrap();
    let old = runtime.select_program(&program).unwrap();
    let retained = runtime
        .retain_program(&old, ModuleEpochRetention::RuntimeValue)
        .unwrap();
    let inner = route(&old, "a");
    let body = old
        .bytecode
        .functions
        .iter()
        .map(|function| {
            old.bind_call_route(CallableTarget::Script(function.id))
                .unwrap()
        })
        .find(|binding| binding.kind() == CallRouteKind::BodyOwned)
        .unwrap();
    let next = source
        .replace("{ 1 }", "{ 2 }")
        .replace("|| a()", "|| a() + 10");
    let candidate = runtime
        .stage_reload_program(&program, None, programs::bytecode(&next, Some(&old)))
        .unwrap();
    let next_token = candidate.publication_token();
    runtime.publish_staged_reload(candidate).unwrap();
    assert_eq!(
        runtime
            .select_call_route(&body)
            .unwrap()
            .publication_token(),
        old.publication_token()
    );
    assert_eq!(
        runtime
            .select_call_route(&inner)
            .unwrap()
            .publication_token(),
        next_token
    );
    drop(retained);
    drop(old);
    runtime.collect_garbage().unwrap();
    assert!(runtime.select_call_route(&body).is_err());
    assert!(runtime.select_call_route(&inner).is_ok());
}

#[test]
fn semantic_impl_order_and_additions_preserve_original_binder_transport() {
    let source = r#"
        trait Tag {}
        struct Old { val n: i32 }
        struct Wrapper<T> { val value: T }
        impl<T: Tag> Wrapper<T> { fn unwrap(self) -> T { self.value } }
        trait Reader { fn unwrap<T: Tag>(self, value: Wrapper<T>) -> T; }
        impl Reader for Old {
            fn unwrap<T: Tag>(self, value: Wrapper<T>) -> T { value.unwrap() }
        }
        impl Old { pub fn read(self) -> i32 { self.n } }
        impl Tag for Old {}
        fn main() -> i32 {
            val reader: Reader = Old { n: 0 };
            val value = Wrapper { value: Old { n: 7 } };
            reader.unwrap(value).read()
        }
    "#;
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("exports", programs::source_code(source))
        .unwrap();
    let old = runtime.select_program(&program).unwrap();
    let bindings: Vec<_> = old
        .members()
        .flat_map(|owner| {
            owner
                .bytecode
                .functions
                .iter()
                .map(|function| {
                    owner
                        .bind_call_route(CallableTarget::Script(function.id))
                        .unwrap()
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        bindings
            .iter()
            .any(|binding| binding.kind() == CallRouteKind::Shared)
    );
    let candidate = source
        .replace("impl Tag for Old {}", "")
        .replace("impl Old {", "impl Tag for Old {} impl Old {")
        .replace(
            "impl<T: Tag> Wrapper<T> { fn unwrap(self) -> T",
            "impl<U: Tag> Wrapper<U> { fn unwrap(self) -> U",
        )
        .replace(
            "fn unwrap<T: Tag>(self, value: Wrapper<T>) -> T",
            "fn unwrap<U: Tag>(self, value: Wrapper<U>) -> U",
        );
    let candidate =
        format!("{candidate} impl Old {{ pub fn added(self) -> i32 {{ self.n + 1 }} }}");
    let staged = runtime
        .stage_reload_program(&program, None, programs::bytecode(&candidate, Some(&old)))
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
    for binding in bindings {
        if binding.kind() == CallRouteKind::BodyOwned {
            continue;
        }
        let selected = runtime.select_call_route(&binding).unwrap();
        let fresh = selected.owner().bind_call_route(selected.target()).unwrap();
        assert_eq!(binding.declaration().unwrap(), fresh.declaration().unwrap());
        let old_parent = binding
            .definitions()
            .definitions()
            .parent(binding.application().unwrap().declaration)
            .unwrap();
        let new_parent = fresh
            .definitions()
            .definitions()
            .parent(fresh.application().unwrap().declaration)
            .unwrap();
        assert_eq!(
            old_parent.map(|id| selected.mapping().map(id).unwrap()),
            new_parent
        );
        let old_selected = old
            .program
            .dispatch
            .tables
            .target(old.slot(), binding.target)
            .unwrap();
        if let Some(generic) = &old_selected.generic {
            let mapped = selected
                .mapping()
                .remap(generic, &Default::default())
                .unwrap();
            assert_eq!(Some(mapped.records()), selected.generic());
        }
    }
}
