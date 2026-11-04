//! Bounded TA1 layout/recovery proof; removed at the production cutover.

use std::{marker::PhantomData, mem::size_of, num::NonZeroU32};

use crate::{lexer::lex, token::TokenKind};

#[derive(Debug, PartialEq, Eq)]
struct Site {
    first: u32,
    end: u32,
    recovery: Option<NonZeroU32>,
}

#[derive(Debug, PartialEq, Eq)]
enum Required<T> {
    Present(T),
    Missing { at: u32 },
}

#[derive(Debug, PartialEq, Eq)]
struct Token<K> {
    index: u32,
    marker: PhantomData<K>,
}

#[derive(Debug, PartialEq, Eq)]
struct Entry<T> {
    element: Required<T>,
    separator: Option<Required<Token<Comma>>>,
}

#[derive(Debug, PartialEq, Eq)]
enum Comma {}

#[derive(Debug, PartialEq, Eq)]
enum Expr {
    Name(Token<()>),
    Binary(Box<Binary>),
    Call(Box<Call>),
    If(Box<If>),
}

#[derive(Debug, PartialEq, Eq)]
struct Binary {
    site: Site,
    lhs: Required<Expr>,
    operator: Required<Token<()>>,
    rhs: Required<Expr>,
}

#[derive(Debug, PartialEq, Eq)]
struct Call {
    site: Site,
    callee: Required<Expr>,
    open: Required<Token<()>>,
    arguments: Vec<Entry<Expr>>,
    close: Required<Token<()>>,
}

#[derive(Debug, PartialEq, Eq)]
struct If {
    site: Site,
    condition: Required<Expr>,
    then_body: Required<(Token<()>, Required<Token<()>>)>,
    else_body: Option<Required<Expr>>,
}

#[derive(Debug, PartialEq, Eq)]
struct Unexpected {
    before_field: u16,
    list_index: u32,
    first: u32,
    end: u32,
}

fn token<K>(index: usize) -> Token<K> {
    Token {
        index: index.try_into().unwrap(),
        marker: PhantomData,
    }
}

#[test]
fn named_roles_lists_and_lossless_storage_have_independent_representation() {
    let text = "if {}";
    let tokens = lex(text);
    let conditional = Expr::If(Box::new(If {
        site: Site {
            first: 0,
            end: 4,
            recovery: None,
        },
        condition: Required::Missing { at: 3 },
        then_body: Required::Present((token(2), Required::Present(token(3)))),
        else_body: None,
    }));
    let Expr::If(conditional) = conditional else {
        unreachable!()
    };
    assert_eq!(conditional.condition, Required::Missing { at: 3 });
    assert!(matches!(conditional.then_body, Required::Present(_)));
    assert!(conditional.else_body.is_none());
    assert_eq!(tokens[2].kind, TokenKind::LBrace);

    let text = "f(a,,b,)";
    let tokens = lex(text);
    let call = Expr::Call(Box::new(Call {
        site: Site {
            first: 0,
            end: 8,
            recovery: None,
        },
        callee: Required::Present(Expr::Name(token(0))),
        open: Required::Present(token(1)),
        arguments: vec![
            Entry {
                element: Required::Present(Expr::Name(token(2))),
                separator: Some(Required::Present(token(3))),
            },
            Entry {
                element: Required::Missing { at: 4 },
                separator: Some(Required::Present(token(4))),
            },
            Entry {
                element: Required::Present(Expr::Name(token(5))),
                separator: Some(Required::Present(token(6))),
            },
        ],
        close: Required::Present(token(7)),
    }));
    let Expr::Call(call) = call else {
        unreachable!()
    };
    assert_eq!(call.arguments.len(), 3);
    assert!(matches!(
        call.arguments[1].element,
        Required::Missing { at: 4 }
    ));
    assert!(call.arguments.last().unwrap().separator.is_some());
    assert_eq!(
        tokens
            .iter()
            .map(|t| &text[t.span.start..t.span.end])
            .collect::<String>(),
        text
    );

    let binary = Expr::Binary(Box::new(Binary {
        site: Site {
            first: 0,
            end: 2,
            recovery: None,
        },
        lhs: Required::Present(Expr::Name(token(0))),
        operator: Required::Present(token(1)),
        rhs: Required::Missing { at: 3 },
    }));
    assert!(
        matches!(binary, Expr::Binary(node) if matches!(node.rhs, Required::Missing { at: 3 }))
    );
    println!(
        "site={} expr={} required_expr={} entry={} binary={} call={} if={}",
        size_of::<Site>(),
        size_of::<Expr>(),
        size_of::<Required<Expr>>(),
        size_of::<Entry<Expr>>(),
        size_of::<Binary>(),
        size_of::<Call>(),
        size_of::<If>()
    );
}

#[test]
fn sparse_recovery_keeps_skipped_text_separate_from_arguments_and_missing_tokens() {
    let text = "f(a @@@, b";
    let tokens = lex(text);
    let first = tokens
        .iter()
        .position(|t| t.kind == TokenKind::Unknown)
        .unwrap();
    let end = tokens
        .iter()
        .position(|t| t.kind == TokenKind::Comma)
        .unwrap();
    let unexpected = Unexpected {
        before_field: 2,
        list_index: 1,
        first: first as u32,
        end: end as u32,
    };
    let mut site = Site {
        first: 0,
        end: (tokens.len() - 1) as u32,
        recovery: None,
    };
    let recoveries = [vec![unexpected]];
    site.recovery = NonZeroU32::new(1);
    let skipped = &recoveries[site.recovery.unwrap().get() as usize - 1][0];
    assert_eq!(skipped.list_index, 1);
    assert_eq!(
        &text[tokens[skipped.first as usize].span.start..tokens[skipped.end as usize - 1].span.end],
        "@@@"
    );
    let closing: Required<Token<()>> = Required::Missing {
        at: text.len() as u32,
    };
    assert!(matches!(closing, Required::Missing { at } if at as usize == text.len()));
    assert_eq!(
        tokens
            .iter()
            .map(|t| &text[t.span.start..t.span.end])
            .collect::<String>(),
        text
    );
}
