use crate::{FormatError, Newline, Options, format};
use kagari_common::cancellation::CancellationToken;
use kagari_source::source::SourceFile;
use rstest::rstest;
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

#[rstest]
#[case::type_aliases(
    "type-aliases",
    include_str!("../tests/fixtures/layout/type-aliases.input.kgr"),
    include_str!("../tests/fixtures/layout/type-aliases.expected.kgr"),
    100,
    true,
    Some(30)
)]
#[case::module_where_clause(
    "type-alias-module-where-clause",
    include_str!("../tests/fixtures/layout/type-alias-module-where-clause.input.kgr"),
    include_str!("../tests/fixtures/layout/type-alias-module-where-clause.expected.kgr"),
    100,
    true,
    Some(30)
)]
#[case::commented_callback(
    "type-alias-commented-callback",
    include_str!("../tests/fixtures/layout/type-alias-commented-callback.input.kgr"),
    include_str!("../tests/fixtures/layout/type-alias-commented-callback.expected.kgr"),
    100,
    true,
    Some(30)
)]
#[case::trailing_where_clause(
    "type-alias-trailing-where-clause",
    include_str!("../tests/fixtures/layout/type-alias-trailing-where-clause.input.kgr"),
    include_str!("../tests/fixtures/layout/type-alias-trailing-where-clause.expected.kgr"),
    100,
    true,
    Some(30)
)]
#[case::function_bindings(
    "function-bindings",
    include_str!("../tests/fixtures/layout/function-bindings.input.kgr"),
    include_str!("../tests/fixtures/layout/function-bindings.expected.kgr"),
    100,
    true,
    None
)]
#[case::async_blocks_comments_postfix(
    "async-blocks-comments-postfix",
    include_str!("../tests/fixtures/layout/async-blocks-comments-postfix.input.kgr"),
    include_str!("../tests/fixtures/layout/async-blocks-comments-postfix.expected.kgr"),
    100,
    true,
    None
)]
#[case::narrow_call(
    "narrow-call",
    include_str!("../tests/fixtures/layout/narrow-call.input.kgr"),
    include_str!("../tests/fixtures/layout/narrow-call.expected.kgr"),
    25,
    false,
    None
)]
fn layout_fixtures(
    #[case] fixture: &str,
    #[case] input: &str,
    #[case] expected: &str,
    #[case] width: usize,
    #[case] check_idempotence: bool,
    #[case] narrow_width: Option<usize>,
) {
    assert_eq!(
        formatted(input, width),
        expected,
        "fixture=tests/fixtures/layout/{fixture}; width={width}"
    );
    if check_idempotence {
        assert_eq!(
            formatted(expected, width),
            expected,
            "fixture=tests/fixtures/layout/{fixture}; width={width}; idempotence"
        );
    }
    if let Some(width) = narrow_width {
        let narrow = formatted(input, width);
        assert_eq!(
            formatted(&narrow, width),
            narrow,
            "fixture=tests/fixtures/layout/{fixture}; width={width}; narrow idempotence"
        );
    }
}

#[test]
fn static_declarations_preserve_layout_comments_and_structure() {
    for (input, expected) in [
        (
            "#[tag]pub static val cache:Vec<i32> =make(1,2);static var count:i32=0;",
            "#[tag]\npub static val cache: Vec<i32> = make(1, 2);\n\nstatic var count: i32 = 0;\n",
        ),
        (
            "mod state{static var count:i32=0;static val seed:i32={val x=1;x};}",
            "mod state {\n    static var count: i32 = 0;\n    static val seed: i32 = {\n        val x = 1;\n        x\n    };\n}\n",
        ),
        (
            "static /* binding */ val value:i32=make(first,second,third);",
            "static /* binding */ val value: i32 = make(first, second, third);\n",
        ),
    ] {
        assert_eq!(formatted(input, 100), expected);
        assert_eq!(formatted(expected, 100), expected);
        let narrow = formatted(input, 30);
        assert_eq!(formatted(&narrow, 30), narrow);
    }
    for input in [
        "#[tag] pub(crate) static val cache: Vec<i32> = make(); // retained\n",
        "mod state { pub(super) static var count: i32 = 0; }",
    ] {
        let once = formatted(input, 40);
        assert_eq!(formatted(&once, 40), once);
    }
}

#[test]
fn static_invalid_declarations_are_not_formatted() {
    for input in [
        "static val count = 0;",
        "static var count: i32;",
        "fn f() { static val count: i32 = 0; }",
        "impl S { static var count: i32 = 0; }",
    ] {
        assert!(
            matches!(
                format(
                    &SourceFile::new("bad", input),
                    Options::default(),
                    &Default::default()
                ),
                Err(FormatError::Diagnostics(_))
            ),
            "{input}"
        );
    }
}

#[rstest]
#[case::nested_async_blocks("fn f(){async{async{42}}}")]
#[case::commented_body_await_postfix("async fn f(){async // body\n{request().await?}.await;}")]
#[case::pending_in_interpolation(
    "fn f(){val pending=async{42};val text=\"before {pending} after\";}"
)]
fn async_blocks_preserve_layout_comments_and_postfix(#[case] input: &str) {
    let once = formatted(input, 30);
    assert_eq!(formatted(&once, 30), once, "input={input:?}; width=30");
}

#[test]
fn char_literals_preserve_spelling_and_structure() {
    check_fixture("char-literals.kgr");
    let input = r#"fn f(){val c:char='🙂';val close='}';val open='\u{7B_}';val s="{close} {open}";match c{'a'..='z'=>'\x7F',_=>'\''}}"#;
    for eol in ["\n", "\r\n"] {
        let source = format!("{input}{eol}");
        let once = formatted(&source, 30);
        for literal in [
            "'🙂'",
            "'}'",
            r"'\u{7B_}'",
            "'a'",
            "'z'",
            r"'\x7F'",
            r"'\''",
        ] {
            assert!(once.contains(literal), "{once}");
        }
        assert!(once.ends_with(eol));
        assert_eq!(formatted(&once, 30), once);
    }
}

#[test]
fn char_literals_with_invalid_decoded_values_are_not_formatted() {
    for literal in [
        "''",
        "'ab'",
        "'e\u{301}'",
        "'\t'",
        "'\n'",
        "'\r'",
        r"'\x80'",
        r"'\u{_41}'",
        r"'\u{0000000}'",
        r"'\u{D800}'",
        r"'\u{110000}'",
    ] {
        let text = format!("fn f() {{ val c = {literal}; }}");
        assert!(
            matches!(
                format(
                    &SourceFile::new("bad.kgr", text),
                    Options::default(),
                    &Default::default()
                ),
                Err(FormatError::Diagnostics(_))
            ),
            "{literal:?}"
        );
    }
}

#[test]
fn text_literals_preserve_multiline_bytes_and_hole_boundaries() {
    check_fixture("byte-literals.kgr");
    for eol in ["\n", "\r\n"] {
        for literal in [
            "\"\n  first\n\"",
            "\"\n{name}\n  {name:?}\n\"",
            "\"first\\\n  \t\n second\"",
            "r\"\n  {name} \\n\n\"",
            "r##\"\n  quote \"# {name}\n\"##",
            r#""\x00\x7F\u{1_F642_} \u{7B} {{}} {name}""#,
            "b\"\n  first {name:?}\n\"",
            "b\"first\\\n  \t\n second\"",
            "br\"\n  {name} \\n\n\"",
            "br##\"\n  quote \"# {name}\n\"##",
            r#"b"\x00\x7F\x80\xFF {} {{}} {name:?}""#,
            "b\"\"",
            "b'A'",
            r"b'\xFF'",
            r"b'\''",
        ] {
            let literal = literal.replace('\n', eol);
            let input = format!("fn f(name:String){{consume({literal});}}{eol}");
            for newline in [Newline::Preserve, Newline::Lf, Newline::Crlf] {
                let options = Options {
                    max_width: 20,
                    newline,
                    ..Default::default()
                };
                let once = format(
                    &SourceFile::new("text.kgr", &input),
                    options,
                    &Default::default(),
                )
                .unwrap();
                assert!(once.contains(&literal), "literal changed: {once:?}");
                let twice = format(
                    &SourceFile::new("text.kgr", &once),
                    options,
                    &Default::default(),
                )
                .unwrap();
                assert_eq!(twice, once);
            }
        }
    }
    for hashes in [0, 1, 255] {
        let delimiter = "#".repeat(hashes);
        for prefix in ["r", "br"] {
            let literal = format!("{prefix}{delimiter}\"raw {{name}} \\n\"{delimiter}");
            let once = formatted(&format!("fn f(){{{literal}}}"), 20);
            assert!(once.contains(&literal));
            assert_eq!(formatted(&once, 20), once);
        }
    }
}

#[test]
fn malformed_text_literals_are_not_formatted() {
    for literal in [
        r#"f"old""#,
        r#""{name + 1}""#,
        r#""{name.value}""#,
        r#""{name }""#,
        r#""{ name}""#,
        r#""{name: ?}""#,
        r#""{name:/* comment */?}""#,
        r#""{name:x}""#,
        r#""}""#,
        r#""\x80""#,
        r#""\u{_41}""#,
        r#""\u{D800}""#,
        "\"standalone\rcarriage\"",
        "r\"standalone\rcarriage\"",
        "r#\"unclosed\"",
        "b'é'",
        "b''",
        "b'ab'",
        "b'\t'",
        r"b'\u{41}'",
        r#"b"\u{41}""#,
        r#"b"\xFG""#,
        "b\"é\"",
        "br\"é\"",
        "b\"standalone\rcarriage\"",
        "br\"standalone\rcarriage\"",
        "br#\"unclosed\"",
    ] {
        let input = format!("fn f(){{{literal}}}");
        assert!(
            matches!(
                format(
                    &SourceFile::new("bad.kgr", input),
                    Options::default(),
                    &Default::default()
                ),
                Err(FormatError::Diagnostics(_))
            ),
            "{literal}"
        );
    }
}

#[test]
fn adjacency_interpolation_comments_and_boundaries_are_idempotent() {
    for text in [
        "fn f(x:i32)->i32{x>>2<<1}",
        "fn f(){val copy=Point{x:1,/* base */..make()};val all=Point{..base};}",
        "fn f(){val x=Point{x:1,.. // base\nmake()};}",
        "fn f(x:Outer<Inner<i32>>){val y=x;}",
        "fn f(){val sum=1+2;val s=\"π {sum} literal {{brace}}\";}",
        "fn f(){val sum=1 // comment\n + 2;val s=\"{sum}\";}",
        "// heading\nfn f(){/* dangling /* nested */ */}// trailing  ",
        "fn f(){val x=1;// trailing\nconsume(x);}",
        "fn f(){/* before */ val x=1; /* after */}",
        "fn f(){fn id<T>(x:T)->T{x}val callback=id::<i32>;callback(42);}",
        "fn outer()->i32{val result=inner(41);/* helper */ fn inner(value:i32)->i32{// body\nvalue+1}/* tail */result}",
        "fn outer(){#[tool::allow(\"unused_parameter\")] async fn inner(value:i32)->i32{value}val closure=async || 42;}",
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
        "byte-literals.kgr",
        "callables.kgr",
        "char-literals.kgr",
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
