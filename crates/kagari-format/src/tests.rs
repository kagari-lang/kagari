use crate::{FormatError, Newline, Options, format};
use kagari_common::cancellation::CancellationToken;
use kagari_source::source::SourceFile;
use rstest::rstest;
use std::{
    fs,
    path::{Path, PathBuf},
};

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
#[case::static_attributes_and_bindings("static-attributes-and-bindings", include_str!("../tests/fixtures/layout/static-attributes-and-bindings.input.kgr"), include_str!("../tests/fixtures/layout/static-attributes-and-bindings.expected.kgr"), 100, true, Some(30))]
#[case::static_inline_module_and_block("static-inline-module-and-block", include_str!("../tests/fixtures/layout/static-inline-module-and-block.input.kgr"), include_str!("../tests/fixtures/layout/static-inline-module-and-block.expected.kgr"), 100, true, Some(30))]
#[case::static_binding_comment("static-binding-comment", include_str!("../tests/fixtures/layout/static-binding-comment.input.kgr"), include_str!("../tests/fixtures/layout/static-binding-comment.expected.kgr"), 100, true, Some(30))]
#[case::attributed_generic_struct("attributed-generic-struct", include_str!("../tests/fixtures/layout/attributed-generic-struct.input.kgr"), include_str!("../tests/fixtures/layout/attributed-generic-struct.expected.kgr"), 100, true, None)]
#[case::enum_variants("enum-variants", include_str!("../tests/fixtures/layout/enum-variants.input.kgr"), include_str!("../tests/fixtures/layout/enum-variants.expected.kgr"), 100, true, None)]
#[case::trait_associated_items("trait-associated-items", include_str!("../tests/fixtures/layout/trait-associated-items.input.kgr"), include_str!("../tests/fixtures/layout/trait-associated-items.expected.kgr"), 100, true, None)]
#[case::grouped_use_alias("grouped-use-alias", include_str!("../tests/fixtures/layout/grouped-use-alias.input.kgr"), include_str!("../tests/fixtures/layout/grouped-use-alias.expected.kgr"), 100, true, None)]
#[case::match_enum_pattern("match-enum-pattern", include_str!("../tests/fixtures/layout/match-enum-pattern.input.kgr"), include_str!("../tests/fixtures/layout/match-enum-pattern.expected.kgr"), 100, true, None)]
#[case::match_struct_rest_pattern("match-struct-rest-pattern", include_str!("../tests/fixtures/layout/match-struct-rest-pattern.input.kgr"), include_str!("../tests/fixtures/layout/match-struct-rest-pattern.expected.kgr"), 100, true, None)]
#[case::tuple_binding_pattern("tuple-binding-pattern", include_str!("../tests/fixtures/layout/tuple-binding-pattern.input.kgr"), include_str!("../tests/fixtures/layout/tuple-binding-pattern.expected.kgr"), 100, true, None)]
#[case::callable_array_tuple_types("callable-array-tuple-types", include_str!("../tests/fixtures/layout/callable-array-tuple-types.input.kgr"), include_str!("../tests/fixtures/layout/callable-array-tuple-types.expected.kgr"), 100, true, None)]
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

#[rstest]
#[case::crate_visible_static_trailing_comment(
    "#[tag] pub(crate) static val cache: Vec<i32> = make(); // retained\n"
)]
#[case::parent_visible_static_in_module("mod state { pub(super) static var count: i32 = 0; }")]
fn static_visibility_is_idempotent(#[case] input: &str) {
    let once = formatted(input, 40);
    assert_eq!(formatted(&once, 40), once);
}

#[rstest]
#[case::missing_type("static val count = 0;")]
#[case::missing_initializer("static var count: i32;")]
#[case::local_static("fn f() { static val count: i32 = 0; }")]
#[case::impl_static("impl S { static var count: i32 = 0; }")]
fn static_invalid_declarations_are_not_formatted(#[case] input: &str) {
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

#[rstest]
#[case::empty("''")]
#[case::multiple_scalars("'ab'")]
#[case::combining_sequence("'e\u{301}'")]
#[case::physical_tab("'\t'")]
#[case::physical_lf("'\n'")]
#[case::physical_cr("'\r'")]
#[case::non_ascii_hex_escape(r"'\x80'")]
#[case::leading_underscore_unicode_escape(r"'\u{_41}'")]
#[case::overlong_unicode_escape(r"'\u{0000000}'")]
#[case::surrogate(r"'\u{D800}'")]
#[case::above_unicode_max(r"'\u{110000}'")]
fn char_literals_with_invalid_decoded_values_are_not_formatted(#[case] literal: &str) {
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

#[rstest]
#[case::obsolete_f_prefix(r#"f"old""#)]
#[case::hole_expression(r#""{name + 1}""#)]
#[case::hole_member(r#""{name.value}""#)]
#[case::hole_trailing_space(r#""{name }""#)]
#[case::hole_leading_space(r#""{ name}""#)]
#[case::hole_debug_space(r#""{name: ?}""#)]
#[case::hole_debug_comment(r#""{name:/* comment */?}""#)]
#[case::hole_unknown_format(r#""{name:x}""#)]
#[case::unmatched_close_brace(r#""}""#)]
#[case::string_non_ascii_hex_escape(r#""\x80""#)]
#[case::string_leading_underscore_unicode(r#""\u{_41}""#)]
#[case::string_surrogate(r#""\u{D800}""#)]
#[case::cooked_bare_cr("\"standalone\rcarriage\"")]
#[case::raw_bare_cr("r\"standalone\rcarriage\"")]
#[case::raw_unclosed_hash("r#\"unclosed\"")]
#[case::byte_non_ascii_scalar("b'é'")]
#[case::byte_empty("b''")]
#[case::byte_multiple("b'ab'")]
#[case::byte_physical_tab("b'\t'")]
#[case::byte_unicode_escape(r"b'\u{41}'")]
#[case::bytes_unicode_escape(r#"b"\u{41}""#)]
#[case::bytes_bad_hex(r#"b"\xFG""#)]
#[case::bytes_non_ascii("b\"é\"")]
#[case::raw_bytes_non_ascii("br\"é\"")]
#[case::bytes_bare_cr("b\"standalone\rcarriage\"")]
#[case::raw_bytes_bare_cr("br\"standalone\rcarriage\"")]
#[case::raw_bytes_unclosed_hash("br#\"unclosed\"")]
fn malformed_text_literals_are_not_formatted(#[case] literal: &str) {
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

#[rstest]
#[case::shift_adjacency("fn f(x:i32)->i32{x>>2<<1}")]
#[case::struct_update_block_comment(
    "fn f(){val copy=Point{x:1,/* base */..make()};val all=Point{..base};}"
)]
#[case::struct_update_line_comment("fn f(){val x=Point{x:1,.. // base\nmake()};}")]
#[case::nested_generic_closers("fn f(x:Outer<Inner<i32>>){val y=x;}")]
#[case::unicode_interpolation_escaped_braces(
    "fn f(){val sum=1+2;val s=\"π {sum} literal {{brace}}\";}"
)]
#[case::operator_line_comment("fn f(){val sum=1 // comment\n + 2;val s=\"{sum}\";}")]
#[case::nested_dangling_and_trailing_comments(
    "// heading\nfn f(){/* dangling /* nested */ */}// trailing  "
)]
#[case::binding_trailing_comment("fn f(){val x=1;// trailing\nconsume(x);}")]
#[case::binding_surrounding_comments("fn f(){/* before */ val x=1; /* after */}")]
#[case::generic_local_function_value(
    "fn f(){fn id<T>(x:T)->T{x}val callback=id::<i32>;callback(42);}"
)]
#[case::local_function_body_and_tail_comments(
    "fn outer()->i32{val result=inner(41);/* helper */ fn inner(value:i32)->i32{// body\nvalue+1}/* tail */result}"
)]
#[case::local_function_attribute_and_async_closure(
    "fn outer(){#[tool::allow(\"unused_parameter\")] async fn inner(value:i32)->i32{value}val closure=async || 42;}"
)]
#[case::crlf_source("fn f(){}\r\n")]
#[case::empty_source("")]
#[case::whitespace_only("  \n\t")]
#[case::semantic_error_is_formattable("fn f()->i32{true}")]
fn adjacency_interpolation_comments_and_boundaries_are_idempotent(#[case] text: &str) {
    let once = formatted(text, 40);
    assert_eq!(formatted(&once, 40), once, "{text}");
}

#[test]
fn preserves_crlf_and_canonical_empty_output() {
    assert_eq!(formatted("fn f(){}\r\n", 100), "fn f() {}\r\n");
    assert_eq!(formatted("", 100), "\n");
}

#[test]
fn rejects_malformed_source() {
    assert!(matches!(
        format(
            &SourceFile::new("bad", "fn broken("),
            Options::default(),
            &Default::default()
        ),
        Err(FormatError::Diagnostics(_))
    ));
}

#[test]
fn pre_cancelled_formatting_returns_no_output() {
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

#[rstest]
#[case::grammar_witnesses("grammar-witnesses.kgr")]
#[case::closures("closures.kgr")]
#[case::binding_patterns("binding-patterns.kgr")]
#[case::struct_update("struct-update.kgr")]
#[case::string_interpolation("string-interpolation.kgr")]
#[case::generic_associated_types("generic-associated-types.kgr")]
#[case::async_declarations("async.kgr")]
#[case::attributes("attributes.kgr")]
#[case::inline_modules("inline-modules.kgr")]
fn representative_grammar_preserves_structure_and_is_idempotent(#[case] name: &str) {
    check_fixture(name);
}

#[rstest]
#[ignore = "complete grammar fixture matrix runs in CI"]
fn complete_grammar_fixture_inventory(
    #[files("../../examples/syntax/*.kgr")]
    #[base_dir = "../../examples/syntax"]
    path: PathBuf,
) {
    check_fixture(path.file_name().unwrap().to_str().unwrap());
}
