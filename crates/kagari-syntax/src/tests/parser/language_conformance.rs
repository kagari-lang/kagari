use kagari_source::diagnostic::Severity;
use rstest::rstest;

use crate::tests::common;

#[rstest]
#[case::modules_and_imports(
    "docs/spec/syntax.md#modules-and-imports",
    "inline modules, public imports, aliases, and glob imports",
    include_str!("../../../tests/fixtures/parser/conformance/modules_and_imports.kgr")
)]
#[case::generic_functions(
    "docs/spec/syntax.md#functions",
    "generic functions with parameter bounds and where clauses",
    include_str!("../../../tests/fixtures/parser/conformance/generic_functions.kgr")
)]
#[case::structs_and_enums(
    "docs/spec/syntax.md#structs-and-enums",
    "val and var fields plus enum tuple payloads",
    include_str!("../../../tests/fixtures/parser/conformance/structs_and_enums.kgr")
)]
#[case::traits_and_impls(
    "docs/spec/traits.md#trait-declarations",
    "trait declarations and trait impl blocks",
    include_str!("../../../tests/fixtures/parser/conformance/traits_and_impls.kgr")
)]
#[case::blocks_and_statements(
    "docs/spec/syntax.md#blocks-and-statements",
    "val and var bindings, assignment, while, loop, break, and tail if",
    include_str!("../../../tests/fixtures/parser/conformance/blocks_and_statements.kgr")
)]
#[case::patterns_and_collections(
    "docs/spec/syntax.md#patterns",
    "tuple patterns, wildcard patterns, arrays, structs, fields, and indexes",
    include_str!("../../../tests/fixtures/parser/conformance/patterns_and_collections.kgr")
)]
fn syntax_spec_positive_cases(
    #[case] spec_section: &str,
    #[case] feature: &str,
    #[case] source: &str,
) {
    let parse = common::parse(source);
    assert!(
        parse.diagnostics().is_empty(),
        "{spec_section} ({feature}) should parse without diagnostics, got {:?}",
        parse.diagnostics()
    );
}

#[rstest]
#[case::let_binding(
    "docs/spec/syntax.md#binding-and-field-writeability",
    "let bindings",
    "fn main() { let value = 1; }"
)]
#[case::static_mut(
    "docs/spec/syntax.md#module-structure",
    "static mut instead of static val/var",
    "pub static mut COUNTER: i32 = 0;"
)]
#[case::dyn_trait_object(
    "docs/spec/traits.md#scope-exclusions",
    "dyn trait object syntax",
    "fn apply(effect: dyn Effect) {}"
)]
fn syntax_spec_rejects_unrecognized_forms(
    #[case] spec_section: &str,
    #[case] feature: &str,
    #[case] source: &str,
) {
    let parse = common::parse(source);

    assert_eq!(
        parse
            .diagnostics()
            .first()
            .map(|diagnostic| diagnostic.severity),
        Some(Severity::Error),
        "{spec_section} ({feature}) should reject with an error, got {:?}",
        parse.diagnostics()
    );
}
