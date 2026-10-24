use crate::test_support::execution::{self, Route};
use kagari_source::source::SourceFile;

#[test]
fn associated_and_where_bounds_support_static_method_calls() {
    for bound in ["type Item: Describe;", "type Item;"] {
        let where_clause = if bound == "type Item;" {
            "where R::Item: Describe"
        } else {
            ""
        };
        let source = format!(
            r#"
            trait Describe {{ fn number(self) -> i32; }}
            struct Number {{ val value: i32 }}
            impl Describe for Number {{ fn number(self) -> i32 {{ self.value }} }}
            trait Reader {{ {bound} fn read(self) -> Self::Item; }}
            struct Source {{ val value: Number }}
            impl Reader for Source {{ type Item = Number; fn read(self) -> Self::Item {{ self.value }} }}
            fn read<R: Reader>(r: R) -> i32 {where_clause} {{ r.read().number() }}
            fn main() -> i32 {{ read(Source {{ value: Number {{ value: 42 }} }}) }}
        "#
        );
        let engine = execution::engine(Default::default());
        let checked = engine
            .compile_source(SourceFile::new("associated.kgr", &source))
            .unwrap();
        execution::assert_returns_i32(
            &engine,
            checked,
            &[Route::Source, Route::Artifact, Route::ArtifactJit],
            42,
        );
    }
}
