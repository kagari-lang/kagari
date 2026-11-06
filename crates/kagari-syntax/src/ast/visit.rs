//! Derived source-order traversal. No parent/children database is stored beside the AST.
//!
//! [`NodeRef`] walks physical syntax, including nested inline bodies. For a logical
//! module that excludes those bodies, use [`ModuleSyntax::visit_owned`](crate::ast::module::ModuleSyntax::visit_owned).
//! Walks include their starting node and do not poll a cancellation token.

mod fields;

use std::{iter, sync::Arc};

use kagari_common::span::Span;
use kagari_source::source::SourceFile;

use crate::{
    ast::{
        borrow::{Ast, AstNode, NodeData},
        expr, item, misc,
        site::{RecoveryPart, Required, SeparatedList, SyntaxSite, Unexpected},
        stmt,
        tokens::{OperatorKind, OperatorToken, Token},
        ty,
        views::ExprView,
    },
    kind::SyntaxKind,
    parser::ParsedFile,
    token::TokenKind,
};

/// A real lexical record resolved through its immutable physical parse.
#[derive(Debug, Clone, Copy)]
pub struct LexicalToken<'a> {
    pub(crate) owner: &'a Arc<ParsedFile>,
    pub(crate) index: u32,
}

impl<'a> LexicalToken<'a> {
    pub fn kind(self) -> SyntaxKind {
        self.lexical_kind().to_syntax_kind()
    }

    pub fn lexical_kind(self) -> TokenKind {
        self.owner.lexical.kind(self.index as usize)
    }

    pub fn span(self) -> Span {
        self.owner.lexical.span(self.index as usize)
    }

    pub fn text(self) -> &'a str {
        let span = self.span();
        &self.owner.source.text()[span.start..span.end]
    }
}

/// A direct grammar element: child node, real token/operator, or missing-role anchor.
/// Missing roles consume no source bytes and are omitted from node-only traversal.
#[derive(Debug, Clone, Copy)]
pub enum ElementRef<'a> {
    Node(NodeRef<'a>),
    Token(LexicalToken<'a>),
    Operator(OperatorRef<'a>),
    /// Zero-width insertion position in physical-source UTF-8 bytes, not a token index.
    Missing {
        at: u32,
    },
}

impl ElementRef<'_> {
    fn coverage(self) -> Option<(u32, u32)> {
        match self {
            Self::Node(node) => {
                let site = node.site().resolve(&node.owner().recoveries);
                // Empty recovered children only anchor an insertion position.
                // They cannot claim the trivia before that position: it may
                // already belong to the enclosing unparsed suffix after a stop.
                (site.first < site.end).then_some((site.first, site.end))
            }
            Self::Token(token) => Some((token.index, token.index + 1)),
            Self::Operator(operator) => Some((operator.first, operator.end)),
            Self::Missing { .. } => None,
        }
    }
}

/// A parser-selected grammatical operator, retaining one or more real lexical pieces.
/// For `a >> b`, `kind()` is `Shr` while the physical tokens are two `Gt` records.
/// Generic closing angles remain separate tokens; lexical traversal does not infer
/// expression operators. [`Self::text`] borrows the complete operator spelling.
#[derive(Debug, Clone, Copy)]
pub struct OperatorRef<'a> {
    owner: &'a Arc<ParsedFile>,
    first: u32,
    end: u32,
    kind: SyntaxKind,
}

impl<'a> OperatorRef<'a> {
    pub fn kind(self) -> SyntaxKind {
        self.kind
    }

    pub fn first_token(self) -> LexicalToken<'a> {
        LexicalToken {
            owner: self.owner,
            index: self.first,
        }
    }

    pub fn last_token(self) -> LexicalToken<'a> {
        LexicalToken {
            owner: self.owner,
            index: self.end - 1,
        }
    }

    pub fn text(self) -> &'a str {
        let first = self.first_token().span().start;
        let end = self.last_token().span().end;
        &self.owner.source.text()[first..end]
    }
}

pub(crate) trait Fields {
    fn fields<'a>(&'a self, owner: &'a Arc<ParsedFile>, output: &mut dyn FnMut(ElementRef<'a>));
}

pub(crate) trait Walk {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>));
}

#[derive(Clone, Copy)]
pub(crate) struct FieldContext<'a> {
    pub owner: &'a Arc<ParsedFile>,
    records: &'a [Unexpected],
    field: u16,
}

impl<'a> FieldContext<'a> {
    pub fn new(owner: &'a Arc<ParsedFile>, site: SyntaxSite) -> Self {
        let records = site.records(&owner.recoveries);
        Self {
            owner,
            records,
            field: 0,
        }
    }

    pub fn field(self, field: u16) -> Self {
        Self { field, ..self }
    }

    pub fn clear(self) -> Self {
        Self {
            records: &[],
            ..self
        }
    }

    fn emit(self, record: &Unexpected, output: &mut dyn FnMut(ElementRef<'a>)) {
        for index in record.first..record.end {
            output(ElementRef::Token(LexicalToken {
                owner: self.owner,
                index,
            }));
        }
    }

    fn at(self, entry: usize, part: RecoveryPart, output: &mut dyn FnMut(ElementRef<'a>)) {
        for record in self.records {
            if record.field == self.field && record.entry as usize == entry && record.part == part {
                self.emit(record, output);
            }
        }
    }

    pub fn element(self, entry: usize, output: &mut dyn FnMut(ElementRef<'a>)) {
        self.at(entry, RecoveryPart::Element, output);
    }

    fn separator(self, entry: usize, output: &mut dyn FnMut(ElementRef<'a>)) {
        self.at(entry, RecoveryPart::Separator, output);
    }

    pub fn remaining(self, field_count: u16, output: &mut dyn FnMut(ElementRef<'a>)) {
        for record in self.records {
            if record.field >= field_count {
                self.emit(record, output);
            }
        }
    }
}

impl<T: Walk> Walk for Required<T> {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        match self {
            Self::Present(node) => node.walk(context, output),
            Self::Missing { at } => {
                context.element(0, output);
                output(ElementRef::Missing { at: *at });
            }
        }
    }
}

impl<T: Walk> Walk for Option<T> {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        if let Some(node) = self {
            node.walk(context, output);
        } else {
            context.element(0, output);
        }
    }
}

impl<T: Walk> Walk for Box<T> {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        self.as_ref().walk(context, output);
    }
}

impl Walk for item::Attributes {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        for (index, attribute) in self.as_slice().iter().enumerate() {
            context.element(index, output);
            attribute.walk(context.clear(), output);
            context.separator(index, output);
        }
        context.element(self.as_slice().len(), output);
    }
}

impl<T: Walk> Walk for Vec<T> {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        for (index, node) in self.iter().enumerate() {
            context.element(index, output);
            node.walk(context.clear(), output);
            context.separator(index, output);
        }
        context.element(self.len(), output);
    }
}

impl<T: Walk, S> Walk for SeparatedList<T, S> {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        for (index, entry) in self.entries.iter().enumerate() {
            context.element(index, output);
            entry.element.walk(context.clear(), output);
            context.separator(index, output);
            entry.separator.walk(context.clear(), output);
        }
        context.element(self.entries.len(), output);
    }
}

impl<K> Walk for Token<K> {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        context.element(0, output);
        output(ElementRef::Token(LexicalToken {
            owner: context.owner,
            index: self.index(),
        }));
    }
}

impl<O: OperatorKind> Walk for OperatorToken<O> {
    fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
        context.element(0, output);
        output(ElementRef::Operator(OperatorRef {
            owner: context.owner,
            first: self.first,
            end: self.end,
            kind: self.operation.syntax_kind(),
        }));
    }
}

macro_rules! grammar_kind {
    ($node:ident, ModuleBody) => {
        if $node.node.open.is_some() {
            SyntaxKind::ModuleBlock
        } else {
            SyntaxKind::SourceFile
        }
    };
    ($node:ident, TypeSyntax) => {
        match $node.node {
            ty::TypeSyntax::Named(_)
            | ty::TypeSyntax::Applied(_)
            | ty::TypeSyntax::Never(_)
            | ty::TypeSyntax::Grouped(_) => SyntaxKind::TypeRef,
            ty::TypeSyntax::Tuple(_) => SyntaxKind::TupleType,
            ty::TypeSyntax::Array(_) => SyntaxKind::ArrayType,
            ty::TypeSyntax::Function(_) => SyntaxKind::FunctionType,
            ty::TypeSyntax::Qualified(_) => SyntaxKind::QualifiedType,
            ty::TypeSyntax::Error(_) => SyntaxKind::Error,
        }
    };
    ($node:ident, $kind:ident) => {
        SyntaxKind::$kind
    };
}

macro_rules! node_refs {
    ($($variant:ident: $ty:ty => $kind:ident),+ $(,)?) => {
        /// A borrowed typed choice for generic consumers; children are derived from fields.
        #[derive(Debug, Clone, Copy)]
        pub enum NodeRef<'a> { $($variant(Ast<'a, $ty>),)+ }

        impl<'a> NodeRef<'a> {
            pub fn kind(self) -> SyntaxKind { match self { $(Self::$variant(node) => { let _ = node; grammar_kind!(node, $kind) },)+ } }

            pub(crate) fn site(self) -> SyntaxSite { match self { $(Self::$variant(node) => node.node.site().resolve(&node.owner.recoveries),)+ } }

            pub(crate) fn owner(self) -> &'a Arc<ParsedFile> { match self { $(Self::$variant(node) => node.owner,)+ } }

            fn fields(self, output: &mut dyn FnMut(ElementRef<'a>)) { match self { $(Self::$variant(node) => node.node.fields(node.owner, output),)+ } }
        }

        $(
            impl<'a> From<Ast<'a, $ty>> for NodeRef<'a> {
                fn from(node: Ast<'a, $ty>) -> Self { Self::$variant(node) }
            }

            impl Walk for $ty {
                fn walk<'a>(&'a self, context: FieldContext<'a>, output: &mut dyn FnMut(ElementRef<'a>)) {
                    context.element(0, output);
                    output(ElementRef::Node(NodeRef::$variant(Ast { owner: context.owner, node: self })));
                }
            }
        )+
    };
}

node_refs! {
    ModuleBody: item::ModuleBody => ModuleBody,
    ModuleDef: item::ModuleDef => ModuleDef,
    UseDecl: item::UseDecl => UseDecl,
    UseTree: item::UseTree => UseTree,
    UseTreeList: item::UseTreeList => UseTreeList,
    FnDef: item::FnDef => FnDef,
    ConstDef: item::ConstDef => ConstDef,
    StaticDef: item::StaticDef => StaticDef,
    StructDef: item::StructDef => StructDef,
    EnumDef: item::EnumDef => EnumDef,
    TraitDef: item::TraitDef => TraitDef,
    ImplBlock: item::ImplBlock => ImplBlock,
    MethodDef: item::MethodDef => MethodDef,
    TypeAlias: item::TypeAlias => TypeAliasDef,
    OpaqueTypeDef: item::OpaqueTypeDef => AssociatedType,
    AssociatedType: item::AssociatedType => AssociatedType,
    Attribute: item::Attribute => Attribute,
    AttributeArgs: item::AttributeArgs => AttributeArgs,
    AttributeArg: item::AttributeArg => AttributeArg,
    AttributeValue: item::AttributeValue => AttributeValue,
    AttributeList: item::AttributeList => AttributeArgList,
    Name: misc::Name => Name,
    Path: misc::Path => Path,
    GenericParamList: misc::GenericParamList => GenericParamList,
    GenericParam: misc::GenericParam => GenericParam,
    GenericArgList: misc::GenericArgList => GenericArgList,
    AssociatedTypeBinding: misc::AssociatedTypeBinding => AssociatedTypeBinding,
    WhereClause: misc::WhereClause => WhereClause,
    WherePredicate: misc::WherePredicate => WherePredicate,
    TraitBoundList: misc::TraitBoundList => TraitBoundList,
    TraitRef: misc::TraitRef => TraitRef,
    CallableTrait: misc::CallableTrait => TypeList,
    ParamList: misc::ParamList => ParamList,
    Param: misc::Param => Param,
    FieldList: misc::FieldList => FieldList,
    Field: misc::Field => Field,
    VariantList: misc::VariantList => VariantList,
    Variant: misc::Variant => Variant,
    TuplePayload: misc::TuplePayload => TypeList,
    Type: ty::TypeSyntax => TypeSyntax,
    QualifiedType: ty::QualifiedType => QualifiedType,
    BlockExpr: expr::BlockExpr => BlockExpr,
    PathExpr: expr::PathExpr => PathExpr,
    Literal: expr::Literal => Literal,
    ParenExpr: expr::ParenExpr => ParenExpr,
    PrefixExpr: expr::PrefixExpr => PrefixExpr,
    CastExpr: expr::CastExpr => CastExpr,
    PropagateExpr: expr::PropagateExpr => PropagateExpr,
    AwaitExpr: expr::AwaitExpr => AwaitExpr,
    BinaryExpr: expr::BinaryExpr => BinaryExpr,
    RangeExpr: expr::RangeExpr => RangeExpr,
    CallExpr: expr::CallExpr => CallExpr,
    FieldExpr: expr::FieldExpr => FieldExpr,
    IndexExpr: expr::IndexExpr => IndexExpr,
    IfExpr: expr::IfExpr => IfExpr,
    BindingCondition: expr::BindingCondition => BindingCondition,
    StructExpr: expr::StructExpr => StructExpr,
    FieldInitList: expr::FieldInitList => FieldInitList,
    FieldInit: expr::FieldInit => FieldInit,
    StructUpdateBase: expr::StructUpdateBase => StructUpdateBase,
    MatchExpr: expr::MatchExpr => MatchExpr,
    MatchArmList: expr::MatchArmList => MatchArmList,
    MatchArm: expr::MatchArm => MatchArm,
    LoopExpr: expr::LoopExpr => LoopExpr,
    AsyncBlockExpr: expr::AsyncBlockExpr => AsyncBlockExpr,
    ClosureExpr: expr::ClosureExpr => ClosureExpr,
    ClosureParamList: expr::ClosureParamList => ClosureParamList,
    ClosureParam: expr::ClosureParam => ClosureParam,
    TupleExpr: expr::TupleExpr => TupleExpr,
    ArrayExpr: expr::ArrayExpr => ArrayExpr,
    InterpolatedString: expr::InterpolatedString => InterpolatedString,
    Interpolation: expr::Interpolation => Interpolation,
    Pattern: expr::Pattern => Pattern,
    PatternField: expr::PatternField => PatternField,
    BindingStmt: stmt::BindingStmt => BindingStmt,
    ReturnStmt: stmt::ReturnStmt => ReturnStmt,
    ExprStmt: stmt::ExprStmt => ExprStmt,
    AssignStmt: stmt::AssignStmt => AssignStmt,
    WhileStmt: stmt::WhileStmt => WhileStmt,
    LoopStmt: stmt::LoopStmt => LoopStmt,
    ForStmt: stmt::ForStmt => ForStmt,
    BreakStmt: stmt::BreakStmt => BreakStmt,
    ContinueStmt: stmt::ContinueStmt => ContinueStmt,
    Error: SyntaxSite => Error,
}

impl<'a> NodeRef<'a> {
    /// Attributes attached to this exact declaration/member; no ancestor lookup is needed.
    pub fn attributes(self) -> impl Iterator<Item = Ast<'a, item::Attribute>> {
        let attributes = match self {
            Self::ModuleDef(node) => node.node.attributes.as_slice(),
            Self::UseDecl(node) => node.node.attributes.as_slice(),
            Self::FnDef(node) => node.node.attributes.as_slice(),
            Self::ConstDef(node) => node.node.attributes.as_slice(),
            Self::StaticDef(node) => node.node.attributes.as_slice(),
            Self::StructDef(node) => node.node.attributes.as_slice(),
            Self::EnumDef(node) => node.node.attributes.as_slice(),
            Self::TraitDef(node) => node.node.attributes.as_slice(),
            Self::ImplBlock(node) => node.node.attributes.as_slice(),
            Self::TypeAlias(node) => node.node.attributes.as_slice(),
            Self::OpaqueTypeDef(node) => node.node.attributes.as_slice(),
            Self::AssociatedType(node) => node.node.attributes.as_slice(),
            Self::MethodDef(node) => node.node.attributes.as_slice(),
            Self::Field(node) => node.node.attributes.as_slice(),
            Self::Variant(node) => node.node.attributes.as_slice(),
            _ => &[],
        };
        attributes.iter().map(move |node| Ast {
            owner: self.owner(),
            node,
        })
    }

    /// A declaration/member's own name, separate from names in its type or body.
    pub fn declaration_name(self) -> Option<Ast<'a, misc::Name>> {
        match self {
            Self::ModuleDef(node) => node.name(),
            Self::FnDef(node) => node.name(),
            Self::ConstDef(node) => node.name(),
            Self::StaticDef(node) => node.name(),
            Self::StructDef(node) => node.name(),
            Self::EnumDef(node) => node.name(),
            Self::TraitDef(node) => node.name(),
            Self::TypeAlias(node) => node.name(),
            Self::OpaqueTypeDef(node) => node.name(),
            Self::AssociatedType(node) => node.name(),
            Self::MethodDef(node) => node.name(),
            Self::Field(node) => node.name(),
            Self::Variant(node) => node.name(),
            _ => None,
        }
    }

    pub fn body_span(self) -> Span {
        self.owner().body_span(self.site())
    }

    pub fn full_span(self) -> Span {
        self.owner().full_span(self.site())
    }

    pub fn text(self) -> &'a str {
        let span = self.body_span();
        &self.owner().source.text()[span.start..span.end]
    }

    /// Collect direct grammar elements and intervening lexical trivia in authored order.
    /// Includes missing anchors and directly owned unexpected tokens; child nodes
    /// remain unexpanded. This allocates a vector, not a retained secondary tree.
    pub fn elements(self) -> Vec<ElementRef<'a>> {
        let mut grammar = Vec::new();
        self.fields(&mut |element| grammar.push(element));
        let mut result = Vec::with_capacity(grammar.len());
        let site = self.site();
        let mut cursor = if matches!(self, Self::ModuleBody(body) if body.node.open.is_none()) {
            0
        } else {
            site.first
        };
        let owner = self.owner();
        let trivia = |first: u32, end: u32, result: &mut Vec<ElementRef<'a>>| {
            for index in first..end {
                let kind = owner.lexical.kind(index as usize);
                if kind.is_trivia() || kind == TokenKind::Eof {
                    result.push(ElementRef::Token(LexicalToken { owner, index }));
                }
            }
        };
        for element in grammar {
            if let Some((first, end)) = element.coverage() {
                trivia(cursor, first, &mut result);
                cursor = cursor.max(end);
            }
            result.push(element);
        }
        trivia(cursor, site.end, &mut result);
        result
    }

    /// Direct child nodes in authored order, excluding tokens, operators and missing slots.
    /// Currently collects [`Self::elements`] before filtering; use named typed
    /// accessors when the desired grammar role is known.
    pub fn children(self) -> impl Iterator<Item = Self> {
        self.elements()
            .into_iter()
            .filter_map(|element| match element {
                ElementRef::Node(node) => Some(node),
                _ => None,
            })
    }

    pub fn first_token(self) -> Option<LexicalToken<'a>> {
        let site = self.site();
        (site.first < site.end).then(|| LexicalToken {
            owner: self.owner(),
            index: site.first,
        })
    }

    pub fn last_token(self) -> Option<LexicalToken<'a>> {
        let site = self.site();
        (site.first < site.end).then(|| LexicalToken {
            owner: self.owner(),
            index: site.end - 1,
        })
    }

    /// Real lexical coverage, without flattening or rediscovering grammatical children.
    /// Includes internal trivia and unexpected tokens; operators stay split into
    /// their physical pieces. Missing roles add no tokens. Reads the lexical table
    /// directly without collecting child nodes or re-lexing the source.
    pub fn tokens(self) -> impl ExactSizeIterator<Item = LexicalToken<'a>> {
        let site = self.site();
        let first = if matches!(self, Self::ModuleBody(body) if body.node.open.is_none()) {
            0
        } else {
            site.first
        };
        (first..site.end).map(move |index| LexicalToken {
            owner: self.owner(),
            index,
        })
    }

    /// Source-order preorder over this node and all physical descendants.
    /// Includes inline module bodies. The iterator uses a pending-node vector and
    /// collects each visited node's children; stopping iteration avoids later work.
    pub fn descendants(self) -> impl Iterator<Item = Self> {
        let mut pending = vec![self];
        iter::from_fn(move || {
            let node = pending.pop()?;
            let children = node.children().collect::<Vec<_>>();
            pending.extend(children.into_iter().rev());
            Some(node)
        })
    }

    /// Visits typed nodes without per-node child vectors. Returning false skips that subtree.
    /// Calls the callback on this node first, then recursively visits children in
    /// source order. False does not terminate other sibling subtrees. Tokens,
    /// operators and missing anchors are not callbacks; use [`Self::elements`] for them.
    pub fn visit(self, callback: &mut impl FnMut(Self) -> bool) {
        if !callback(self) {
            return;
        }
        self.fields(&mut |element| {
            if let ElementRef::Node(child) = element {
                child.visit(callback);
            }
        });
    }

    pub fn expression(self) -> Option<ExprView<'a>> {
        match self {
            Self::BlockExpr(node) => Some(ExprView::BlockExpr(node)),
            Self::PathExpr(node) => Some(ExprView::PathExpr(node)),
            Self::Literal(node) => Some(ExprView::Literal(node)),
            Self::ParenExpr(node) => Some(ExprView::ParenExpr(node)),
            Self::PrefixExpr(node) => Some(ExprView::PrefixExpr(node)),
            Self::CastExpr(node) => Some(ExprView::CastExpr(node)),
            Self::PropagateExpr(node) => Some(ExprView::PropagateExpr(node)),
            Self::AwaitExpr(node) => Some(ExprView::AwaitExpr(node)),
            Self::BinaryExpr(node) => Some(ExprView::BinaryExpr(node)),
            Self::RangeExpr(node) => Some(ExprView::RangeExpr(node)),
            Self::CallExpr(node) => Some(ExprView::CallExpr(node)),
            Self::FieldExpr(node) => Some(ExprView::FieldExpr(node)),
            Self::IndexExpr(node) => Some(ExprView::IndexExpr(node)),
            Self::IfExpr(node) => Some(ExprView::IfExpr(node)),
            Self::StructExpr(node) => Some(ExprView::StructExpr(node)),
            Self::MatchExpr(node) => Some(ExprView::MatchExpr(node)),
            Self::LoopExpr(node) => Some(ExprView::LoopExpr(node)),
            Self::ClosureExpr(node) => Some(ExprView::ClosureExpr(node)),
            Self::AsyncBlockExpr(node) => Some(ExprView::AsyncBlockExpr(node)),
            Self::TupleExpr(node) => Some(ExprView::TupleExpr(node)),
            Self::ArrayExpr(node) => Some(ExprView::ArrayExpr(node)),
            Self::InterpolatedString(node) => Some(ExprView::InterpolatedString(node)),
            _ => None,
        }
    }
}

impl AstNode for NodeRef<'_> {
    fn body_span(&self) -> Span {
        (*self).body_span()
    }

    fn full_span(&self) -> Span {
        (*self).full_span()
    }

    fn source(&self) -> &SourceFile {
        &self.owner().source
    }
}
