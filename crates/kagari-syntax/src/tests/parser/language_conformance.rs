use kagari_common::{DiagnosticKind, Severity};

use crate::tests::common;

struct SyntaxCase {
    spec_section: &'static str,
    feature: &'static str,
    source: &'static str,
}

#[test]
fn syntax_spec_positive_cases_are_grouped_by_language_feature() {
    let cases = [
        SyntaxCase {
            spec_section: "docs/spec/syntax.md#modules-and-imports",
            feature: "inline modules, public imports, aliases, and glob imports",
            source: r#"
pub mod gameplay {
    pub use crate::math::{Vector as Vec2, ops::*};
    pub const TICK_RATE: i32 = 60;
}
"#,
        },
        SyntaxCase {
            spec_section: "docs/spec/syntax.md#functions",
            feature: "generic functions with parameter bounds and where clauses",
            source: r#"
fn identity<T: Clone>(value: T) -> T
where T: Clone
{
    value
}
"#,
        },
        SyntaxCase {
            spec_section: "docs/spec/syntax.md#structs-and-enums",
            feature: "val and var fields plus enum tuple payloads",
            source: r#"
pub struct Player<T> {
    val id: T,
    pub var name: String,
}

enum Maybe<T> {
    None,
    Some(T),
}
"#,
        },
        SyntaxCase {
            spec_section: "docs/spec/traits.md#trait-declarations",
            feature: "trait declarations and trait impl blocks",
            source: r#"
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
"#,
        },
        SyntaxCase {
            spec_section: "docs/spec/syntax.md#blocks-and-statements",
            feature: "val and var bindings, assignment, while, loop, break, and tail if",
            source: r#"
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
"#,
        },
        SyntaxCase {
            spec_section: "docs/spec/syntax.md#patterns",
            feature: "tuple patterns, wildcard patterns, arrays, structs, fields, and indexes",
            source: r#"
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
"#,
        },
    ];

    for case in cases {
        let parse = common::parse(case.source);
        assert!(
            parse.diagnostics().is_empty(),
            "{} ({}) should parse without diagnostics, got {:?}",
            case.spec_section,
            case.feature,
            parse.diagnostics()
        );
    }
}

#[test]
fn syntax_spec_rejects_non_spec_rust_forms() {
    let cases = [
        (
            "docs/spec/syntax.md#binding-and-field-writeability",
            "let bindings",
            "fn main() { let value = 1; }",
            DiagnosticKind::InvalidLetBinding,
        ),
        (
            "docs/spec/syntax.md#module-structure",
            "static items",
            "pub static mut COUNTER: i32 = 0;",
            DiagnosticKind::InvalidStaticItem,
        ),
        (
            "docs/spec/traits.md#scope-exclusions",
            "dyn trait object syntax",
            "fn apply(effect: dyn Effect) {}",
            DiagnosticKind::InvalidDynTraitSyntax,
        ),
        (
            "docs/spec/syntax.md#ordinary-parameter-semantics",
            "ref parameters",
            "fn update(ref value: i32) {}",
            DiagnosticKind::InvalidRefParameter,
        ),
        (
            "docs/spec/syntax.md#impl-blocks-and-methods",
            "mut self receivers",
            "fn update(mut self: Player) {}",
            DiagnosticKind::InvalidReceiverModifier,
        ),
        (
            "docs/spec/syntax.md#structs-and-enums",
            "fields without val or var",
            "struct Player { hp: i32 }",
            DiagnosticKind::ExpectedFieldBinding,
        ),
    ];

    for (spec_section, feature, source, expected) in cases {
        let parse = common::parse(source);
        assert!(
            parse.diagnostics().iter().any(|diagnostic| {
                diagnostic.severity == Severity::Error && diagnostic.kind == expected
            }),
            "{spec_section} ({feature}) should reject with {expected:?}, got {:?}",
            parse.diagnostics()
        );
    }
}
