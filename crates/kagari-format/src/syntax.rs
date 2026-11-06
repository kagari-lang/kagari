//! Typed grammar visitors produce layout documents; width belongs to the printer.

use crate::{
    comments::{Attachment, Comments, is_comment},
    layout::Doc,
};
use kagari_common::cancellation::{CancellationToken, Cancelled};
use kagari_syntax::{
    ast::{
        views::TypeView,
        visit::{ElementRef, LexicalToken, NodeRef},
    },
    kind::SyntaxKind,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Ordinary,
    Source,
    Block,
    MemberList,
    UseGroup,
    StructFields,
    Pattern,
    Generic,
    Qualified,
    Callable,
    Index,
    Attribute,
    AttributeArgs,
    Prefix,
    Closure,
    ClosureParams,
    Binary,
}

impl Context {
    fn of(node: NodeRef<'_>) -> Self {
        match node {
            NodeRef::ModuleBody(body) if body.data().open.is_none() => Self::Source,
            NodeRef::ModuleBody(_)
            | NodeRef::BlockExpr(_)
            | NodeRef::TraitDef(_)
            | NodeRef::ImplBlock(_) => Self::Block,
            NodeRef::FieldList(_) | NodeRef::VariantList(_) | NodeRef::MatchArmList(_) => {
                Self::MemberList
            }
            NodeRef::UseTreeList(_) => Self::UseGroup,
            NodeRef::FieldInitList(_) => Self::StructFields,
            NodeRef::Pattern(_) => Self::Pattern,
            NodeRef::GenericArgList(_) | NodeRef::GenericParamList(_) => Self::Generic,
            NodeRef::QualifiedType(_) => Self::Qualified,
            NodeRef::Type(ty) => match ty.view() {
                TypeView::Qualified(_) => Self::Qualified,
                TypeView::Function(_) => Self::Callable,
                _ => Self::Ordinary,
            },
            NodeRef::FnDef(_)
            | NodeRef::MethodDef(_)
            | NodeRef::CallExpr(_)
            | NodeRef::Variant(_) => Self::Callable,
            NodeRef::IndexExpr(_) => Self::Index,
            NodeRef::Attribute(_) => Self::Attribute,
            NodeRef::AttributeArgs(_) => Self::AttributeArgs,
            NodeRef::PrefixExpr(_) => Self::Prefix,
            NodeRef::ClosureExpr(_) => Self::Closure,
            NodeRef::ClosureParamList(_) => Self::ClosureParams,
            NodeRef::BinaryExpr(_) => Self::Binary,
            _ => Self::Ordinary,
        }
    }

    fn angles(self) -> bool {
        matches!(self, Self::Generic | Self::Qualified)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PieceKind {
    Token(SyntaxKind),
    Node(Context),
    Attribute,
}

struct Piece<'a> {
    doc: Doc,
    first: LexicalToken<'a>,
    last: LexicalToken<'a>,
    kind: PieceKind,
}

pub(crate) fn document(
    root: NodeRef<'_>,
    comments: &Comments,
    cancel: &CancellationToken,
) -> Result<Doc, Cancelled> {
    Ok(node(root, comments, cancel)?.map_or(Doc::Concat(Vec::new()), |piece| piece.doc))
}

fn node<'a>(
    root: NodeRef<'a>,
    comments: &Comments,
    cancel: &CancellationToken,
) -> Result<Option<Piece<'a>>, Cancelled> {
    cancel.check()?;
    let context = Context::of(root);
    // Complete literal spelling bypasses discretionary layout, including hole boundaries.
    if let NodeRef::InterpolatedString(string) = root {
        return Ok(Some(Piece {
            doc: Doc::text(string.source_text()),
            first: root.first_token().expect("string opening delimiter"),
            last: root.last_token().expect("string closing delimiter"),
            kind: PieceKind::Node(context),
        }));
    }
    let mut pieces = Vec::new();
    for element in root.elements() {
        cancel.check()?;
        match element {
            ElementRef::Node(child) => {
                if let Some(piece) = node(child, comments, cancel)? {
                    pieces.push(piece);
                }
            }
            ElementRef::Token(token) => {
                if matches!(token.kind(), SyntaxKind::Whitespace | SyntaxKind::Eof) {
                    continue;
                }
                pieces.push(Piece {
                    doc: Doc::text(token.text()),
                    first: token,
                    last: token,
                    kind: PieceKind::Token(token.kind()),
                });
            }
            ElementRef::Operator(operator) => pieces.push(Piece {
                doc: Doc::text(operator.text()),
                first: operator.first_token(),
                last: operator.last_token(),
                kind: PieceKind::Token(operator.kind()),
            }),
            ElementRef::Missing { .. } => unreachable!("formatter rejects malformed source"),
        }
    }
    if pieces.is_empty() {
        return Ok(None);
    }
    let first = pieces.first().expect("nonempty pieces").first;
    let last = pieces.last().expect("nonempty pieces").last;
    let doc = sequence(&pieces, context, comments);
    let kind = if matches!(root, NodeRef::Attribute(_)) {
        PieceKind::Attribute
    } else {
        PieceKind::Node(context)
    };
    Ok(Some(Piece {
        first,
        last,
        kind,
        doc,
    }))
}

fn sequence(pieces: &[Piece<'_>], owner: Context, comments: &Comments) -> Doc {
    let mut docs = Vec::new();
    let mut index = 0;
    while index < pieces.len() {
        if index > 0 {
            docs.push(separator(
                &pieces[index - 1],
                &pieces[index],
                owner,
                comments,
            ));
        }
        let current = &pieces[index];
        if let Some(close) = closing(current.kind, owner)
            && let Some(end) = pieces[index + 1..]
                .iter()
                .position(|piece| piece.kind == PieceKind::Token(close))
                .map(|offset| offset + index + 1)
        {
            let inner = &pieces[index + 1..end];
            let brace = current.kind == PieceKind::Token(SyntaxKind::LBrace);
            let block = brace
                && !matches!(
                    owner,
                    Context::UseGroup | Context::StructFields | Context::Pattern
                );
            let edge = || {
                if block {
                    Doc::hard()
                } else {
                    Doc::soft(if brace { " " } else { "" })
                }
            };
            let inner_doc = sequence(inner, owner, comments);
            let mut group = vec![current.doc.clone()];
            if !inner.is_empty() {
                group.push(Doc::Concat(vec![edge(), inner_doc]).indent());
                group.push(
                    if inner
                        .last()
                        .is_some_and(|piece| piece.last.kind() == SyntaxKind::LineComment)
                    {
                        Doc::RequiredLine
                    } else {
                        edge()
                    },
                );
            }
            group.push(pieces[end].doc.clone());
            docs.push(Doc::Concat(group).group());
            index = end + 1;
            continue;
        }
        docs.push(current.doc.clone());
        index += 1;
    }
    Doc::Concat(docs).group()
}

fn closing(kind: PieceKind, owner: Context) -> Option<SyntaxKind> {
    match kind {
        PieceKind::Token(SyntaxKind::LParen) => Some(SyntaxKind::RParen),
        PieceKind::Token(SyntaxKind::LBracket) => Some(SyntaxKind::RBracket),
        PieceKind::Token(SyntaxKind::LBrace) => Some(SyntaxKind::RBrace),
        PieceKind::Token(SyntaxKind::Lt) if owner.angles() => Some(SyntaxKind::Gt),
        PieceKind::Token(SyntaxKind::Pipe) if owner == Context::ClosureParams => {
            Some(SyntaxKind::Pipe)
        }
        _ => None,
    }
}

fn separator(left: &Piece<'_>, right: &Piece<'_>, owner: Context, comments: &Comments) -> Doc {
    let a = left.last.kind();
    let b = right.first.kind();
    if a == SyntaxKind::LineComment {
        return Doc::RequiredLine;
    }
    if is_comment(b) {
        return match comments.attachment(&right.first) {
            Some(Attachment::Trailing) => Doc::text(" "),
            _ => Doc::hard(),
        };
    }
    if is_comment(a) {
        return if comments.attachment(&left.last) == Some(Attachment::Trailing)
            && a == SyntaxKind::BlockComment
        {
            Doc::text(" ")
        } else {
            Doc::hard()
        };
    }
    if right.kind == PieceKind::Node(Context::AttributeArgs) {
        return Doc::text("");
    }
    if left.kind == PieceKind::Attribute {
        return Doc::hard();
    }
    if owner == Context::Source {
        return Doc::Concat(vec![Doc::hard(), Doc::hard()]);
    }
    if owner == Context::Block && matches!(a, SyntaxKind::Semi | SyntaxKind::RBrace) {
        return Doc::hard();
    }
    if a == SyntaxKind::Comma {
        return if owner == Context::MemberList {
            Doc::hard()
        } else {
            Doc::soft(" ")
        };
    }
    if matches!(
        b,
        SyntaxKind::Comma
            | SyntaxKind::Semi
            | SyntaxKind::Colon
            | SyntaxKind::RParen
            | SyntaxKind::RBracket
            | SyntaxKind::Question
            | SyntaxKind::Dot
            | SyntaxKind::ColonColon
    ) || matches!(
        a,
        SyntaxKind::Dot | SyntaxKind::ColonColon | SyntaxKind::Hash
    ) {
        return Doc::text("");
    }
    if matches!(a, SyntaxKind::LParen | SyntaxKind::LBracket) {
        return Doc::text("");
    }
    if owner.angles() && (a == SyntaxKind::Lt || b == SyntaxKind::Gt) {
        return Doc::text("");
    }
    if right.kind == PieceKind::Node(Context::Generic) {
        return Doc::text("");
    }
    if b == SyntaxKind::LParen
        && matches!(
            owner,
            Context::Callable | Context::Pattern | Context::AttributeArgs
        )
    {
        return Doc::text("");
    }
    if b == SyntaxKind::LBracket && matches!(owner, Context::Index | Context::Attribute) {
        return Doc::text("");
    }
    if owner == Context::Prefix {
        return Doc::text("");
    }
    if owner == Context::Closure && (a == SyntaxKind::Pipe || b == SyntaxKind::Pipe) {
        return if right.kind == PieceKind::Token(SyntaxKind::Pipe) {
            Doc::text("")
        } else {
            Doc::text(" ")
        };
    }
    if matches!(a, SyntaxKind::DotDot | SyntaxKind::DotDotEq)
        || matches!(b, SyntaxKind::DotDot | SyntaxKind::DotDotEq)
    {
        return Doc::text("");
    }
    if owner == Context::Binary && operator(a) {
        return Doc::soft(" ").indent();
    }
    Doc::text(" ")
}

fn operator(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Plus
            | SyntaxKind::Minus
            | SyntaxKind::Star
            | SyntaxKind::Slash
            | SyntaxKind::Percent
            | SyntaxKind::EqEq
            | SyntaxKind::NotEq
            | SyntaxKind::IdentityEq
            | SyntaxKind::IdentityNotEq
            | SyntaxKind::Lt
            | SyntaxKind::Gt
            | SyntaxKind::Le
            | SyntaxKind::Ge
            | SyntaxKind::Shl
            | SyntaxKind::Shr
            | SyntaxKind::Amp
            | SyntaxKind::Pipe
            | SyntaxKind::Caret
            | SyntaxKind::AmpAmp
            | SyntaxKind::PipePipe
    )
}
