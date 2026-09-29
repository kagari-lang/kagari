use crate::package::PackageError;
use kagari_common::{SourceFile, Span, cancellation::CancellationToken};
use kagari_syntax::{
    Parse,
    ast::{AstNode, Attribute, Field, Item, MethodDef, Name, Variant},
    kind::SyntaxKind,
    syntax_node::SyntaxNode,
};
use std::collections::BTreeSet;

/// Written annotation only. HIR validates its semantic meaning and target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NativeMarkerKind {
    Intrinsic,
    Numeric,
    ParseRadix,
    BuiltinType,
    BuiltinEnum,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeMarker {
    pub kind: NativeMarkerKind,
    pub binding: String,
    pub span: Span,
}

/// Coordinates into the original syntax, not a second declaration/type language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationSite {
    pub kind: SyntaxKind,
    pub span: Span,
    pub name_span: Option<Span>,
    pub body_span: Option<Span>,
    pub documentation: String,
    pub written_signature: String,
    pub markers: Vec<NativeMarker>,
}

fn span(node: &SyntaxNode) -> Span {
    let range = node.text_range();
    Span::new(usize::from(range.start()), usize::from(range.end()))
}

fn documentation(node: &SyntaxNode, source: &str) -> String {
    if let Some(item) = Item::cast(node.clone()) {
        item.documentation(source)
    } else if let Some(method) = MethodDef::cast(node.clone()) {
        method.documentation(source)
    } else if let Some(variant) = Variant::cast(node.clone()) {
        variant.documentation(source)
    } else if let Some(field) = Field::cast(node.clone()) {
        field.documentation(source)
    } else {
        String::new()
    }
}

pub(crate) fn declarations(
    source: &SourceFile,
    parsed: &Parse,
    cancel: &CancellationToken,
) -> Result<Vec<DeclarationSite>, PackageError> {
    let mut sites = Vec::new();
    for node in parsed.syntax().syntax().descendants() {
        cancel.check()?;
        if !matches!(
            node.kind(),
            SyntaxKind::FnDef
                | SyntaxKind::MethodDef
                | SyntaxKind::TraitDef
                | SyntaxKind::ImplBlock
                | SyntaxKind::AssociatedType
                | SyntaxKind::EnumDef
                | SyntaxKind::StructDef
                | SyntaxKind::ConstDef
                | SyntaxKind::Variant
                | SyntaxKind::Field
        ) {
            continue;
        }
        let mut markers = Vec::new();
        let mut seen = BTreeSet::new();
        for attr in node.children().filter_map(Attribute::cast) {
            let kind = match attr.name_text().as_deref() {
                Some("intrinsic") => NativeMarkerKind::Intrinsic,
                Some("numeric") => NativeMarkerKind::Numeric,
                Some("parse_radix") => NativeMarkerKind::ParseRadix,
                Some("builtin_type") => NativeMarkerKind::BuiltinType,
                Some("builtin_enum") => NativeMarkerKind::BuiltinEnum,
                _ => continue,
            };
            let invalid = |message: &str| PackageError::Annotation {
                uri: source.name().into(),
                span: span(attr.syntax()),
                message: message.into(),
            };
            if !seen.insert(kind) {
                return Err(invalid("duplicate native annotation"));
            }
            let args = attr
                .args()
                .map(|args| args.arguments().collect::<Vec<_>>())
                .unwrap_or_default();
            let [arg] = args.as_slice() else {
                return Err(invalid("native annotation requires one binding path"));
            };
            let binding = arg
                .value()
                .and_then(|value| value.path())
                .and_then(|path| path.text())
                .filter(|_| arg.name_text().is_none())
                .ok_or_else(|| invalid("native annotation requires an unnamed binding path"))?;
            markers.push(NativeMarker {
                kind,
                binding,
                span: span(attr.syntax()),
            });
        }
        if markers
            .iter()
            .filter(|marker| {
                matches!(
                    marker.kind,
                    NativeMarkerKind::Intrinsic
                        | NativeMarkerKind::Numeric
                        | NativeMarkerKind::ParseRadix
                )
            })
            .count()
            > 1
        {
            return Err(PackageError::Annotation {
                uri: source.name().into(),
                span: span(&node),
                message: "a declaration cannot specify multiple native bindings".into(),
            });
        }
        sites.push(DeclarationSite {
            documentation: documentation(&node, source.text()),
            written_signature: node.text().to_string(),
            kind: node.kind(),
            span: span(&node),
            name_span: node
                .children()
                .find_map(Name::cast)
                .map(|name| span(name.syntax())),
            body_span: node
                .children()
                .find(|child| child.kind() == SyntaxKind::BlockExpr)
                .map(|body| span(&body)),
            markers,
        });
    }
    Ok(sites)
}
