use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};
use kagari_stdlib::catalog;

const INTERFACES: &str = r#"
trait X { fn x(self) -> i32; }
trait Y { fn y(self) -> i32; }
trait A: X + Y {}
trait B: X + Y {}
trait C: X {}
trait D: A {}
trait E: A {}
"#;

#[test]
fn common_interfaces_consider_all_completing_arms_before_selection() {
    for [first, second, third] in [
        ["a", "b", "c"],
        ["a", "c", "b"],
        ["b", "a", "c"],
        ["b", "c", "a"],
        ["c", "a", "b"],
        ["c", "b", "a"],
    ] {
        let source = SourceFile::new(
            "join.kgr",
            format!(
                "{INTERFACES}
fn choose(tag: i32, a: A, b: B, c: C) -> i32 {{
    val joined = match tag {{ 0 => {first}, 1 => {second}, _ => {third} }};
    joined.x()
}}"
            ),
        );
        let analysis = crate::analyze_source(&source, catalog::shared().unwrap()).unwrap();
        assert!(
            analysis.diagnostics().is_empty(),
            "{first}/{second}/{third}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_ok());
    }
}

#[test]
fn common_interfaces_preserve_ambiguity_dominance_and_branch_context() {
    for (body, accepted) in [
        (
            "val joined = match tag { 0 => a, _ => b }; joined.x()",
            false,
        ),
        (
            "val joined = match tag { 0 => b, _ => a }; joined.x()",
            false,
        ),
        (
            "val joined: X = match tag { 0 => a, _ => b }; joined.x()",
            true,
        ),
        (
            "val joined: Y = match tag { 0 => b, _ => a }; joined.y()",
            true,
        ),
        (
            "val joined = match tag { 0 => d, _ => e }; val exact: A = joined; exact.x()",
            true,
        ),
        (
            "val joined = if tag == 0 { a } else { c }; joined.x()",
            true,
        ),
        (
            "val joined = if tag == 0 { a } else { b }; joined.x()",
            false,
        ),
        (
            "val joined: Y = if tag == 0 { a } else { b }; joined.y()",
            true,
        ),
        (
            "val joined = match tag { 0 => { return 42; }, 1 => a, _ => c }; joined.x()",
            true,
        ),
        (
            "val joined = match tag { 0 => a, _ => c, 1 => b }; joined.x()",
            true,
        ),
        (
            "val joined = match tag { 0 => 1u8, _ => 2 }; val exact: u8 = joined; 42",
            true,
        ),
        (
            "val joined = match tag { 0 => None, _ => Some(42) }; val exact: Option<i32> = joined; 42",
            true,
        ),
        (
            "val left: std::collections::HashMap<i32, i32> = std::collections::HashMap::new(); val right: std::collections::HashMap<i32, i32> = std::collections::HashMap::new(); val joined = match tag { 0 => left, _ => right }; joined.insert(1, 42); 42",
            true,
        ),
        (
            "val view: Iterable<Item = i32> = Vec::from([1]); val joined = match tag { 0 => Vec::from([2]), _ => view }; val exact: Iterable<Item = i32> = joined; 42",
            true,
        ),
        (
            "val view: Iterable<Item = i32> = Vec::from([1]); val joined = match tag { 0 => view, _ => Vec::from([2]) }; val exact: Iterable<Item = i32> = joined; 42",
            true,
        ),
    ] {
        let source = SourceFile::new(
            "join-context.kgr",
            format!(
                "{INTERFACES} fn choose(tag: i32, a: A, b: B, c: C, d: D, e: E) -> i32 {{ {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, catalog::shared().unwrap()).unwrap();
        assert_eq!(
            analysis.diagnostics().is_empty(),
            accepted,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        if !accepted {
            assert!(
                analysis.diagnostics().iter().any(|diagnostic| matches!(
                    &diagnostic.kind, DiagnosticKind::AmbiguousCommonInterface { candidates }
                        if candidates == &["X", "Y"]
                )),
                "{body}: {:?}",
                analysis.diagnostics()
            );
        }
        assert_eq!(analysis.into_codegen().is_ok(), accepted, "{body}");
    }
}

#[test]
fn common_interfaces_preserve_associated_outputs() {
    for (output, accepted) in [("i32", true), ("i64", false)] {
        let source = SourceFile::new(
            "join-associated.kgr",
            format!(
                r#"
trait Carrier {{ type Item; fn item(self) -> Self::Item; }}
trait Left: Carrier<Item = i32> {{}}
trait Right: Carrier<Item = {output}> {{}}
fn choose(flag: bool, left: Left, right: Right) -> i32 {{
    val joined = if flag {{ left }} else {{ right }};
    joined.item()
}}
"#
            ),
        );
        let analysis = crate::analyze_source(&source, catalog::shared().unwrap()).unwrap();
        assert_eq!(
            analysis.diagnostics().is_empty(),
            accepted,
            "{output}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), accepted, "{output}");
    }
}

#[test]
fn common_interfaces_check_ordinary_implementation_bounds() {
    let declarations = r#"
trait Read<T> { fn read(self) -> T; }
struct Left<T> { val item: T }
struct Right<T> { val item: T }
impl<T: Eq> Read<T> for Left<T> { fn read(self) -> T { self.item } }
impl<T: Eq> Read<T> for Right<T> { fn read(self) -> T { self.item } }
"#;
    for (signature, accepted) in [
        (
            "fn choose(flag: bool, left: Left<i32>, right: Right<i32>) -> i32",
            true,
        ),
        (
            "fn choose<T: Eq>(flag: bool, left: Left<T>, right: Right<T>) -> T",
            true,
        ),
        (
            "fn choose(flag: bool, left: Left<f32>, right: Right<f32>) -> f32",
            false,
        ),
        (
            "fn choose<T>(flag: bool, left: Left<T>, right: Right<T>) -> T",
            false,
        ),
        (
            "fn choose(flag: bool, left: Left<i32>, right: Right<i64>) -> i32",
            false,
        ),
    ] {
        let source = SourceFile::new(
            "join-bounds.kgr",
            format!(
                "{declarations} {signature} {{ val joined = if flag {{ left }} else {{ right }}; joined.read() }}"
            ),
        );
        let analysis = crate::analyze_source(&source, catalog::shared().unwrap()).unwrap();
        assert_eq!(
            analysis.diagnostics().is_empty(),
            accepted,
            "{signature}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), accepted, "{signature}");
    }
}
