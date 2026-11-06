use crate::{
    lexer::lex,
    lexical::Lexical,
    parser::{ParseMode, ParseOptions, core::Parser, parse},
    token::TokenKind,
};
use kagari_source::source::SourceFile;

#[test]
fn comparison_lookahead_stops_at_its_statement_boundary() {
    for count in [1, 16, 256, 4_000] {
        let source = SourceFile::new("comparisons.kgr", "x < y; ".repeat(count));
        let tokens = lex(source.text());
        let starts = tokens
            .iter()
            .enumerate()
            .filter_map(|(index, token)| (token.kind == TokenKind::Lt).then_some(index + 1))
            .collect::<Vec<_>>();
        let lexical = Lexical::new(source.text(), tokens, &Default::default()).unwrap();
        let parser = Parser::new(
            &source,
            lexical,
            Default::default(),
            Default::default(),
            ParseMode::Source,
        );
        let mut visited = 0;
        for start in starts {
            let mut cursor = start;
            assert!(!parser.skip_angle_group(&mut cursor));
            assert_eq!(
                parser.lexical.kind(cursor - 1),
                TokenKind::Semi,
                "lookahead escaped its independent statement at size {count}"
            );
            visited += cursor - start;
        }
        assert_eq!(visited, count * 3, "work is local to each comparison");
    }
}

#[test]
fn angle_lookahead_preserves_type_shapes_and_delimiter_ownership() {
    for arguments in [
        "i32",
        "Vec<Vec<i32>>",
        "Item = (i32, [u8]),",
        "crate::types::Item, super::Other, self::Value",
        "fn((i32, [u8]), !) -> [Vec<i32>]",
        "<T as Trait<Item = U>>::Assoc<V>",
        "<T as Fn((i32, [u8])) -> V>::Output",
    ] {
        let text = format!("<{arguments}>; next");
        let source = SourceFile::new("types.kgr", &text);
        let lexical = Lexical::new(source.text(), lex(source.text()), &Default::default()).unwrap();
        let parser = Parser::new(
            &source,
            lexical,
            Default::default(),
            Default::default(),
            ParseMode::Source,
        );
        let mut cursor = 1;
        assert!(parser.skip_angle_group(&mut cursor), "{arguments}");
        assert_eq!(
            parser.nth_nontrivia_kind_from(&mut cursor),
            Some(TokenKind::Semi)
        );
        for expression in [
            format!("Item<{arguments}> {{}}"),
            format!("Item<{arguments}>::make()"),
            format!("make::<Reader<{arguments}>>()"),
        ] {
            let source = SourceFile::new("expressions.kgr", format!("fn f() {{ {expression}; }}"));
            let parsed = parse(&source, ParseOptions::default(), &Default::default()).unwrap();
            assert!(
                parsed.diagnostics().is_empty(),
                "{expression}: {:?}",
                parsed.diagnostics()
            );
        }
    }
}

#[test]
fn angle_lookahead_rejects_non_type_tokens_and_mismatched_delimiters() {
    for text in ["<T; next >", "<T + U>", "<[T)>", "<(T]>", "<fn(T> -> U)"] {
        let source = SourceFile::new("not-types.kgr", text);
        let lexical = Lexical::new(source.text(), lex(source.text()), &Default::default()).unwrap();
        let parser = Parser::new(
            &source,
            lexical,
            Default::default(),
            Default::default(),
            ParseMode::Source,
        );
        assert!(!parser.skip_angle_group(&mut 1), "{text}");
    }
}
