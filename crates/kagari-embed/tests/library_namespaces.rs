use kagari_embed::{
    BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::PreparedProgram,
};
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;

#[test]
fn reexports_preserve_trait_type_and_constructor_identity_in_artifacts() {
    let engine = KagariEngine::new(Default::default());
    let artifact = engine.compile_to_artifact(SourceFile::new("namespaces.kgr", r#"
use core::ops::Add as CoreAdd;
use std::ops::Add as StdAdd;
use core::hash::Hash as CoreHash;
use std::hash::Hash as StdHash;
use std::collections::HashMap;
struct Number { val value: i32 }
impl PartialEq for Number { fn eq(self, other: Number) -> bool { self.value == other.value } }
impl Eq for Number {}
impl CoreAdd<Number> for Number {
    type Output = Number;
    fn add(self, other: Number) -> Number { Number { value: self.value + other.value } }
}
impl CoreHash for Number { fn hash(self) -> i64 { self.value.hash() } }
fn plus<T: StdAdd<T>>(left: T, right: T) -> T::Output { left + right }
fn hash<T: StdHash>(value: T) -> i64 { value.hash() }
fn main() -> i32 {
    val values: std::vec::Vec<i32> = alloc::vec::Vec::new();
    values.push(20);
    val other: alloc::vec::Vec<i32> = Vec::new();
    other.push(22);
    val text: std::string::String = "hello";
    val result: core::option::Option<i32> = std::option::Option::Some(plus(Number { value: values[0] }, Number { value: other[0] }).value);
    val map: HashMap<i32, i32> = HashMap::new();
    map.insert(1, hash(Number { value: 42 }) as i32);
    if text.len() != 5usize { return 0; }
    match result { Some(value) => value + map.len() as i32 - 1, None => 0 }
}
"#), Default::default()).unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn explicit_prelude_requires_imports_and_local_names_shadow_defaults() {
    let engine = KagariEngine::new(Default::default());
    for name in [
        "Hash",
        "Debug",
        "Display",
        "Add<i32>",
        "HashMap<i32, i32>",
        "HashSet<i32>",
        "List<i32>",
    ] {
        let source = format!("fn missing(value: {name}) {{}}");
        assert!(
            engine
                .compile_to_artifact(SourceFile::new("missing.kgr", source), Default::default())
                .is_err(),
            "{name}"
        );
    }
    for source in [
        "fn identity(value: Vec<i32>) -> Option<Vec<i32>> { Some(value) }",
        "struct Vec { val value: i32 } fn main() -> i32 { Vec { value: 42 }.value }",
        "use std::ops::Add; fn plus<T: Add<T>>(a: T, b: T) -> T::Output { a + b }",
        "use std::iter::Iterable; fn source<T: Iterable>(value: T) -> T::Iter { value.iter() }",
    ] {
        engine
            .compile_to_artifact(SourceFile::new("prelude.kgr", source), Default::default())
            .unwrap();
    }
    for source in [
        "use core::language::Add; fn main() {}",
        "fn main() { val values: ArrayList<i32> = []; }",
        "mod std {} fn main(value: std::vec::Vec<i32>) {}",
    ] {
        assert!(
            engine
                .compile_to_artifact(SourceFile::new("retired.kgr", source), Default::default())
                .is_err()
        );
    }
}
