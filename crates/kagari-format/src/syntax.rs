//! CST grammar rules produce documents; width and printing belong to layout.

use crate::{
    comments::{Attachment, Comments, is_comment},
    layout::Doc,
};
use kagari_common::cancellation::{CancellationToken, Cancelled};
use kagari_syntax::{
    kind::SyntaxKind,
    syntax_node::{SyntaxNode, SyntaxToken},
};

struct Piece {
    doc: Doc,
    first: SyntaxToken,
    last: SyntaxToken,
    kind: SyntaxKind,
}

pub(crate) fn document(
    root: &SyntaxNode,
    comments: &Comments,
    cancel: &CancellationToken,
) -> Result<Doc, Cancelled> {
    Ok(node(root, comments, cancel)?.map_or(Doc::Concat(Vec::new()), |piece| piece.doc))
}

fn node(
    root: &SyntaxNode,
    comments: &Comments,
    cancel: &CancellationToken,
) -> Result<Option<Piece>, Cancelled> {
    cancel.check()?;
    let mut pieces = Vec::new();
    for element in root.children_with_tokens() {
        cancel.check()?;
        if let Some(child) = element.as_node() {
            if let Some(piece) = node(child, comments, cancel)? {
                pieces.push(piece);
            }
        } else if let Some(token) = element.as_token() {
            if matches!(token.kind(), SyntaxKind::Whitespace | SyntaxKind::Eof) {
                continue;
            }
            pieces.push(Piece {
                doc: Doc::text(token.text()),
                first: token.clone(),
                last: token.clone(),
                kind: token.kind(),
            });
        }
    }
    if pieces.is_empty() {
        return Ok(None);
    }
    let first = pieces.first().unwrap().first.clone();
    let last = pieces.last().unwrap().last.clone();
    let mut doc = sequence(&pieces, root.kind(), comments);
    if root.kind() == SyntaxKind::InterpolatedString {
        doc = doc.flat();
    }
    Ok(Some(Piece {
        first,
        last,
        kind: root.kind(),
        doc,
    }))
}

fn sequence(pieces: &[Piece], owner: SyntaxKind, comments: &Comments) -> Doc {
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
                .position(|piece| piece.kind == close)
                .map(|offset| offset + index + 1)
        {
            let inner = &pieces[index + 1..end];
            let block = current.kind == SyntaxKind::LBrace
                && !matches!(
                    owner,
                    SyntaxKind::UseTree | SyntaxKind::StructExpr | SyntaxKind::Pattern
                );
            let edge = || {
                if block {
                    Doc::hard()
                } else {
                    Doc::soft(if current.kind == SyntaxKind::LBrace {
                        " "
                    } else {
                        ""
                    })
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

fn closing(kind: SyntaxKind, owner: SyntaxKind) -> Option<SyntaxKind> {
    match kind {
        SyntaxKind::LParen => Some(SyntaxKind::RParen),
        SyntaxKind::LBracket => Some(SyntaxKind::RBracket),
        SyntaxKind::LBrace => Some(SyntaxKind::RBrace),
        SyntaxKind::Lt
            if matches!(
                owner,
                SyntaxKind::GenericArgList
                    | SyntaxKind::GenericParamList
                    | SyntaxKind::QualifiedType
            ) =>
        {
            Some(SyntaxKind::Gt)
        }
        _ => None,
    }
}

fn separator(left: &Piece, right: &Piece, owner: SyntaxKind, comments: &Comments) -> Doc {
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
    if owner == SyntaxKind::InterpolatedString
        || matches!(a, SyntaxKind::FormatOpen)
        || matches!(b, SyntaxKind::FormatClose)
    {
        return Doc::text("");
    }
    if right.kind == SyntaxKind::AttributeArgs {
        return Doc::text("");
    }
    if left.kind == SyntaxKind::Attribute {
        return Doc::hard();
    }
    if owner == SyntaxKind::SourceFile {
        return Doc::Concat(vec![Doc::hard(), Doc::hard()]);
    }
    if matches!(
        owner,
        SyntaxKind::BlockExpr
            | SyntaxKind::ModuleBlock
            | SyntaxKind::TraitDef
            | SyntaxKind::ImplBlock
    ) && (a == SyntaxKind::Semi || a == SyntaxKind::RBrace)
    {
        return Doc::hard();
    }
    if a == SyntaxKind::Comma {
        return if matches!(
            owner,
            SyntaxKind::FieldList | SyntaxKind::VariantList | SyntaxKind::MatchArmList
        ) {
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
    if matches!(
        owner,
        SyntaxKind::GenericArgList | SyntaxKind::GenericParamList | SyntaxKind::QualifiedType
    ) && (a == SyntaxKind::Lt || b == SyntaxKind::Gt)
    {
        return Doc::text("");
    }
    if right.kind == SyntaxKind::GenericArgList || right.kind == SyntaxKind::GenericParamList {
        return Doc::text("");
    }
    if b == SyntaxKind::LParen
        && matches!(
            owner,
            SyntaxKind::FnDef
                | SyntaxKind::MethodDef
                | SyntaxKind::FunctionType
                | SyntaxKind::CallExpr
                | SyntaxKind::Variant
                | SyntaxKind::Pattern
                | SyntaxKind::AttributeArgs
        )
    {
        return Doc::text("");
    }
    if b == SyntaxKind::LBracket && matches!(owner, SyntaxKind::IndexExpr | SyntaxKind::Attribute) {
        return Doc::text("");
    }
    if owner == SyntaxKind::PrefixExpr {
        return Doc::text("");
    }
    if owner == SyntaxKind::ClosureExpr && (a == SyntaxKind::Pipe || b == SyntaxKind::Pipe) {
        return if right.kind == SyntaxKind::Pipe
            || (left.kind == SyntaxKind::Pipe && right.kind == SyntaxKind::ClosureParamList)
        {
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
    if owner == SyntaxKind::BinaryExpr && operator(a) {
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
