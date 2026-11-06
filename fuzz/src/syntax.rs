use kagari_common::span::Span;
use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};
use kagari_syntax::{
    ast::visit::{ElementRef, LexicalToken, NodeRef},
    parser::{ParseLimits, ParseMode, ParseOptions, parse},
};

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
        for limits in [ParseLimits::default(), small] {
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
    let mut expected = String::new();
    for token in parsed.tokens() {
        check_token(token, text);
        assert_eq!(token.span().start, offset, "lexical partition has a gap");
        offset = token.span().end;
        lexical.push_str(token.text());
        if !token.lexical_kind().is_trivia() {
            expected.push_str(token.text());
        }
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
    let mut grammar = String::new();
    check_node(NodeRef::ModuleBody(root.body()), text, &mut grammar);
    assert_eq!(
        grammar, expected,
        "grammar traversal lost or duplicated tokens"
    );
    drop(root);
    for module in retained {
        assert_eq!(module.source().text(), text);
        checked_slice(text, module.full_span());
        module.visit_owned(&mut |node| {
            assert_eq!(node.text(), checked_slice(text, node.body_span()));
            true
        });
    }
    // Scope exit also exercises recursive destruction of recovered AST nodes.
}

fn check_node(node: NodeRef<'_>, text: &str, grammar: &mut String) {
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
                if !token.lexical_kind().is_trivia() {
                    grammar.push_str(token.text());
                }
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
                grammar.push_str(operator.text());
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
