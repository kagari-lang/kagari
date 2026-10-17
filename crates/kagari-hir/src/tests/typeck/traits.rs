use super::*;
use crate::{hir::expr::ExprKind, typeck::table::CallTarget};
use kagari_common::identity::map::DefinitionContext;
use kagari_source::diagnostic::DiagnosticKind;
use kagari_stdlib::catalog as foundation_catalog;

const FACTORY: &str = r#"
trait Factory { fn make() -> Self; }
struct Player { val health: i32 }
impl Factory for Player { fn make() -> Self { Player { health: 100 } } }
"#;

#[test]
fn contextual_trait_static_calls_share_checked_dispatch_and_expected_boundaries() {
    let source = SourceFile::new(
        "factory.kgr",
        format!(
            r#"
{FACTORY}
use self::Factory as Maker;
use self::Factory as Default;
struct Holder {{ val player: Player }}
fn consume(player: Player) {{}}
fn contextual() -> Player {{ Factory::make() }}
fn concrete() -> Player {{ Player::make() }}
fn qualified() -> Player {{ <Player as Factory>::make() }}
fn generic<T: Factory>() -> T {{ Factory::make() }}
fn generic_owner<T: Factory>() -> T {{ T::make() }}
fn alias() -> Player {{ Maker::make() }}
fn shadowed_prelude() -> Player {{ Default::make() }}
fn value_shadow(Factory: i32) -> Player {{ Factory::make() }}
fn local_shadow() -> Player {{ val Factory = 42; Factory::make() }}
fn returned() -> Player {{ return Factory::make(); }}
fn boundaries() {{
    var player: Player = Factory::make();
    player = Factory::make();
    consume(Factory::make());
    val holder = Holder {{ player: Factory::make() }};
    val delayed = Factory::make();
    consume(delayed);
}}
"#
        ),
    );
    let analysis = crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
    let checked = analysis
        .into_codegen()
        .expect("ordinary static trait calls");
    for name in [
        "contextual",
        "concrete",
        "qualified",
        "generic",
        "generic_owner",
        "alias",
        "shadowed_prelude",
        "value_shadow",
        "local_shadow",
    ] {
        let function = checked
            .lowered
            .module
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        let call = checked
            .lowered
            .module
            .block(function.body.unwrap())
            .tail_expr
            .unwrap();
        let resolution = checked
            .typed
            .type_table
            .call_resolution(call)
            .expect("selected static call");
        assert!(
            matches!(resolution.target, CallTarget::TraitMethod { .. }),
            "{name}"
        );
        assert!(resolution.receiver.is_none(), "{name}");
        let signature = checked
            .typed
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        assert_eq!(
            checked.typed.type_table.protocol_receiver(call),
            Some(&signature.return_type),
            "{name}"
        );
    }
}

#[test]
fn contextual_trait_static_calls_constrain_general_declared_results() {
    let source = SourceFile::new(
        "factory-result.kgr",
        r#"
trait Factory<A> { fn make<B>(value: B) -> (Self, A, B); }
struct Player {}
impl Factory<i32> for Player {
    fn make<B>(value: B) -> (Self, i32, B) { (Player {}, 42, value) }
}
fn explicit() -> (Player, i32, bool) { Factory<i32>::make::<bool>(true) }
fn inferred() -> (Player, i32, bool) { Factory<i32>::make(true) }
trait Nested { fn make() -> Option<Self>; }
impl Nested for Player { fn make() -> Option<Self> { Some(Player {}) } }
fn nested() -> Option<Player> { Nested::make() }
"#,
    );
    let analysis = crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
    analysis
        .into_codegen()
        .expect("structural result constraints");
}

#[test]
fn contextual_trait_static_calls_do_not_invent_self_from_implementations() {
    for extra in [
        "",
        "struct Other {} impl Factory for Other { fn make() -> Self { Other {} } }",
    ] {
        let source = SourceFile::new(
            "factory-context.kgr",
            format!("{FACTORY} {extra} fn main() {{ val player = Factory::make(); }}"),
        );
        let analysis =
            crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(
            &diagnostic.kind, DiagnosticKind::CannotInferGenericArgument { parameter, .. } if parameter == "Self"
        )), "{:?}", analysis.diagnostics());
        assert!(analysis.into_codegen().is_err());
    }
    let source = SourceFile::new(
        "factory-projection.kgr",
        r#"
trait Factory { type Item; fn make() -> Self::Item; }
struct Player {}
impl Factory for Player { type Item = i32; fn make() -> i32 { 42 } }
fn main() -> i32 { Factory::make() }
"#,
    );
    let analysis = crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
    assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(
        &diagnostic.kind, DiagnosticKind::CannotInferGenericArgument { parameter, .. } if parameter == "Self"
    )), "{:?}", analysis.diagnostics());
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn contextual_trait_static_calls_require_proven_bounds_and_preserve_ambiguity() {
    for declaration in [
        "fn missing<T>() -> T { Factory::make() }",
        "fn incompatible() -> bool { Factory::make() }",
    ] {
        let source = SourceFile::new("factory-bound.kgr", format!("{FACTORY} {declaration}"));
        let analysis =
            crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        assert!(
            analysis.diagnostics().iter().any(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::GenericBoundNotSatisfied { .. }
            )),
            "{:?}",
            analysis.diagnostics()
        );
        let facts = analysis.facts();
        for (call, expression) in facts.lowered.module.body.expressions() {
            if matches!(expression.kind, ExprKind::Call { .. }) {
                assert!(facts.typed.type_table.call_resolution(call).is_none());
                assert!(facts.typed.type_table.protocol_receiver(call).is_none());
            }
        }
        assert!(analysis.into_codegen().is_err());
    }
    let source = SourceFile::new(
        "factory-ambiguity.kgr",
        format!(
            r#"
{FACTORY}
trait Other {{ fn make() -> Self; }}
impl Other for Player {{ fn make() -> Self {{ Player {{ health: 1 }} }} }}
fn main() -> Player {{ Player::make() }}
"#
        ),
    );
    let analysis = crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
    assert!(
        analysis
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::AmbiguousMethod { .. })),
        "{:?}",
        analysis.diagnostics()
    );
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn contextual_trait_static_calls_preserve_static_only_admission() {
    for declaration in [
        "fn dynamic(value: Factory) {}",
        "fn dynamic() { val value: Factory = Factory::make(); }",
    ] {
        let source = SourceFile::new("factory-shadow.kgr", format!("{FACTORY} {declaration}"));
        let analysis =
            crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        assert!(!analysis.diagnostics().is_empty(), "{declaration}");
        assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic.kind,
            DiagnosticKind::InvalidInterfaceType { .. }
        )));
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn contextual_trait_static_calls_recovery_and_never_do_not_prove_a_receiver() {
    for declaration in [
        "fn diverges() -> never { Factory::make() }",
        "fn recovery() -> Missing { Factory::make() }",
    ] {
        let source = SourceFile::new("factory-recovery.kgr", format!("{FACTORY} {declaration}"));
        let analysis =
            crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(
            &diagnostic.kind, DiagnosticKind::CannotInferGenericArgument { parameter, .. } if parameter == "Self"
        )), "{:?}", analysis.diagnostics());
        let facts = analysis.facts();
        for (call, expression) in facts.lowered.module.body.expressions() {
            if matches!(expression.kind, ExprKind::Call { .. }) {
                assert!(facts.typed.type_table.call_resolution(call).is_none());
                assert!(facts.typed.type_table.protocol_receiver(call).is_none());
            }
        }
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn contextual_trait_static_calls_keep_independent_argument_diagnostics() {
    let source = SourceFile::new(
        "factory-arguments.kgr",
        r#"
trait Factory { fn make(seed: i32) -> Self; }
struct Player {}
impl Factory for Player { fn make(seed: i32) -> Self { Player {} } }
fn main() { Factory::make(false, unknown); }
"#,
    );
    let analysis = crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
    for expected in ["Self", "arity", "argument", "name"] {
        assert!(
            analysis
                .diagnostics()
                .iter()
                .any(|diagnostic| match (&diagnostic.kind, expected) {
                    (DiagnosticKind::CannotInferGenericArgument { parameter, .. }, "Self") =>
                        parameter == "Self",
                    (DiagnosticKind::CallArityMismatch { .. }, "arity") => true,
                    (DiagnosticKind::ArgumentTypeMismatch { .. }, "argument") => true,
                    (DiagnosticKind::UnknownName { .. }, "name") => true,
                    _ => false,
                }),
            "{expected}: {:?}",
            analysis.diagnostics()
        );
    }
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn trait_method_where_bounds_keep_self_and_associated_output_owners() {
    let lowered = common::lower_ok(
        "trait Sequence { type Item; fn size(self) -> usize where Self: Iterable<Item = Self::Item>, Self::Item: Eq; }",
    );
    let analyzed = crate::analyze_source(&lowered.source, foundation_catalog::shared().unwrap())
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
        let result = crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
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
fn applied_impl_bounds_reuse_the_callers_generic_assumptions() {
    for (bound, valid) in [(": Marker", true), ("", false)] {
        let source = SourceFile::new(
            "applied-impl-bounds.kgr",
            format!(
                r#"
trait Marker {{}}
trait Ready {{}}
struct Wrap<T> {{ val value: T }}
impl<T: Marker> Ready for Wrap<T> {{}}
struct Needs<T: Ready> {{ val value: T }}
fn take<T: Ready>(value: T) {{}}
fn forward<T{bound}>(value: Wrap<T>) {{
    val checked: Needs<Wrap<T>> = Needs {{ value: value }};
    take(value);
}}
"#
            ),
        );
        let result = crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        if valid {
            result
                .into_checked()
                .expect("application and call searches retain the caller's Marker bound");
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
    let context = DefinitionContext::new().unwrap();
    let lowered = common::lower_ok("trait Sequence { fn size(self) -> usize where Sequence: Eq; }");
    let names = resolve_names(&lowered)
        .unwrap()
        .into_checked()
        .expect("trait names");
    let typed = check_module(&context, &lowered, &names, None);
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
    let analyzed = crate::analyze_source(&lowered.source, foundation_catalog::shared().unwrap())
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
                analyzed.declarations.definitions(),
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
    let context = DefinitionContext::new().unwrap();
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
    let diagnostics = check_module(&context, &lowered, &names, None)
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
    crate::analyze_source(&lowered.source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_checked()
        .expect("method arguments are inferred independently through the same interface");
}

#[test]
fn rejects_interface_method_with_another_self_parameter() {
    let context = DefinitionContext::new().unwrap();
    let lowered = common::lower_ok(
        "trait Pair { fn same(self, other: Self) -> bool; } fn use_pair(value: Pair) {}",
    );
    let names = resolve_names(&lowered).unwrap().into_checked().unwrap();
    let diagnostics = check_module(&context, &lowered, &names, None)
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
    let diagnostics = crate::analyze_source(
        &missing_method.source,
        foundation_catalog::shared().unwrap(),
    )
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
    let diagnostics =
        crate::analyze_source(&wrong_return.source, foundation_catalog::shared().unwrap())
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
    let context = DefinitionContext::new().unwrap();
    let valid = common::lower_ok(
        "trait Convert { fn take<T>(self, value: T) -> T; } struct Holder {} impl Convert for Holder { fn take<U>(self, value: U) -> U { value } }",
    );
    let names = resolve_names(&valid).unwrap().into_checked().unwrap();
    check_module(&context, &valid, &names, None)
        .into_checked()
        .expect("equivalent method binders should match");

    let invalid = common::lower_ok(
        "trait Convert { fn take<T>(self, value: T) -> T; } struct Holder {} impl Convert for Holder { fn take<U, V>(self, value: U) -> U { value } }",
    );
    let names = resolve_names(&invalid).unwrap().into_checked().unwrap();
    let diagnostics = check_module(&context, &invalid, &names, None)
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
    let context = DefinitionContext::new().unwrap();
    let valid = common::lower_ok(
        "trait Marker<T> {} trait Consumer<T> { fn take<U: Marker<T>>(self, value: U) -> U; } struct Holder {} impl Consumer<i32> for Holder { fn take<V: Marker<i32>>(self, value: V) -> V { value } }",
    );
    let names = resolve_names(&valid).unwrap().into_checked().unwrap();
    check_module(&context, &valid, &names, None)
        .into_checked()
        .expect("equivalent applied method bounds should match");

    let invalid = common::lower_ok(
        "trait Marker<T> {} trait Consumer<T> { fn take<U: Marker<T>>(self, value: U) -> U; } struct Holder {} impl Consumer<i32> for Holder { fn take<V: Marker<bool>>(self, value: V) -> V { value } }",
    );
    let names = resolve_names(&invalid).unwrap().into_checked().unwrap();
    let diagnostics = check_module(&context, &invalid, &names, None)
        .into_checked()
        .expect_err("different private method bounds should fail");
    assert!(diagnostics.iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::TraitMethodMismatch { reason, .. } if reason == "generic bound differs"
    )));
}

#[test]
fn applied_trait_interface_type_keeps_trait_and_method_binders_distinct() {
    let context = DefinitionContext::new().unwrap();
    let valid = common::lower_ok(
        "trait Echo<T> { fn get(self) -> T; } fn use_interface(value: Echo<i32>) {}",
    );
    let names = resolve_names(&valid).unwrap().into_checked().unwrap();
    check_module(&context, &valid, &names, None)
        .into_checked()
        .expect("an applied trait interface has concrete inherited arguments");

    let generic = common::lower_ok(
        "trait Echo<T> { fn get<U>(self, value: U) -> T; } fn use_interface(value: Echo<i32>) -> i32 { value.get(\"key\") }",
    );
    let names = resolve_names(&generic).unwrap().into_checked().unwrap();
    check_module(&context, &generic, &names, None)
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
    crate::analyze_source(&valid.source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_checked()
        .expect("generic interface call forwards a statically proven bound");
    let invalid = common::lower_ok(
        "trait Transform { fn apply<K: Ord>(self, value: K) -> K; } struct Unordered {} fn call(receiver: Transform) { receiver.apply(Unordered {}); }",
    );
    let diagnostics = crate::analyze_source(&invalid.source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_checked()
        .expect_err("interface calls must not bypass method-local bounds");
    assert!(diagnostics.iter().any(|diagnostic| matches!(
        diagnostic.kind,
        DiagnosticKind::GenericBoundNotSatisfied { .. }
    )));
}

#[test]
fn dynamic_trait_eligibility_checks_the_whole_applied_surface() {
    for (declaration, application, accepted) in [
        ("trait View { fn read(self) -> i32; }", "View", true),
        (
            "trait View { fn map<T>(self, value: T) -> T { value } }",
            "View",
            true,
        ),
        (
            "trait View { fn read(self) -> i32; fn other(self, value: [Self]); }",
            "View",
            false,
        ),
        (
            "trait View { fn read(self) -> fn() -> Self; }",
            "View",
            false,
        ),
        ("trait View { fn make() -> i32; }", "View", false),
        (
            "trait View { const SIZE: i32; fn read(self) -> i32; }",
            "View",
            false,
        ),
        (
            "trait View { type Item<T>; fn read(self) -> i32; }",
            "View",
            false,
        ),
        (
            "trait Related<T> {} trait View { fn read<T: Related<Self>>(self, item: T); }",
            "View",
            false,
        ),
        (
            "trait Related<T> {} trait Parent { fn read<T: Related<Self>>(self, item: T); } trait View: Parent {}",
            "View",
            false,
        ),
        (
            "trait Related<T> {} trait View { fn read<T: Related<i32>>(self, item: T); }",
            "View",
            true,
        ),
        (
            "trait View { type Item; fn read(self) -> Self::Item; }",
            "View<Item = i32>",
            true,
        ),
        (
            "trait View { type Item; fn read(self) -> Self::Item; }",
            "View",
            false,
        ),
        (
            "trait Parent { type Item; fn read(self) -> Self::Item; } trait View: Parent<Item = i32> {}",
            "View",
            true,
        ),
        (
            "trait View { fn read(self) -> i32 where Self: View; }",
            "View",
            true,
        ),
    ] {
        let static_source = SourceFile::new(
            "static-trait.kgr",
            format!("{declaration} fn use_static<T: View>() {{}}"),
        );
        let analysis =
            crate::analyze_source(&static_source, foundation_catalog::shared().unwrap()).unwrap();
        assert!(
            analysis.diagnostics().is_empty(),
            "{declaration}: {:?}",
            analysis.diagnostics()
        );
        let source = SourceFile::new(
            "dynamic-trait.kgr",
            format!("{declaration} fn accept(value: {application}) {{}}"),
        );
        let analysis =
            crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        assert_eq!(
            analysis.diagnostics().is_empty(),
            accepted,
            "{declaration}: {:?}",
            analysis.diagnostics()
        );
        if !accepted {
            assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::InvalidInterfaceType { .. }
            )));
        }
        assert_eq!(analysis.into_codegen().is_ok(), accepted);
    }
}

#[test]
fn dynamic_standard_traits_and_caller_known_outputs_use_structural_rules() {
    for (source, accepted) in [
        ("fn accept(value: std::fmt::Debug) {}", true),
        ("fn accept(value: std::fmt::Display) {}", true),
        ("fn accept(value: std::hash::Hash) {}", true),
        (
            "fn erase<T: std::fmt::Debug>(value: T) -> std::fmt::Debug { value }",
            false,
        ),
        ("fn accept(value: PartialEq) {}", false),
        ("fn accept(value: Eq) {}", false),
        (
            "trait Source { type Item; fn read(self) -> Self::Item; } fn get<T>(source: Source<Item = T>) -> T { source.read() }",
            true,
        ),
        (
            "trait View { fn clone(self) -> Self; } struct Holder { val view: View }",
            false,
        ),
        (
            "trait View { fn clone(self) -> Self; } enum Holder { Some(View) }",
            false,
        ),
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new("structural-trait.kgr", source),
            foundation_catalog::shared().unwrap(),
        )
        .unwrap();
        assert_eq!(
            analysis.diagnostics().is_empty(),
            accepted,
            "{source}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), accepted, "{source}");
    }
}

#[test]
fn implementation_headers_use_complete_protocol_evidence() {
    for (argument, valid) in [
        ("Key", true),
        ("(Key, i32)", true),
        ("[Key]", true),
        ("Missing", false),
    ] {
        let source = SourceFile::new(
            "header-bounds.kgr",
            format!(
                r#"
struct Key {{}}
impl PartialEq for Key {{ fn eq(self, other: Self) -> bool {{ true }} }}
struct Missing {{}}
trait Use<T: PartialEq> {{ fn get(self) -> i32; }}
struct C {{}}
impl Use<{argument}> for C {{ fn get(self) -> i32 {{ 42 }} }}
fn main() -> i32 {{ C {{}}.get() }}
"#
            ),
        );
        let result = crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        if valid {
            result
                .into_checked()
                .expect("complete applicable implementation");
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
fn dynamic_associated_bounds_cannot_retain_erased_self() {
    for (bound, valid) in [("i32", true), ("Self", false)] {
        let source = SourceFile::new(
            "associated-dynamic.kgr",
            format!(
                r#"
trait Related<T> {{}}
impl<T> Related<T> for i32 {{}}
trait Read {{ type Item: Related<{bound}>; fn read(self) -> Self::Item; }}
struct Reader {{}}
impl Read for Reader {{ type Item = i32; fn read(self) -> i32 {{ 42 }} }}
fn main() -> i32 {{ val view: Read<Item = i32> = Reader {{}}; view.read() }}
"#
            ),
        );
        let result = crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        if valid {
            result
                .into_checked()
                .expect("receiver-independent associated bound");
        } else {
            assert!(
                result.diagnostics().iter().any(|diagnostic| matches!(
                    diagnostic.kind,
                    DiagnosticKind::InvalidInterfaceType { .. }
                )),
                "{:?}",
                result.diagnostics()
            );
        }
    }
}

#[test]
fn associated_impl_bounds_require_the_concrete_self_receiver() {
    for (receiver, valid) in [("Reader", true), ("Other", false)] {
        let source = SourceFile::new(
            "associated-impl-self.kgr",
            format!(
                r#"
trait Related<T> {{}}
trait Read {{ type Item: Related<Self>; }}
struct Reader {{}}
struct Other {{}}
struct Item {{}}
impl Related<{receiver}> for Item {{}}
impl Read for Reader {{ type Item = Item; }}
fn main() {{}}
"#
            ),
        );
        let result = crate::analyze_source(&source, foundation_catalog::shared().unwrap()).unwrap();
        if valid {
            result.into_checked().expect("Item: Related<Reader>");
        } else {
            assert!(
                result.diagnostics().iter().any(|diagnostic| matches!(
                    diagnostic.kind,
                    DiagnosticKind::InvalidAssociatedType { .. }
                )),
                "{:?}",
                result.diagnostics()
            );
        }
    }
}
