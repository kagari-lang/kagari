use crate::test_support::execution::{self, Route};
use kagari_source::source::SourceFile;

fn execute_contextual_source(source: &str, expected: i32) {
    let engine = execution::engine(Default::default());
    let artifact = engine
        .compile_source(SourceFile::new("context.kgr", source))
        .unwrap();
    execution::assert_returns_i32(
        &engine,
        artifact,
        &[Route::Source, Route::Artifact, Route::ArtifactJit],
        expected,
    );
}

#[test]
fn irrefutable_match_stops_before_unreachable_generic_arms() {
    for (pattern, result) in [("_", "42"), ("value", "value")] {
        execute_contextual_source(
            &format!(
                "fn grow<T>(x: T) -> i32 {{ grow((x, x)) }} fn main() -> i32 {{ match 42 {{ {pattern} => {result}, _ => grow(1) }} }}"
            ),
            42,
        );
    }
}

#[test]
fn contextual_phantom_layouts_execute_from_source_and_encoded_artifacts() {
    execute_contextual_source(
        "struct Marker<T> { val value: i32 } struct Outer<T> { val marker: Marker<T> } enum Tag<T> { Empty, Data(Marker<T>) } impl<T> PartialEq for Tag<T> { fn eq(self, other: Self) -> bool { match (self, other) { (Tag::Empty, Tag::Empty) => true, (Tag::Data(a), Tag::Data(b)) => a.value == b.value, _ => false } } } fn tag() -> Tag<i32> { Tag::Empty } fn build() -> (Outer<i32>, [Outer<bool>]) { (if true { Outer { marker: Marker { value: 20 } } } else { Outer { marker: Marker { value: 0 } } }, [match 1 { 1 => Outer { marker: Marker { value: 22 } }, _ => Outer { marker: Marker { value: 0 } } }]) } fn main() -> i32 { val a: Outer<i32> = Outer { marker: Marker { value: 20 } }; val b: Outer<bool> = Outer { marker: Marker { value: 22 } }; val result = build(); val empty: Tag<i32> = Tag::Empty; val payload: Tag<bool> = Tag::Data(Marker { value: 5 }); if empty == tag() { result[0].marker.value + result[1][0].marker.value + a.marker.value + b.marker.value } else { 0 } }",
        84,
    );
}

#[test]
fn annotated_const_dependencies_execute_independently_of_declaration_order() {
    for declarations in [
        "const ANSWER: i32 = BASE + 2; const BASE: i32 = 40;",
        "const BASE: i32 = 40; const ANSWER: i32 = BASE + 2;",
        "const ANSWER = BASE + 2; const BASE: i32 = 40;",
    ] {
        execute_contextual_source(&format!("{declarations} fn main() -> i32 {{ ANSWER }}"), 42);
    }
}
