use kagari_common::span::Span;
use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};
use kagari_syntax::{
    ast::visit::{ElementRef, LexicalToken, NodeRef},
    parser::{ParseLimits, ParseMode, ParseOptions, parse},
    token::TokenKind,
};

use crate::traversal;

pub(crate) fn check(text: &str) {
    // Keep the entire input as source. Small budgets vary with the input without
    // reserving a binary header that would make source fixtures unreplayable.
    let small = ParseLimits {
        max_diagnostics: text.len() % 5,
        max_nesting: usize::from(text.as_bytes().first().copied().unwrap_or(0)) % 9,
        max_tree_depth: usize::from(text.as_bytes().last().copied().unwrap_or(0)) % 17,
    };
    let source = SourceFile::new("fuzz.kgr", text);
    for mode in [ParseMode::Source, ParseMode::Declarations] {
        for limits in [
            ParseLimits::default(),
            ParseLimits {
                max_diagnostics: small.max_diagnostics,
                ..ParseLimits::default()
            },
            ParseLimits {
                max_nesting: small.max_nesting,
                ..ParseLimits::default()
            },
            ParseLimits {
                max_tree_depth: small.max_tree_depth,
                ..ParseLimits::default()
            },
            small,
        ] {
            check_parse(&source, ParseOptions { mode, limits });
        }
    }
}

fn check_parse(source: &SourceFile, options: ParseOptions) {
    let text = source.text();
    let parsed = parse(source, options, &Default::default())
        .expect("an uncancelled parse returns a recoverable snapshot");
    assert_eq!(parsed.mode(), options.mode);
    let mut offset = 0;
    let mut lexical = String::new();
    let mut expected = Vec::new();
    for token in parsed.tokens() {
        check_token(token, text);
        assert_eq!(token.span().start, offset, "lexical partition has a gap");
        offset = token.span().end;
        lexical.push_str(token.text());
        expected.push((token.lexical_kind(), token.span()));
    }
    assert_eq!(offset, text.len());
    assert_eq!(lexical, text);

    let mut ordinary = 0;
    let mut exhausted = 0;
    for diagnostic in parsed.diagnostics() {
        if let Some(span) = diagnostic.span {
            checked_slice(text, span);
        }
        if matches!(diagnostic.kind, DiagnosticKind::CompileLimitExceeded { .. }) {
            exhausted += 1;
        } else {
            ordinary += 1;
        }
    }
    assert!(ordinary <= options.limits.max_diagnostics);
    assert!(exhausted <= 1);

    let root = parsed.module();
    let mut retained = Vec::new();
    for node in NodeRef::ModuleBody(root.body()).descendants() {
        if let NodeRef::ModuleDef(module) = node
            && let Some(body) = module.body()
        {
            assert!(body.location().resolve(&parsed).is_some());
            retained.push(body);
        }
    }
    // Retained module handles, including inline bodies, own their source backing.
    drop(parsed);
    let mut grammar = Coverage {
        expected: &expected,
        position: 0,
    };
    check_node(NodeRef::ModuleBody(root.body()), text, &mut grammar);
    assert_eq!(
        grammar.position,
        expected.len(),
        "grammar traversal omitted the lexical suffix"
    );
    traversal::check_physical(NodeRef::ModuleBody(root.body()));
    traversal::check_owned(&root);
    drop(root);
    for module in retained {
        assert_eq!(module.source().text(), text);
        checked_slice(text, module.full_span());
        module.visit_owned(&mut |node| {
            assert_eq!(node.text(), checked_slice(text, node.body_span()));
            true
        });
        traversal::check_owned(&module);
    }
    // Scope exit also exercises recursive destruction of recovered AST nodes.
}

/// Compare real lexical identities, including trivia and EOF. Equal spellings
/// at different source positions must never substitute for each other.
struct Coverage<'a> {
    expected: &'a [(TokenKind, Span)],
    position: usize,
}

impl Coverage<'_> {
    fn token(&mut self, token: LexicalToken<'_>) {
        assert_eq!(
            self.expected.get(self.position),
            Some(&(token.lexical_kind(), token.span())),
            "grammar traversal lost, duplicated or reordered a lexical token"
        );
        self.position += 1;
    }

    fn operator(&mut self, first: LexicalToken<'_>, last: LexicalToken<'_>) {
        self.token(first);
        if first.span() == last.span() {
            return;
        }
        // Operators expose endpoints rather than a lexical iterator. Expand the
        // contiguous physical pieces from the independently retained lexer table.
        while self
            .expected
            .get(self.position)
            .is_some_and(|(_, span)| span.start < last.span().start)
        {
            let (kind, span) = &self.expected[self.position];
            assert!(!kind.is_trivia() && *kind != TokenKind::Eof);
            assert!(span.end <= last.span().start);
            self.position += 1;
        }
        self.token(last);
    }
}

fn check_node(node: NodeRef<'_>, text: &str, grammar: &mut Coverage<'_>) {
    assert_eq!(node.text(), checked_slice(text, node.body_span()));
    checked_slice(text, node.full_span());
    if let Some(name) = node.declaration_name() {
        assert_eq!(name.source_text(), checked_slice(text, name.body_span()));
    }
    for attribute in node.attributes() {
        assert_eq!(
            attribute.source_text(),
            checked_slice(text, attribute.body_span())
        );
    }
    for token in [node.first_token(), node.last_token()]
        .into_iter()
        .flatten()
    {
        check_token(token, text);
    }
    for element in node.elements() {
        match element {
            ElementRef::Node(child) => check_node(child, text, grammar),
            ElementRef::Token(token) => {
                check_token(token, text);
                grammar.token(token);
            }
            ElementRef::Operator(operator) => {
                let first = operator.first_token();
                let last = operator.last_token();
                check_token(first, text);
                check_token(last, text);
                assert_eq!(
                    operator.text(),
                    checked_slice(text, Span::new(first.span().start, last.span().end))
                );
                grammar.operator(first, last);
            }
            ElementRef::Missing { at } => {
                assert!(text.is_char_boundary(at as usize));
            }
        }
    }
}

fn check_token(token: LexicalToken<'_>, text: &str) {
    assert_eq!(token.text(), checked_slice(text, token.span()));
}

fn checked_slice(text: &str, span: Span) -> &str {
    text.get(span.start..span.end)
        .expect("source ranges must be ordered, in bounds and on UTF-8 boundaries")
}
