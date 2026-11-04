//! Borrowed source context. Ordinary nodes never own reference-counted handles.

use std::sync::Arc;

use kagari_common::span::Span;
use kagari_source::source::SourceFile;

use crate::{
    ast::{
        expr::{Expr, Literal, PathExpr, Pattern},
        item::{Attribute, AttributeValue, Item, Member},
        misc::{Name, Path},
        site::{RecoveryPosition, SeparatedList, SyntaxSite, Unexpected},
        stmt::Stmt,
        tokens::{OperatorToken, Token},
        ty::TypeSyntax,
    },
    parser::ParsedFile,
};

/// Source coverage of a stored grammar node, interpreted only by its owning parse.
pub trait NodeData {
    fn site(&self) -> SyntaxSite;

    fn field_name(&self, _field: u16) -> Option<&'static str> {
        None
    }
}

/// The declaration/member that syntactically owns each attached attribute.
pub trait AttributeOwner {
    fn attributes(&self) -> &[Attribute];
}

impl NodeData for SyntaxSite {
    fn site(&self) -> SyntaxSite {
        *self
    }
}

impl<K> NodeData for Token<K> {
    fn site(&self) -> SyntaxSite {
        SyntaxSite::token(self.index())
    }
}

impl<O> NodeData for OperatorToken<O> {
    fn site(&self) -> SyntaxSite {
        SyntaxSite {
            first: self.first,
            end: self.end,
        }
    }
}

impl NodeData for Name {
    fn site(&self) -> SyntaxSite {
        self.token.site()
    }
}

impl NodeData for Literal {
    fn site(&self) -> SyntaxSite {
        self.token.site()
    }
}

impl NodeData for Path {
    fn site(&self) -> SyntaxSite {
        match self {
            Self::Single(name) => name.site(),
            Self::Segmented(path) => path.site(),
        }
    }
}

impl NodeData for PathExpr {
    fn site(&self) -> SyntaxSite {
        match self {
            Self::Plain(path) => path.site(),
            Self::Applied(path) => path.site(),
            Self::Qualified(ty) => ty.site(),
        }
    }
}

macro_rules! enum_sites {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        impl NodeData for $name {
            fn site(&self) -> SyntaxSite {
                match self {
                    $(Self::$variant(node) => node.site(),)+
                    Self::Error(site) => *site,
                }
            }
        }
    };
}

enum_sites!(Expr {
    BlockExpr,
    PathExpr,
    Literal,
    ParenExpr,
    PrefixExpr,
    CastExpr,
    PropagateExpr,
    AwaitExpr,
    BinaryExpr,
    RangeExpr,
    CallExpr,
    FieldExpr,
    IndexExpr,
    IfExpr,
    StructExpr,
    MatchExpr,
    LoopExpr,
    ClosureExpr,
    AsyncBlockExpr,
    TupleExpr,
    ArrayExpr,
    InterpolatedString,
});

enum_sites!(TypeSyntax {
    Named,
    Applied,
    Never,
    Grouped,
    Tuple,
    Array,
    Function,
    Qualified
});
enum_sites!(Pattern {
    Wildcard,
    Path,
    Literal,
    Or,
    Range,
    Grouped,
    Tuple,
    TupleStruct,
    Struct
});
enum_sites!(Stmt {
    BindingStmt,
    ReturnStmt,
    ExprStmt,
    AssignStmt,
    WhileStmt,
    LoopStmt,
    ForStmt,
    BreakStmt,
    ContinueStmt,
    FnDef
});
enum_sites!(Item {
    ModuleDef,
    UseDecl,
    FnDef,
    ConstDef,
    StaticDef,
    StructDef,
    EnumDef,
    TraitDef,
    ImplBlock,
    TypeAlias,
    OpaqueTypeDef
});
enum_sites!(Member {
    MethodDef,
    AssociatedType,
    ConstDef
});
enum_sites!(AttributeValue {
    Literal,
    Path,
    List
});

/// An immutable node borrow paired with the parse that resolves its tokens/sites.
/// Cloning copies two borrows, without cloning nodes, source text or an `Arc`.
#[derive(Debug)]
pub struct Ast<'a, T: ?Sized> {
    pub(crate) owner: &'a Arc<ParsedFile>,
    pub(crate) node: &'a T,
}

impl<T: ?Sized> Copy for Ast<'_, T> {}

impl<T: ?Sized> Clone for Ast<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, T: ?Sized> Ast<'a, T> {
    /// Borrows explicit grammar fields; mutation and construction remain parser-owned.
    pub fn data(self) -> &'a T {
        self.node
    }

    pub(crate) fn with<U: ?Sized>(self, node: &'a U) -> Ast<'a, U> {
        Ast {
            owner: self.owner,
            node,
        }
    }

    pub(crate) fn list<U, S>(
        self,
        list: &'a SeparatedList<U, S>,
    ) -> impl Iterator<Item = Ast<'a, U>> {
        list.entries
            .iter()
            .filter_map(move |entry| entry.element.as_ref().map(|node| self.with(node)))
    }

    pub fn source(self) -> &'a SourceFile {
        &self.owner.source
    }
}

impl<'a, T: AttributeOwner> Ast<'a, T> {
    pub fn attributes(self) -> impl Iterator<Item = Ast<'a, Attribute>> {
        self.node
            .attributes()
            .iter()
            .map(move |attribute| self.with(attribute))
    }
}

impl<'a, T: NodeData + ?Sized> Ast<'a, T> {
    pub fn body_span(self) -> Span {
        self.owner.body_span(self.node.site())
    }

    pub fn full_span(self) -> Span {
        self.owner.full_span(self.node.site())
    }

    pub fn source_text(self) -> &'a str {
        let span = self.body_span();
        &self.owner.source.text()[span.start..span.end]
    }

    pub fn unexpected(self) -> impl Iterator<Item = UnexpectedContent<'a>> {
        let records = self.node.site().records(&self.owner.recoveries);
        records.iter().map(move |record| UnexpectedContent {
            owner: self.owner,
            record,
            position: RecoveryPosition {
                field: if record.field == u16::MAX {
                    "unparsed_suffix"
                } else {
                    self.node.field_name(record.field).unwrap_or("input")
                },
                entry: record.entry as usize,
                part: record.part,
            },
        })
    }
}

/// Borrowed skipped input with parser-assigned ownership and insertion position.
#[derive(Debug, Clone, Copy)]
pub struct UnexpectedContent<'a> {
    owner: &'a Arc<ParsedFile>,
    record: &'a Unexpected,
    position: RecoveryPosition,
}

impl<'a> UnexpectedContent<'a> {
    pub fn position(self) -> RecoveryPosition {
        self.position
    }

    pub fn span(self) -> Span {
        self.owner.body_span(SyntaxSite {
            first: self.record.first,
            end: self.record.end,
        })
    }

    pub fn text(self) -> &'a str {
        let span = self.span();
        &self.owner.source.text()[span.start..span.end]
    }
}

/// Shared source-site access for borrowed typed alternatives and concrete nodes.
pub trait AstNode {
    fn body_span(&self) -> Span;

    fn full_span(&self) -> Span;

    fn source(&self) -> &SourceFile;

    fn source_text(&self) -> &str {
        let span = self.body_span();
        &self.source().text()[span.start..span.end]
    }

    fn documentation(&self) -> String {
        let start = self.body_span().start;
        let prefix = &self.source().text()[..start];
        let mut lines = prefix.lines().rev();
        let mut docs = Vec::new();
        if let Some(last) = lines.next()
            && !last.trim().is_empty()
        {
            if let Some(doc) = last.trim_start().strip_prefix("///") {
                docs.push(doc.strip_prefix(' ').unwrap_or(doc));
            } else {
                return String::new();
            }
        }
        for line in lines {
            let Some(doc) = line.trim_start().strip_prefix("///") else {
                break;
            };
            docs.push(doc.strip_prefix(' ').unwrap_or(doc));
        }
        docs.reverse();
        docs.join("\n")
    }
}

impl<T: NodeData + ?Sized> AstNode for Ast<'_, T> {
    fn body_span(&self) -> Span {
        (*self).body_span()
    }

    fn full_span(&self) -> Span {
        (*self).full_span()
    }

    fn source(&self) -> &SourceFile {
        (*self).source()
    }
}
