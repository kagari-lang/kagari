//! Transparent aliases share runtime types while retaining source declaration identity.

use crate::{
    analyze_source,
    declarations::DeclarationId,
    declare_analysis,
    hir::ty::TypeKind,
    imports::{
        ModuleGraph,
        tests::{analyze, insert},
        types::TypeCatalog,
    },
    lower::lower_module_controlled,
    resolver::resolved::ResolvedName,
    tests::test_analysis,
    types::TypeId,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, map::DefinitionContext},
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog as foundation_catalog;
use kagari_syntax::parser::parse_declarations;
use kagari_types::scalar::BuiltinType;
use std::sync::Arc;

fn balanced_tuple(leaf: &str, levels: usize) -> String {
    let mut value = leaf.to_owned();
    for _ in 0..levels {
        value = format!("({value}, {value})");
    }
    value
}

#[test]
fn type_alias_review_projection_predicates_survive_erasure() {
    let declarations = "trait Marker {} trait Carrier { type Item; } struct S {} impl Carrier for S { type Item = bool; } type Erased<T: Carrier> where T::Item: Marker = i32;";
    for (usage, valid) in [
        ("fn bad(x: Erased<S>) {}", false),
        ("impl Marker for bool {} fn good(x: Erased<S>) {}", true),
        (
            "fn forward<T: Carrier>(x: Erased<T>) where T::Item: Marker {}",
            true,
        ),
        (
            "type Forward<T: Carrier> where T::Item: Marker = Erased<T>;",
            true,
        ),
    ] {
        let result = analyze_source(
            &SourceFile::new("alias-predicates.kgr", format!("{declarations} {usage}")),
            foundation_catalog::shared().unwrap(),
        )
        .unwrap();
        if valid {
            assert!(
                result.diagnostics().is_empty(),
                "{usage}: {:?}",
                result.diagnostics()
            );
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
fn type_alias_review_obligation_expansion_is_bounded_before_substitution() {
    let wide = balanced_tuple("T", 10);
    let big = balanced_tuple("i32", 9);
    let text = format!(
        "trait Related<T> {{}} type Wide<T> = {wide}; type Big = {big}; type Erased<T: Related<Wide<T>>> = i32; fn bad(x: Erased<Big>) {{}}"
    );
    let result = analyze_source(
        &SourceFile::new("alias-obligation-limit.kgr", text),
        foundation_catalog::shared().unwrap(),
    )
    .unwrap();
    assert!(
        result.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic.kind,
            DiagnosticKind::CompileLimitExceeded { .. }
        )),
        "expected bounded obligation diagnostic, found {} diagnostics",
        result.diagnostics().len()
    );
}

#[test]
fn type_alias_review_where_clause_owns_projection_context() {
    for projection in ["T::Item", "<T as Carrier>::Item"] {
        let text = format!(
            "trait Carrier {{ type Item; }} type Output<T> where T: Carrier = {projection};"
        );
        let result = analyze_source(
            &SourceFile::new("alias-where.kgr", text),
            foundation_catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(
            result.diagnostics().is_empty(),
            "{projection}: {:?}",
            result.diagnostics()
        );
    }
}

#[test]
fn type_alias_review_imported_supertraits_and_alias_arguments_converge() {
    let mut sources = SourceDatabase::default();
    insert(
        &mut sources,
        "traits",
        "pub trait Base { type Item; } pub trait Child: Base {} pub trait Parent<T> { type Value; } pub type Arg = i32; pub trait Other: Parent<Arg> {}",
    );
    let root = insert(
        &mut sources,
        "root",
        "use pkg::traits::{Child, Other, Parent}; type Output<T: Child> = T::Item; type OtherOutput<T: Other> = <T as Parent<i32>>::Value;",
    );
    let snapshot = analyze(&sources);
    assert!(
        snapshot
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty(),
        "{:?}",
        snapshot.file(root).unwrap().result().diagnostics()
    );
}

#[test]
fn type_alias_local_supertrait_forward_arguments_are_order_independent() {
    for aliases in [
        "type Output<T: Child> = T::Item; type Arg = i32;",
        "type Arg = i32; type Output<T: Child> = T::Item;",
    ] {
        let result = analyze_source(
            &SourceFile::new(
                "alias-local-parents.kgr",
                format!("trait Parent<X> {{ type Item; }} trait Child: Parent<Arg> {{}} {aliases}"),
            ),
            foundation_catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(
            result.diagnostics().is_empty(),
            "{aliases}: {:?}",
            result.diagnostics()
        );
    }
}

#[test]
fn type_alias_composite_retention_is_bounded_across_siblings() {
    let big = balanced_tuple("i32", 11);
    let siblings = vec!["Big"; 16].join(", ");
    for target in [
        format!("({siblings}, Missing)"),
        format!("fn({siblings}) -> Missing"),
    ] {
        let result = analyze_source(
            &SourceFile::new(
                "alias-composite-budget.kgr",
                format!("type Big = {big}; type TooLarge = {target};"),
            ),
            foundation_catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(result.diagnostics().iter().any(|diagnostic| matches!(
            &diagnostic.kind, DiagnosticKind::InvalidTypeAlias { name, .. } if name == "TooLarge"
        )), "{:?}", result.diagnostics());
        let facts = result.facts();
        let table = facts.signatures.facts().type_table();
        let mut retained = 0;
        for (index, (_, reference)) in facts.lowered.module.body.types.iter().enumerate() {
            if matches!(&reference.kind, TypeKind::Named(name) if name == "Big") {
                let resolved = table
                    .type_ref(facts.lowered.source_map.type_id(index))
                    .expect("visited sibling");
                retained += usize::from(!resolved.ty.is_unresolved());
                assert!(
                    resolved.target.is_some(),
                    "oversized siblings retain navigation"
                );
            }
            if matches!(&reference.kind, TypeKind::Named(name) if name == "Missing") {
                let resolved = table
                    .type_ref(facts.lowered.source_map.type_id(index))
                    .expect("later unresolved sibling visited");
                assert!(resolved.ty.is_unresolved());
            }
        }
        assert!(
            retained <= 1,
            "retained {retained} expanded siblings in {target}"
        );
    }
}

#[test]
fn type_alias_review_associated_member_binders_supply_alias_bounds() {
    for (member, definition) in [
        ("Out<T: Marker>", "Out<T: Marker> = Erased<T>"),
        (
            "Out<T> where T: Marker",
            "Out<T> = Erased<T> where T: Marker",
        ),
    ] {
        let text = format!(
            "trait Marker {{}} type Erased<T: Marker> = i32; trait Family {{ type {member}; }} struct S {{}} impl Family for S {{ type {definition}; }}"
        );
        let result = analyze_source(
            &SourceFile::new("alias-family.kgr", text),
            foundation_catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(
            result.diagnostics().is_empty(),
            "{member}: {:?}",
            result.diagnostics()
        );
    }
}

#[test]
fn type_alias_review_dependency_depth_is_declaration_order_independent() {
    let mut accepted = Vec::new();
    for last in [63, 64] {
        let mut items = vec!["type A0 = i32;".to_owned()];
        for i in 1..=last {
            items.push(format!("type A{i} = A{};", i - 1));
        }
        for reverse in [false, true] {
            let text = if reverse {
                items.iter().rev().cloned().collect::<String>()
            } else {
                items.concat()
            };
            let result = analyze_source(
                &SourceFile::new("alias-depth.kgr", text),
                foundation_catalog::shared().unwrap(),
            )
            .unwrap();
            accepted.push(result.diagnostics().is_empty());
        }
    }
    assert_eq!(accepted, [true, true, false, false]);
}

#[test]
fn type_alias_review_final_size_does_not_count_replaced_placeholders() {
    let wide = balanced_tuple("T", 11);
    let result = analyze_source(
        &SourceFile::new(
            "alias-final-size.kgr",
            format!("type Wide<T> = {wide}; fn pass(x: Wide<i32>) {{}}"),
        ),
        foundation_catalog::shared().unwrap(),
    )
    .unwrap();
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
}

#[test]
fn type_alias_review_final_size_does_not_sum_intermediate_expansions() {
    let big = balanced_tuple("i32", 11);
    let result = analyze_source(
        &SourceFile::new(
            "alias-intermediate-size.kgr",
            format!(
                "type Big = {big}; type Identity<T> = T; fn pass(x: Identity<Big>) -> Big {{ x }}"
            ),
        ),
        foundation_catalog::shared().unwrap(),
    )
    .unwrap();
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
}

#[test]
fn type_alias_declaration_mode_rejects_native_only_attributes() {
    for attribute in ["#[builtin_type(Array)]", "#[lang = \"array\"]", "#[native]"] {
        let source = Arc::new(SourceFile::new(
            "alias-metadata.kgr",
            format!("{attribute} pub type Value = i32;"),
        ));
        let cancel = CancellationToken::default();
        let parsed = parse_declarations(&source, Default::default(), &cancel).unwrap();
        assert!(
            parsed.diagnostics().is_empty(),
            "{:?}",
            parsed.diagnostics()
        );
        let mut lowered = lower_module_controlled(source, &parsed.syntax().into(), &cancel);
        // Even an attribute allowlisted by registration cannot give an alias
        // native storage identity. Exercise signature validation independently
        // of the stricter native source/registration correspondence gate.
        lowered.registered_native_api = true;
        for attribute in &lowered.attributes {
            lowered
                .native_attributes
                .insert((attribute.span.start, attribute.span.end));
        }
        let lowered = Arc::new(lowered);
        let context = DefinitionContext::new().unwrap();
        let graph = ModuleGraph::build([lowered.as_ref()], &context, &cancel).unwrap();
        let unit = graph
            .catalog
            .definitions
            .lookup_module(lowered.source.module_identity())
            .unwrap();
        let declared = declare_analysis(
            lowered,
            graph.node(&unit).unwrap().imports.clone(),
            graph.catalog.clone(),
            &context,
            &cancel,
        )
        .unwrap();
        let types = TypeCatalog::new([&declared])
            .bindings(&declared.names.facts, &cancel)
            .unwrap();
        let prepared = declared.check_signatures(types, None, &cancel).unwrap();
        assert!(prepared.signatures.diagnostics().iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidTypeAlias { reason, .. } if reason.contains("attribute"))), "{attribute}: {:?}", prepared.signatures.diagnostics());
    }
}

#[test]
fn type_alias_transparency_substitution_and_nominal_construction() {
    let text = "type Number = Later; type Later = i32; struct Pair<A, B> { val first: A, val second: B } type Swapped<X, Y> = Pair<Y, X>; type Concrete = Swapped<bool, Number>; enum ResultBox<T> { Value(T) } type IntResult = ResultBox<Number>; type Values<T> = [T]; type Callback<T> = fn(T) -> T; fn identity<T>(value: Values<T>) -> [T] { value } fn make() -> Pair<i32, bool> { Concrete { first: 42, second: true } } fn generic() -> Pair<i32, bool> { Swapped<bool, i32> { first: 7, second: false } } fn variant() -> ResultBox<i32> { IntResult::Value(42) } fn apply(f: Callback<Number>, x: Number) -> i32 { f(x) }";
    let result = analyze_source(
        &SourceFile::new("aliases.kgr", text),
        foundation_catalog::shared().unwrap(),
    )
    .unwrap();
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    let facts = result.facts();
    let alias = &facts.lowered.module.type_aliases[0];
    let declaration = facts
        .declarations
        .target(ResolvedName::TypeAlias(alias.id))
        .unwrap();
    let DeclarationId::Definition(id) = declaration.id else {
        panic!("alias definition");
    };
    assert_eq!(
        facts
            .declarations
            .definitions()
            .resolve(id)
            .unwrap()
            .last()
            .unwrap()
            .kind,
        DefinitionKind::TypeAlias
    );
    let canonical = facts.declarations.imported_types().alias(&id).unwrap();
    assert_eq!(canonical.target, TypeId::Builtin(BuiltinType::I32));
    assert!(facts.lowered.module.opaque_types.is_empty());
    assert!(result.into_codegen().is_ok());
}

#[test]
fn type_alias_member_paths_preserve_fixed_owner_arguments() {
    let prefix = "struct Cell<T> { val value: T } impl<T> Cell<T> { fn make(value: T) -> Cell<T> { Cell { value: value } } } type IntCell = Cell<i32>;";
    let valid = format!("{prefix} fn main() -> i32 {{ IntCell::make(42).value }}");
    let result = analyze_source(
        &SourceFile::new("alias-members.kgr", valid),
        foundation_catalog::shared().unwrap(),
    )
    .unwrap();
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    let invalid = format!("{prefix} fn main() {{ IntCell::make(true); }}");
    let result = analyze_source(
        &SourceFile::new("alias-members.kgr", invalid),
        foundation_catalog::shared().unwrap(),
    )
    .unwrap();
    assert!(
        result.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic.kind,
            DiagnosticKind::ArgumentTypeMismatch { .. }
        )),
        "{:?}",
        result.diagnostics()
    );
    assert!(result.into_codegen().is_err());
}

#[test]
fn type_alias_patterns_preserve_fixed_owner_arguments() {
    let prefix = "struct Cell<T> { val value: T } enum Boxed<T> { Value(T) } type IntCell = Cell<i32>; type IntBox = Boxed<i32>; type Generic<T> = Boxed<T>;";
    let valid = format!(
        "{prefix} fn get(cell: Cell<i32>, boxed: Boxed<i32>) -> i32 {{ val IntCell {{ value }} = cell; match boxed {{ IntBox::Value(other) => value + other }} }}"
    );
    let result = analyze_source(
        &SourceFile::new("alias-patterns.kgr", valid),
        foundation_catalog::shared().unwrap(),
    )
    .unwrap();
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    for body in [
        "fn bad(cell: Cell<bool>) { val IntCell { value } = cell; }",
        "fn bad(boxed: Boxed<bool>) { match boxed { IntBox::Value(other) => other }; }",
        "fn bad(boxed: Boxed<i32>) { match boxed { Generic::Value(other) => other }; }",
    ] {
        let result = analyze_source(
            &SourceFile::new("alias-patterns.kgr", format!("{prefix} {body}")),
            foundation_catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(
            result.diagnostics().iter().any(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::PatternTypeMismatch { .. }
            )),
            "{:?}",
            result.diagnostics()
        );
        assert!(result.into_codegen().is_err());
    }
}

#[test]
fn type_alias_generic_constructors_preserve_owner_substitution_and_bounds() {
    let prefix = "struct Pair<A, B> { val first: A, val second: B } enum Either<A, B> { Pair(A, B) } type Swapped<A, B> = Pair<B, A>; type Flipped<A, B> = Either<B, A>; type Fixed = Either<i32, bool>;";
    let valid = format!(
        "{prefix} fn structure() -> Pair<i32, bool> {{ Swapped<bool, i32> {{ first: 42, second: true }} }} fn enumeration() -> Either<i32, bool> {{ Flipped<bool, i32>::Pair(42, true) }}"
    );
    let result = analyze_source(
        &SourceFile::new("alias-constructors.kgr", valid),
        foundation_catalog::shared().unwrap(),
    )
    .unwrap();
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    for expression in [
        "Swapped { first: 42, second: true }",
        "Swapped<bool, i32> { first: true, second: 42 }",
        "Flipped::Pair(42, true)",
        "Flipped<bool, i32>::Pair(true, 42)",
        "Fixed::Pair::<bool, i32>(true, 42)",
    ] {
        let text = format!("{prefix} fn bad() {{ {expression}; }}");
        let result = analyze_source(
            &SourceFile::new("alias-constructors.kgr", text),
            foundation_catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(!result.diagnostics().is_empty(), "accepted {expression}");
        assert!(result.into_codegen().is_err(), "accepted {expression}");
    }
    let erased = "trait Marker {} struct Plain {} enum Unit { Value } type StructAlias<T: Marker> = Plain; type EnumAlias<T: Marker> = Unit; fn bad() { StructAlias<i32> {}; EnumAlias<i32>::Value; }";
    let result = analyze_source(
        &SourceFile::new("alias-constructors.kgr", erased),
        foundation_catalog::shared().unwrap(),
    )
    .unwrap();
    assert_eq!(
        result
            .diagnostics()
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::GenericBoundNotSatisfied { .. }
            ))
            .count(),
        2,
        "{:?}",
        result.diagnostics()
    );
}

#[test]
fn type_alias_expansion_limits_and_cancellation_do_not_expand_unbounded_targets() {
    let mut source = "type A0 = i32;".to_owned();
    for index in 1..16 {
        let prior = index - 1;
        source.push_str(&format!("type A{index} = (A{prior}, A{prior});"));
    }
    let result = analyze_source(
        &SourceFile::new("alias-expansion.kgr", source),
        foundation_catalog::shared().unwrap(),
    )
    .unwrap();
    assert!(result.diagnostics().iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidTypeAlias { reason, .. } if reason.contains("expansion"))), "{:?}", result.diagnostics());
    assert!(result.into_codegen().is_err());
    let cancelled = kagari_common::cancellation::CancellationToken::default();
    cancelled.cancel();
    assert!(!crate::typeck::aliases::within_expansion_limit(
        &TypeId::Builtin(BuiltinType::I32),
        &Default::default(),
        &cancelled
    ));
}

#[test]
fn type_alias_definition_scope_imports_reexports_and_navigation() {
    let mut sources = SourceDatabase::default();
    let library = insert(
        &mut sources,
        "library",
        "pub struct Model { pub val value: i32 } pub type Item = Model; pub type Values<T> = [T];",
    );
    insert(
        &mut sources,
        "facade",
        "pub use pkg::library::{Item as Renamed, Values};",
    );
    let text = "use pkg::facade::{Renamed, Values}; struct Model { val wrong: bool } fn build() -> Renamed { Renamed { value: 42 } } fn pass(x: Values<i32>) -> [i32] { x }";
    let root = insert(&mut sources, "root", text);
    let snapshot = analyze(&sources);
    let file = snapshot.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    snapshot.check_program(root, &Default::default()).unwrap();
    let target = snapshot
        .definition_at(root, text.find("-> Renamed").unwrap() + 3)
        .unwrap();
    assert_eq!(target.name, "Item");
    assert_eq!(target.location.file, library);
    let signature = file
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|function| function.name == "build")
        .unwrap();
    let TypeId::Struct(nominal) = &signature.return_type else {
        panic!("normalized struct");
    };
    assert_eq!(
        file.definitions()
            .resolve(nominal.declaration)
            .unwrap()
            .to_path()
            .module
            .path,
        ["library"]
    );
}

#[test]
fn type_alias_cycles_invalid_targets_and_bounds_are_checked_when_unused() {
    for text in [
        "type A = A;",
        "type A = B; type B = A;",
        "type Grow<T> = Grow<[T]>;",
        "type Left<T> = Right<[T]>; type Right<T> = Left<(T, T)>;",
        "type Missing = Undeclared;",
        "type Generic<T> = [T]; type MissingArgs = Generic;",
        "type Generic<T> = [T]; type ExtraArgs = Generic<i32, bool>;",
        "type Hole = _;",
        "type Erased<T> = i32; type Invalid = Erased<Undeclared>;",
        "trait Marker {} struct Box<T: Marker> { val value: T } type Erased<T> = i32; type Invalid = Erased<Box<f32>>;",
        "struct Box<T: Eq> { val value: T } type Invalid<T> = Box<T>;",
        "trait Marker {} type Erased<T: Marker> = i32; type Invalid = Erased<f32>;",
    ] {
        let result = analyze_source(
            &SourceFile::new("invalid-alias.kgr", text),
            foundation_catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(!result.diagnostics().is_empty(), "accepted {text}");
        assert!(result.into_codegen().is_err(), "accepted {text}");
    }
}

#[test]
fn type_alias_module_cycles_are_diagnosed_without_signature_recursion() {
    let mut sources = SourceDatabase::default();
    let left = insert(
        &mut sources,
        "left",
        "pub type Left<T> = pkg::right::Right<[T]>;",
    );
    let right = insert(
        &mut sources,
        "right",
        "pub type Right<T> = pkg::left::Left<(T, T)>;",
    );
    let snapshot = analyze(&sources);
    for file in [left, right] {
        let diagnostics = snapshot.file(file).unwrap().result().diagnostics();
        assert!(diagnostics.iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidTypeAlias { reason, .. } if reason.contains("cyclic"))), "{diagnostics:?}");
    }
}

#[test]
fn type_alias_visibility_cannot_publish_private_targets_or_import_private_aliases() {
    for text in [
        "struct Hidden {} pub type Leaked = Hidden;",
        "pub(crate) struct Hidden {} pub type Leaked = Hidden;",
        "pub(super) struct Hidden {} pub type Leaked = Hidden;",
        "struct Hidden {} pub type Leaked = [Hidden];",
        "type Hidden = i32; pub type Leaked = Hidden;",
        "struct Hidden {} pub struct Container<T> { val value: T } pub type Leaked = Container<Hidden>;",
    ] {
        let result = analyze_source(
            &SourceFile::new("private-alias.kgr", text),
            foundation_catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(result.diagnostics().iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidTypeAlias { reason, .. } if reason.contains("less visible"))), "{text}: {:?}", result.diagnostics());
    }
    let mut sources = SourceDatabase::default();
    insert(&mut sources, "private", "type Hidden = i32;");
    let root = insert(
        &mut sources,
        "root",
        "use pkg::private::Hidden; fn value(x: Hidden) -> i32 { x }",
    );
    let snapshot = analyze(&sources);
    assert!(
        !snapshot
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    assert!(snapshot.check_program(root, &Default::default()).is_err());
}

#[test]
fn type_alias_input_bounds_survive_erasure_and_declaration_contexts() {
    for text in [
        "trait Marker {} type Erased<T: Marker> = i32; fn bad(x: Erased<f32>) {}",
        "trait Marker {} type Erased<T: Marker> = i32; fn bad() { val x: Erased<f32> = 1; }",
        "trait Marker {} type Erased<T: Marker> = i32; struct Bad { val value: Erased<f32> }",
        "trait Marker {} type Erased<T: Marker> = i32; enum Bad { Value(Erased<f32>) }",
    ] {
        let result = analyze_source(
            &SourceFile::new("alias-bounds.kgr", text),
            foundation_catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(
            result.diagnostics().iter().any(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::GenericBoundNotSatisfied { .. }
            )),
            "{text}: {:?}",
            result.diagnostics()
        );
        assert!(result.into_codegen().is_err());
    }
    let text = "struct Box<T: Eq> { val value: T } type Checked<T: Eq> = Box<T>; type Erased<T> where T: Eq = i32; type Forward<T: Eq> = Erased<T>; fn good<T: Eq>(value: Checked<T>, erased: Forward<T>) -> Box<T> { val n: Erased<T> = erased; value }";
    let result = analyze_source(
        &SourceFile::new("valid-alias-bounds.kgr", text),
        foundation_catalog::shared().unwrap(),
    )
    .unwrap();
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
}

#[test]
fn type_alias_interface_values_are_not_trait_bound_aliases() {
    let valid =
        "trait Read { fn read(self) -> i32; } type Reader = Read; fn pass(x: Reader) -> Read { x }";
    let result = analyze_source(
        &SourceFile::new("alias-interface.kgr", valid),
        foundation_catalog::shared().unwrap(),
    )
    .unwrap();
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    for text in [
        "trait Read {} type Reader = Read; fn bad<T: Reader>() {}",
        "trait Read {} type Reader = Read; struct S {} impl Reader for S {}",
    ] {
        let result = analyze_source(
            &SourceFile::new("alias-interface.kgr", text),
            foundation_catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(
            result.diagnostics().iter().any(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::InvalidTraitReference { .. }
            )),
            "{:?}",
            result.diagnostics()
        );
        assert!(result.into_codegen().is_err());
    }
}

#[test]
fn type_alias_navigation_and_template_survive_body_edit_snapshot_reuse() {
    let text = "fn before() {} type Alias<T> = [T]; fn pass(x: Alias<i32>) -> [i32] { x }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("alias-reuse.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut database = test_analysis();
    let old = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let old_file = old.file(id).unwrap();
    assert!(
        old_file.result().diagnostics().is_empty(),
        "{:?}",
        old_file.result().diagnostics()
    );
    let target = old_file
        .definition_at(text.find("Alias<i32>").unwrap())
        .unwrap();
    let binder = old_file
        .definition_at(text.find("[T]").unwrap() + 1)
        .unwrap();
    let edited = text.replace("fn before() {}", "fn before() { val n = 42; }");
    sources
        .set("alias-reuse.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let new = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = new.file(id).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    assert!(file.signatures_reused());
    assert_eq!(
        file.definition_at(edited.find("Alias<i32>").unwrap())
            .unwrap()
            .id,
        target.id
    );
    assert_eq!(
        file.definition_at(edited.find("[T]").unwrap() + 1)
            .unwrap()
            .id,
        binder.id
    );
}
