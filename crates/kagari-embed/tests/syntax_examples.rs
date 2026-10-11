use kagari_embed::{
    BytecodeArtifact, context::ExecutionContext, error::EmbeddingError, program::PreparedProgram,
};
use kagari_source::source::SourceFile;

use kagari_runtime::value::Value;

// Share the SDK runner across integration binaries without a production dependency.
#[path = "support/execution.rs"]
mod execution;

macro_rules! syntax_examples {
    ($($name:ident: $path:literal => $source:literal),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                let engine = execution::engine(Default::default());
                let artifact = engine.compile_to_artifact(
                    SourceFile::new($path, include_str!($source)), Default::default(),
                ).unwrap();
                execution::assert_returns_i32(&engine, artifact,
                    &[execution::Route::Source, execution::Route::Artifact], 42);
            }
        )*
    };
}

syntax_examples! {
    callables: "examples/syntax/callables.kgr" => "../../../examples/syntax/callables.kgr",
    array_operations: "examples/syntax/array-operations.kgr" => "../../../examples/syntax/array-operations.kgr",
    bitwise: "examples/syntax/bitwise.kgr" => "../../../examples/syntax/bitwise.kgr",
    numeric_conversions: "examples/syntax/numeric-conversions.kgr" => "../../../examples/syntax/numeric-conversions.kgr",
    numeric_6502_numeric: "examples/6502-numeric.kgr" => "../../../examples/6502-numeric.kgr",
    error_conversion: "examples/syntax/error-conversion.kgr" => "../../../examples/syntax/error-conversion.kgr",
    type_inference: "examples/syntax/type-inference.kgr" => "../../../examples/syntax/type-inference.kgr",
    iterator_extensions: "examples/syntax/iterator-extensions.kgr" => "../../../examples/syntax/iterator-extensions.kgr",
    collection_pipelines: "examples/syntax/collection-pipelines.kgr" => "../../../examples/syntax/collection-pipelines.kgr",
    collection_access: "examples/syntax/collection-access.kgr" => "../../../examples/syntax/collection-access.kgr",
    string_interpolation: "examples/syntax/string-interpolation.kgr" => "../../../examples/syntax/string-interpolation.kgr",
    iterators: "examples/syntax/iterators.kgr" => "../../../examples/syntax/iterators.kgr",
    conversions: "examples/syntax/conversions.kgr" => "../../../examples/syntax/conversions.kgr",
    ordering: "examples/syntax/ordering.kgr" => "../../../examples/syntax/ordering.kgr",
    operators: "examples/syntax/operators.kgr" => "../../../examples/syntax/operators.kgr",
    index: "examples/syntax/index.kgr" => "../../../examples/syntax/index.kgr",
    standard_traits: "examples/syntax/standard-traits.kgr" => "../../../examples/syntax/standard-traits.kgr",
    result_option: "examples/syntax/result-option.kgr" => "../../../examples/syntax/result-option.kgr",
    generic_associated_types: "examples/syntax/generic-associated-types.kgr" => "../../../examples/syntax/generic-associated-types.kgr",
    associated_constants: "examples/syntax/associated-constants.kgr" => "../../../examples/syntax/associated-constants.kgr",
    default_methods: "examples/syntax/default-methods.kgr" => "../../../examples/syntax/default-methods.kgr",
    trait_inheritance: "examples/syntax/trait-inheritance.kgr" => "../../../examples/syntax/trait-inheritance.kgr",
    generic_interfaces: "examples/syntax/generic-interfaces.kgr" => "../../../examples/syntax/generic-interfaces.kgr",
    associated_types: "examples/syntax/associated-types.kgr" => "../../../examples/syntax/associated-types.kgr",
    visibility: "examples/syntax/visibility.kgr" => "../../../examples/syntax/visibility.kgr",
    inline_modules: "examples/syntax/inline-modules.kgr" => "../../../examples/syntax/inline-modules.kgr",
    tuple_types: "examples/syntax/tuple-types.kgr" => "../../../examples/syntax/tuple-types.kgr",
    attributes: "examples/syntax/attributes.kgr" => "../../../examples/syntax/attributes.kgr",
    binding_conditions: "examples/syntax/binding-conditions.kgr" => "../../../examples/syntax/binding-conditions.kgr",
    pattern_alternatives: "examples/syntax/pattern-alternatives.kgr" => "../../../examples/syntax/pattern-alternatives.kgr",
    match_guards: "examples/syntax/match-guards.kgr" => "../../../examples/syntax/match-guards.kgr",
    ranges: "examples/syntax/ranges.kgr" => "../../../examples/syntax/ranges.kgr",
    blocks: "examples/syntax/blocks.kgr" => "../../../examples/syntax/blocks.kgr",
    literals_and_comments: "examples/syntax/literals-and-comments.kgr" => "../../../examples/syntax/literals-and-comments.kgr",
    closures: "examples/syntax/closures.kgr" => "../../../examples/syntax/closures.kgr",
    data_model: "examples/syntax/data-model.kgr" => "../../../examples/syntax/data-model.kgr",
    expressions: "examples/syntax/expressions.kgr" => "../../../examples/syntax/expressions.kgr",
    control_flow: "examples/syntax/control-flow.kgr" => "../../../examples/syntax/control-flow.kgr",
    for_collections: "examples/syntax/for-collections.kgr" => "../../../examples/syntax/for-collections.kgr",
    r#match: "examples/syntax/match.kgr" => "../../../examples/syntax/match.kgr",
    where_bounds: "examples/syntax/where-bounds.kgr" => "../../../examples/syntax/where-bounds.kgr",
    import_alias: "examples/syntax/import-alias.kgr" => "../../../examples/syntax/import-alias.kgr",
    interface_dispatch: "examples/interface-dispatch.kgr" => "../../../examples/interface-dispatch.kgr",
    generic_trait_methods: "examples/generic-trait-methods.kgr" => "../../../examples/generic-trait-methods.kgr",
    standard_library: "examples/standard-library.kgr" => "../../../examples/standard-library.kgr",
}

#[test]
fn pattern_alternatives_require_the_same_bindings() {
    let engine = execution::engine(Default::default());
    let error = engine
        .compile_to_artifact(
            SourceFile::new(
                "bad-or-pattern.kgr",
                "fn main() -> i32 { match (1, 2) { (1, x) | (2, y) => x, _ => 0 } }",
            ),
            Default::default(),
        )
        .unwrap_err();
    assert!(format!("{error:?}").contains("the same bindings in every alternative"));
}

#[test]
fn attributes_without_compiler_behavior_are_rejected_before_execution() {
    let engine = execution::engine(Default::default());
    for (attribute, expected) in [
        ("#[requires(role = \"admin\")]", "KG_ATTRIBUTE_UNSUPPORTED"),
        ("#[unregistered]", "KG_ATTRIBUTE_UNKNOWN"),
    ] {
        let source = format!("{attribute} fn main() -> i32 {{ 42 }}");
        let error = engine
            .compile_to_artifact(
                SourceFile::new("attribute-rejection.kgr", source),
                Default::default(),
            )
            .unwrap_err();
        assert!(
            format!("{error:?}").contains(expected),
            "{attribute}: {error:?}"
        );
    }
}

#[test]
fn grouped_standard_globs_execute() {
    let engine = execution::engine(Default::default());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "standard-glob.kgr",
                "use std::{collections::*}; fn main() -> i32 { var total=0;for value in Vec::from([22,20]).map(|value|value){total+=value;}total }",
            ),

            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
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
fn nested_inline_modules_resolve_qualified_members() {
    let engine = execution::engine(Default::default());
    let source = "pub mod outer { pub mod other { pub fn value() -> i32 { 14 } } pub mod inner { use super::other::value; pub fn call() -> i32 { value() } } } mod facade { pub use super::outer as api; pub use super::outer::other::value as answer; } use self::facade::{api as m, answer as direct}; fn main() -> i32 { m::other::value() + direct() + m::inner::call() }";
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("nested-inline.kgr", source),
            Default::default(),
        )
        .unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    drop(engine);
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
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
fn inline_module_errors_point_into_the_original_source() {
    let engine = execution::engine(Default::default());
    let id = engine
        .set_source(
            "mem://inline-error",
            "mod child { pub fn value() -> i32 { missing } } fn main() -> i32 { child::value() }"
                .into(),
            kagari_source::source_database::SourceLayer::Base,
        )
        .unwrap();
    let error = engine
        .compile_snapshot(engine.source_snapshot(), id, &Default::default())
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected diagnostics");
    };
    assert!(
        diagnostics.iter().any(
            |diagnostic| diagnostic.span.is_some_and(|span| span.file == id
                && span.range.start >= "mod child { pub fn value() -> i32 { ".len())
        ),
        "{diagnostics:?}"
    );
}

#[test]
fn for_loop_detects_structural_change_and_cleans_up_after_trap() {
    let source = r#"
fn main() -> i32 {
    val values = Vec::from([1, 2]);
    for value in values {
        values.push(3);
    }
    0
}

fn after_trap() -> i32 { 42 }
"#;
    let engine = execution::engine(Default::default());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("for-mutation.kgr", source),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
    assert!(format!("{error:?}").contains("invalid or structurally modified iterator"));
    let result = runtime
        .execute(&loaded, "after_trap", &[], &context)
        .unwrap();
    assert_eq!(
        result
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn closure_trap_releases_execution_resources() {
    let engine = execution::engine(Default::default());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "closure_trap.kgr",
                r#"
fn fail() -> i32 {
    var count = 40;
    val fail = || { count = count + 1; 1 / 0 };
    fail()
}
fn after() -> i32 { 42 }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    assert!(runtime.execute(&loaded, "fail", &[], &context).is_err());
    assert_eq!(
        runtime
            .execute(&loaded, "after", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn remainder_matches_checked_arithmetic_contract() {
    let engine = execution::engine(Default::default());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "remainder.kgr",
                r#"
fn integer() -> i32 { 42 % 5 }
fn fractional() -> f32 { 5.5 % 2.0 }
fn zero() -> i32 { val divisor = 0; 5 % divisor }
fn overflow() -> i32 { val minimum = -2147483648; minimum % -1 }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "integer", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(2)
    );
    assert_eq!(
        runtime
            .execute(&loaded, "fractional", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::F32(1.5)
    );
    for (name, message) in [
        ("zero", "integer remainder by zero"),
        ("overflow", "integer overflow"),
    ] {
        let error = runtime.execute(&loaded, name, &[], &context).unwrap_err();
        assert!(format!("{error:?}").contains(message), "{name}: {error:?}");
    }
}
