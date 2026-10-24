use kagari_source::diagnostic::Severity;
use rstest::rstest;

use crate::tests::common;

#[rstest]
#[case::modules_and_imports(
    "docs/spec/syntax.md#modules-and-imports",
    "inline modules, public imports, aliases, and glob imports",
    r#"
pub mod gameplay {
    pub use crate::math::{Vector as Vec2, ops::*};
    pub const TICK_RATE: i32 = 60;
}
"#
)]
#[case::generic_functions(
    "docs/spec/syntax.md#functions",
    "generic functions with parameter bounds and where clauses",
    r#"
fn identity<T: Clone>(value: T) -> T
where T: Clone
{
    value
}
"#
)]
#[case::structs_and_enums(
    "docs/spec/syntax.md#structs-and-enums",
    "val and var fields plus enum tuple payloads",
    r#"
pub struct Player<T> {
    val id: T,
    pub var name: String,
}

enum Maybe<T> {
    None,
    Some(T),
}
"#
)]
#[case::traits_and_impls(
    "docs/spec/traits.md#trait-declarations",
    "trait declarations and trait impl blocks",
    r#"
trait Display {
    fn show(self) -> String;
}

struct Player {
    val name: String,
}

impl Display for Player {
    pub fn show(self) -> String {
        self.name
    }
}
"#
)]
#[case::blocks_and_statements(
    "docs/spec/syntax.md#blocks-and-statements",
    "val and var bindings, assignment, while, loop, break, and tail if",
    r#"
fn main() -> i32 {
    var total = 0;
    while total < 3 {
        total = total + 1;
    }
    loop {
        break;
    }
    if total == 3 { total } else { 0 }
}
"#
)]
#[case::patterns_and_collections(
    "docs/spec/syntax.md#patterns",
    "tuple patterns, wildcard patterns, arrays, structs, fields, and indexes",
    r#"
struct Point {
    val x: i32,
    val y: i32,
}

fn main() -> i32 {
    val point = Point { x: 1, y: 2 };
    val pair = (point.x, [1, 2][0]);
    match pair {
        (_, value) => value,
    }
}
"#
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
#[case::ref_parameter(
    "docs/spec/syntax.md#ordinary-parameter-semantics",
    "ref parameters",
    "fn update(ref value: i32) {}"
)]
#[case::mut_receiver(
    "docs/spec/syntax.md#impl-blocks-and-methods",
    "mut self receivers",
    "fn update(mut self: Player) {}"
)]
#[case::missing_field_binding(
    "docs/spec/syntax.md#structs-and-enums",
    "fields without val or var",
    "struct Player { hp: i32 }"
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
