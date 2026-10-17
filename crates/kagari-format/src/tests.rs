use crate::{FormatError, Options, format};
use kagari_common::cancellation::CancellationToken;
use kagari_source::source::SourceFile;
use std::{fs, path::Path};

fn formatted(text: &str, width: usize) -> String {
    format(
        &SourceFile::new("test.kgr", text),
        Options {
            max_width: width,
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn functions_bindings_and_width_have_expected_layout() {
    let text = "fn add(a:i32,b:i32)->i32{val sum=a+b;sum}";
    let expected = "fn add(a: i32, b: i32) -> i32 {\n    val sum = a + b;\n    sum\n}\n";
    assert_eq!(formatted(text, 100), expected);
    assert_eq!(formatted(expected, 100), expected);
    assert_eq!(
        formatted("fn call(){consume(first,second,third);}", 25),
        "fn call() {\n    consume(\n        first,\n        second,\n        third\n    );\n}\n"
    );
}

#[test]
fn adjacency_interpolation_comments_and_boundaries_are_idempotent() {
    for text in [
        "fn f(x:i32)->i32{x>>2<<1}",
        "fn f(){val copy=Point{x:1,/* base */..make()};val all=Point{..base};}",
        "fn f(){val x=Point{x:1,.. // base\nmake()};}",
        "fn f(x:Outer<Inner<i32>>){val y=x;}",
        "fn f(){val s=f\"π {1+2} literal {{brace}}\";}",
        "fn f(){val s=f\"{1 // comment\n + 2}\";}",
        "// heading\nfn f(){/* dangling /* nested */ */}// trailing  ",
        "fn f(){val x=1;// trailing\nconsume(x);}",
        "fn f(){/* before */ val x=1; /* after */}",
        "fn f(){}\r\n",
        "",
        "  \n\t",
        "fn f()->i32{true}",
    ] {
        let once = formatted(text, 40);
        assert_eq!(formatted(&once, 40), once, "{text}");
    }
    assert_eq!(formatted("fn f(){}\r\n", 100), "fn f() {}\r\n");
    assert_eq!(formatted("", 100), "\n");
    assert!(matches!(
        format(
            &SourceFile::new("bad", "fn broken("),
            Options::default(),
            &Default::default()
        ),
        Err(FormatError::Diagnostics(_))
    ));
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        format(
            &SourceFile::new("cancelled", ""),
            Options::default(),
            &cancel
        ),
        Err(FormatError::Cancelled)
    ));
}

fn check_fixture(name: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/syntax")
        .join(name);
    let text = fs::read_to_string(&path).unwrap();
    let source = SourceFile::new(path.display().to_string(), text);
    let once = format(&source, Options::default(), &Default::default())
        .unwrap_or_else(|error| panic!("{}: {error:?}", path.display()));
    assert_eq!(formatted(&once, 100), once, "{}", path.display());
}

#[test]
fn representative_grammar_preserves_structure_and_is_idempotent() {
    for name in [
        "grammar-witnesses.kgr",
        "closures.kgr",
        "binding-patterns.kgr",
        "struct-update.kgr",
        "string-interpolation.kgr",
        "generic-associated-types.kgr",
        "async.kgr",
        "attributes.kgr",
        "inline-modules.kgr",
    ] {
        check_fixture(name);
    }
}

#[test]
#[ignore = "complete grammar fixture matrix runs in CI"]
fn complete_grammar_fixture_inventory() {
    for name in [
        "array-operations.kgr",
        "associated-constants.kgr",
        "associated-types.kgr",
        "async.kgr",
        "attributes.kgr",
        "binding-conditions.kgr",
        "binding-patterns.kgr",
        "bitwise.kgr",
        "blocks.kgr",
        "callables.kgr",
        "closures.kgr",
        "collection-access.kgr",
        "collection-pipelines.kgr",
        "control-flow.kgr",
        "conversions.kgr",
        "data-model.kgr",
        "default-methods.kgr",
        "error-conversion.kgr",
        "expressions.kgr",
        "for-collections.kgr",
        "generic-associated-types.kgr",
        "generic-interfaces.kgr",
        "grammar-witnesses.kgr",
        "import-alias.kgr",
        "index.kgr",
        "inline-modules.kgr",
        "iterator-extensions.kgr",
        "iterators.kgr",
        "literals-and-comments.kgr",
        "match-guards.kgr",
        "match.kgr",
        "numeric-conversions.kgr",
        "operators.kgr",
        "ordering.kgr",
        "pattern-alternatives.kgr",
        "ranges.kgr",
        "result-option.kgr",
        "standard-traits.kgr",
        "struct-update.kgr",
        "string-interpolation.kgr",
        "trait-inheritance.kgr",
        "try-protocols.kgr",
        "tuple-types.kgr",
        "type-inference.kgr",
        "visibility.kgr",
        "where-bounds.kgr",
    ] {
        check_fixture(name);
    }
}

#[test]
fn declarations_types_attributes_and_patterns_have_canonical_layout() {
    for (input, expected) in [
        (
            "#[tool::allow(\"unused_parameter\")]struct Boxed<T>{var value:T}",
            "#[tool::allow(\"unused_parameter\")]\nstruct Boxed<T> {\n    var value: T\n}\n",
        ),
        (
            "enum Value{Empty,Number(i32),}",
            "enum Value {\n    Empty,\n    Number(i32),\n}\n",
        ),
        (
            "trait Item{type Value;const LIMIT:i32;fn get(self)->i32;}",
            "trait Item {\n    type Value;\n    const LIMIT: i32;\n    fn get(self) -> i32;\n}\n",
        ),
        (
            "use thing::{name,other as alias};",
            "use thing::{ name, other as alias };\n",
        ),
        (
            "fn f(value:Option<i32>)->i32{match value{Some(x)=>x+1,None=>0}}",
            "fn f(value: Option<i32>) -> i32 {\n    match value {\n        Some(x) => x + 1,\n        None => 0\n    }\n}\n",
        ),
        (
            "fn f(value:Point)->i32{match value{Point{x,..}=>x}}",
            "fn f(value: Point) -> i32 {\n    match value {\n        Point { x, .. } => x\n    }\n}\n",
        ),
        (
            "fn f(pair:(i32,i32)){var(x,y):(i32,i32)=pair;val _=x+y;}",
            "fn f(pair: (i32, i32)) {\n    var (x, y): (i32, i32) = pair;\n    val _ = x + y;\n}\n",
        ),
        (
            "fn f(x:fn((i32,bool),[i32])->i32){val values=[1,2,3];val pair=(1,true);}",
            "fn f(x: fn((i32, bool), [i32]) -> i32) {\n    val values = [1, 2, 3];\n    val pair = (1, true);\n}\n",
        ),
    ] {
        assert_eq!(formatted(input, 100), expected, "{input}");
        assert_eq!(formatted(expected, 100), expected);
    }
}
