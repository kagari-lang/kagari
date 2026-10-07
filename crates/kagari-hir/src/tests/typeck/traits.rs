use super::*;
use kagari_source::diagnostic::DiagnosticKind;
use kagari_stdlib::catalog as foundation_catalog;

#[test]
fn trait_method_where_bounds_keep_self_and_associated_output_owners() {
    let lowered = common::lower_ok(
        "trait Sequence { type Item; fn size(self) -> usize where Self: Iterable<Item = Self::Item>, Self::Item: Eq; }",
    );
    let analyzed = crate::analyze_source(&lowered.source, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_checked()
        .expect("Self bounds in their declaring trait context");
    let typed = analyzed.typed;
    let method = typed
        .functions
        .iter()
        .find(|function| function.name == "size")
        .unwrap();
    assert!(
        method
            .bounds
            .keys()
            .any(|ty| matches!(ty, TypeId::SelfType(_)))
    );
    assert!(method.bounds.keys().any(|ty| matches!(ty, TypeId::Projection { receiver, .. } if matches!(receiver.as_ref(), TypeId::SelfType(_)))));
}

#[test]
fn associated_projection_bounds_require_the_same_receiver_and_member() {
    for (projection, valid) in [
        ("<T as Carrier>::Item", true),
        ("<U as Carrier>::Item", false),
        ("<T as Carrier>::Other", false),
    ] {
        let source = SourceFile::new(
            "projection-bound.kgr",
            format!(
                r#"
trait Carrier {{ type Item; type Other; }}
fn take<T, I>(value: T) where T: Carrier<Item = I> {{}}
fn forward<T: Carrier, U: Carrier>(value: T) {{ take::<T, {projection}>(value); }}
"#
            ),
        );
        let result = crate::analyze_source(&source, foundation_catalog::shared()).unwrap();
        if valid {
            result
                .into_checked()
                .expect("reflexive associated equality");
        } else {
            assert!(
                result.diagnostics().iter().any(|diagnostic| matches!(
                    diagnostic.kind,
                    DiagnosticKind::GenericBoundNotSatisfied { .. }
                )),
                "{:?}",
                result.diagnostics()
            );
        }
    }
}

#[test]
fn trait_name_is_not_a_self_parameter_in_where_bounds() {
    let lowered = common::lower_ok("trait Sequence { fn size(self) -> usize where Sequence: Eq; }");
    let names = resolve_names(&lowered)
        .unwrap()
        .into_checked()
        .expect("trait names");
    let typed = check_module(&lowered, &names, None);
    assert!(typed.diagnostics().iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidBoundTarget { name } if name == "Sequence")));
}

#[test]
fn validates_trait_impl_and_interface_method_calls() {
    let lowered = common::lower_ok(
        r#"
trait Display {
    fn show(self) -> String;
}

struct Player {
    val name: String,
}

impl Display for Player {
    fn show(self) -> String {
        self.name
    }
}

fn show_interface(value: Display) -> String {
    value.show()
}

fn show_static<T>(value: T) -> String
where T: Display
{
    value.show()
}
"#,
    );
    let analyzed = crate::analyze_source(&lowered.source, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_checked()
        .expect("type checker should succeed");
    let typed = analyzed.typed;

    let show_interface = typed
        .functions
        .iter()
        .find(|function| function.name == "show_interface")
        .expect("expected show_interface");
    assert_eq!(
        show_interface.params[0].ty,
        TypeId::Trait(crate::types::NominalType {
            associated_types: Default::default(),
            declaration: common::definition(
                &lowered,
                kagari_common::identity::DefinitionKind::Trait,
                "Display"
            ),
            arguments: Vec::new()
        })
    );
    assert_eq!(
        show_interface.return_type,
        TypeId::Builtin(BuiltinType::String)
    );
}

#[test]
fn reports_unknown_trait_bounds() {
    let lowered = common::lower_ok(
        r#"
fn show<T>(value: T) -> T
where T: Missing
{
    value
}
"#,
    );
    let names = resolve_names(&lowered)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("type checker should reject unknown bound");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::UnknownTrait {
                trait_name: "Missing".to_string(),
            }
    }));
}

#[test]
fn accepts_interface_use_of_generic_trait_methods() {
    let lowered = common::lower_ok(
        r#"
trait Mapper {
    fn map<T>(self, value: T) -> T;
}

fn use_mapper(value: Mapper) -> (i32, String) {
    (value.map(42), value.map("answer"))
}
"#,
    );
    crate::analyze_source(&lowered.source, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_checked()
        .expect("method arguments are inferred independently through the same interface");
}

#[test]
fn rejects_interface_method_with_another_self_parameter() {
    let lowered = common::lower_ok(
        "trait Pair { fn same(self, other: Self) -> bool; } fn use_pair(value: Pair) {}",
    );
    let names = resolve_names(&lowered).unwrap().into_checked().unwrap();
    let diagnostics = check_module(&lowered, &names, None)
        .into_checked()
        .expect_err("a second Self argument cannot be called through an interface value");
    assert!(diagnostics.iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::InvalidInterfaceType { reason, .. }
            if reason == "method `same` is not interface-compatible"
    )));
}

#[test]
fn rejects_invalid_trait_impls() {
    let missing_method = common::lower_ok(
        r#"
trait Display {
    fn show(self) -> String;
}

struct Player {
    val name: String,
}

impl Display for Player {}
"#,
    );
    let diagnostics = crate::analyze_source(&missing_method.source, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_checked()
        .expect_err("type checker should reject impl");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::TraitMethodMismatch {
                trait_name: "Display".to_string(),
                method_name: "show".to_string(),
                reason: "missing impl method".to_string(),
            }
    }));

    let wrong_return = common::lower_ok(
        r#"
trait Display {
    fn show(self) -> String;
}

struct Player {
    val name: String,
}

impl Display for Player {
    fn show(self) -> i32 {
        1
    }
}
"#,
    );
    let diagnostics = crate::analyze_source(&wrong_return.source, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_checked()
        .expect_err("type checker should reject impl");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::TraitMethodMismatch {
                trait_name: "Display".to_string(),
                method_name: "show".to_string(),
                reason: "return type expected `String`, found `i32`".to_string(),
            }
    }));
}

#[test]
fn trait_method_generic_binders_match_by_position() {
    let valid = common::lower_ok(
        "trait Convert { fn take<T>(self, value: T) -> T; } struct Holder {} impl Convert for Holder { fn take<U>(self, value: U) -> U { value } }",
    );
    let names = resolve_names(&valid).unwrap().into_checked().unwrap();
    check_module(&valid, &names, None)
        .into_checked()
        .expect("equivalent method binders should match");

    let invalid = common::lower_ok(
        "trait Convert { fn take<T>(self, value: T) -> T; } struct Holder {} impl Convert for Holder { fn take<U, V>(self, value: U) -> U { value } }",
    );
    let names = resolve_names(&invalid).unwrap().into_checked().unwrap();
    let diagnostics = check_module(&invalid, &names, None)
        .into_checked()
        .expect_err("different method binder arity should fail");
    assert!(diagnostics.iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::TraitMethodMismatch { reason, .. }
            if reason == "generic parameter count differs"
    )));
}

#[test]
fn private_trait_method_bounds_match_after_trait_and_method_substitution() {
    let valid = common::lower_ok(
        "trait Marker<T> {} trait Consumer<T> { fn take<U: Marker<T>>(self, value: U) -> U; } struct Holder {} impl Consumer<i32> for Holder { fn take<V: Marker<i32>>(self, value: V) -> V { value } }",
    );
    let names = resolve_names(&valid).unwrap().into_checked().unwrap();
    check_module(&valid, &names, None)
        .into_checked()
        .expect("equivalent applied method bounds should match");

    let invalid = common::lower_ok(
        "trait Marker<T> {} trait Consumer<T> { fn take<U: Marker<T>>(self, value: U) -> U; } struct Holder {} impl Consumer<i32> for Holder { fn take<V: Marker<bool>>(self, value: V) -> V { value } }",
    );
    let names = resolve_names(&invalid).unwrap().into_checked().unwrap();
    let diagnostics = check_module(&invalid, &names, None)
        .into_checked()
        .expect_err("different private method bounds should fail");
    assert!(diagnostics.iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::TraitMethodMismatch { reason, .. } if reason == "generic bound differs"
    )));
}

#[test]
fn applied_trait_interface_type_keeps_trait_and_method_binders_distinct() {
    let valid = common::lower_ok(
        "trait Echo<T> { fn get(self) -> T; } fn use_interface(value: Echo<i32>) {}",
    );
    let names = resolve_names(&valid).unwrap().into_checked().unwrap();
    check_module(&valid, &names, None)
        .into_checked()
        .expect("an applied trait interface has concrete inherited arguments");

    let generic = common::lower_ok(
        "trait Echo<T> { fn get<U>(self, value: U) -> T; } fn use_interface(value: Echo<i32>) -> i32 { value.get(\"key\") }",
    );
    let names = resolve_names(&generic).unwrap().into_checked().unwrap();
    check_module(&generic, &names, None)
        .into_checked()
        .expect("T remains i32 while method U is inferred as String");
}

#[test]
fn generic_interface_method_checks_bounds_and_forwards_generic_arguments() {
    let valid = common::lower_ok(
        r#"
trait Transform { fn apply<K: Ord>(self, value: K) -> K; }
fn forward<K: Ord>(receiver: Transform, value: K) -> K { receiver.apply(value) }
fn call(receiver: Transform) -> i32 { forward(receiver, 42) }
"#,
    );
    crate::analyze_source(&valid.source, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_checked()
        .expect("generic interface call forwards a statically proven bound");
    let invalid = common::lower_ok(
        "trait Transform { fn apply<K: Ord>(self, value: K) -> K; } struct Unordered {} fn call(receiver: Transform) { receiver.apply(Unordered {}); }",
    );
    let diagnostics = crate::analyze_source(&invalid.source, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_checked()
        .expect_err("interface calls must not bypass method-local bounds");
    assert!(diagnostics.iter().any(|diagnostic| matches!(
        diagnostic.kind,
        DiagnosticKind::GenericBoundNotSatisfied { .. }
    )));
}
