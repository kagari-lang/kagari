use crate::{
    ast::{
        macros::ast_node,
        misc::{Name, Path},
        stmt::Stmt,
        support,
        traits::AstNode,
    },
    kind::SyntaxKind,
    syntax_node::SyntaxNode,
};

ast_node!(BlockExpr, BlockExpr);
ast_node!(PathExpr, PathExpr);
ast_node!(Literal, Literal);
ast_node!(ParenExpr, ParenExpr);
ast_node!(PrefixExpr, PrefixExpr);
ast_node!(BinaryExpr, BinaryExpr);
ast_node!(RangeExpr, RangeExpr);
ast_node!(CallExpr, CallExpr);
ast_node!(FieldExpr, FieldExpr);
ast_node!(IndexExpr, IndexExpr);
ast_node!(IfExpr, IfExpr);
ast_node!(StructExpr, StructExpr);
ast_node!(FieldInitList, FieldInitList);
ast_node!(FieldInit, FieldInit);
ast_node!(MatchExpr, MatchExpr);
ast_node!(LoopExpr, LoopExpr);
ast_node!(ClosureExpr, ClosureExpr);
ast_node!(ClosureParamList, ClosureParamList);
ast_node!(ClosureParam, ClosureParam);
ast_node!(MatchArmList, MatchArmList);
ast_node!(MatchArm, MatchArm);
ast_node!(Pattern, Pattern);
ast_node!(PatternField, PatternField);
ast_node!(TupleExpr, TupleExpr);
ast_node!(ArrayExpr, ArrayExpr);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    BlockExpr(BlockExpr),
    PathExpr(PathExpr),
    Literal(Literal),
    ParenExpr(ParenExpr),
    PrefixExpr(PrefixExpr),
    BinaryExpr(BinaryExpr),
    RangeExpr(RangeExpr),
    CallExpr(CallExpr),
    FieldExpr(FieldExpr),
    IndexExpr(IndexExpr),
    IfExpr(IfExpr),
    StructExpr(StructExpr),
    MatchExpr(MatchExpr),
    LoopExpr(LoopExpr),
    ClosureExpr(ClosureExpr),
    TupleExpr(TupleExpr),
    ArrayExpr(ArrayExpr),
}

impl AstNode for Expr {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::BlockExpr
                | SyntaxKind::PathExpr
                | SyntaxKind::Literal
                | SyntaxKind::ParenExpr
                | SyntaxKind::PrefixExpr
                | SyntaxKind::BinaryExpr
                | SyntaxKind::RangeExpr
                | SyntaxKind::CallExpr
                | SyntaxKind::FieldExpr
                | SyntaxKind::IndexExpr
                | SyntaxKind::IfExpr
                | SyntaxKind::StructExpr
                | SyntaxKind::MatchExpr
                | SyntaxKind::LoopExpr
                | SyntaxKind::ClosureExpr
                | SyntaxKind::TupleExpr
                | SyntaxKind::ArrayExpr
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        match syntax.kind() {
            SyntaxKind::BlockExpr => BlockExpr::cast(syntax).map(Self::BlockExpr),
            SyntaxKind::PathExpr => PathExpr::cast(syntax).map(Self::PathExpr),
            SyntaxKind::Literal => Literal::cast(syntax).map(Self::Literal),
            SyntaxKind::ParenExpr => ParenExpr::cast(syntax).map(Self::ParenExpr),
            SyntaxKind::PrefixExpr => PrefixExpr::cast(syntax).map(Self::PrefixExpr),
            SyntaxKind::BinaryExpr => BinaryExpr::cast(syntax).map(Self::BinaryExpr),
            SyntaxKind::RangeExpr => RangeExpr::cast(syntax).map(Self::RangeExpr),
            SyntaxKind::CallExpr => CallExpr::cast(syntax).map(Self::CallExpr),
            SyntaxKind::FieldExpr => FieldExpr::cast(syntax).map(Self::FieldExpr),
            SyntaxKind::IndexExpr => IndexExpr::cast(syntax).map(Self::IndexExpr),
            SyntaxKind::IfExpr => IfExpr::cast(syntax).map(Self::IfExpr),
            SyntaxKind::StructExpr => StructExpr::cast(syntax).map(Self::StructExpr),
            SyntaxKind::MatchExpr => MatchExpr::cast(syntax).map(Self::MatchExpr),
            SyntaxKind::LoopExpr => LoopExpr::cast(syntax).map(Self::LoopExpr),
            SyntaxKind::ClosureExpr => ClosureExpr::cast(syntax).map(Self::ClosureExpr),
            SyntaxKind::TupleExpr => TupleExpr::cast(syntax).map(Self::TupleExpr),
            SyntaxKind::ArrayExpr => ArrayExpr::cast(syntax).map(Self::ArrayExpr),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::BlockExpr(node) => node.syntax(),
            Self::PathExpr(node) => node.syntax(),
            Self::Literal(node) => node.syntax(),
            Self::ParenExpr(node) => node.syntax(),
            Self::PrefixExpr(node) => node.syntax(),
            Self::BinaryExpr(node) => node.syntax(),
            Self::RangeExpr(node) => node.syntax(),
            Self::CallExpr(node) => node.syntax(),
            Self::FieldExpr(node) => node.syntax(),
            Self::IndexExpr(node) => node.syntax(),
            Self::IfExpr(node) => node.syntax(),
            Self::StructExpr(node) => node.syntax(),
            Self::MatchExpr(node) => node.syntax(),
            Self::LoopExpr(node) => node.syntax(),
            Self::ClosureExpr(node) => node.syntax(),
            Self::TupleExpr(node) => node.syntax(),
            Self::ArrayExpr(node) => node.syntax(),
        }
    }
}

impl BlockExpr {
    pub fn statements(&self) -> impl Iterator<Item = Stmt> {
        support::children(self.syntax())
    }

    pub fn tail_expr(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).last()
    }
}

impl PathExpr {
    pub fn generic_args(&self) -> Option<super::GenericArgList> {
        support::child(self.syntax())
    }
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        if self.generic_args().is_some() {
            return Some(format!(
                "{}::{}",
                self.path()?.text()?,
                self.name()?.text()?
            ));
        }
        self.path()
            .and_then(|path| path.text())
            .or_else(|| self.name().and_then(|name| name.text()))
    }

    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }
}

impl Literal {
    pub fn text(&self) -> Option<String> {
        self.syntax()
            .first_token()
            .map(|token| token.text().to_string())
    }

    pub fn kind(&self) -> Option<SyntaxKind> {
        self.syntax().first_token().map(|token| token.kind())
    }
}

impl ParenExpr {
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl PrefixExpr {
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }

    pub fn operator(&self) -> Option<SyntaxKind> {
        self.syntax()
            .children_with_tokens()
            .find_map(|element| match element {
                rowan::NodeOrToken::Token(token)
                    if matches!(token.kind(), SyntaxKind::Minus | SyntaxKind::Bang) =>
                {
                    Some(token.kind())
                }
                _ => None,
            })
    }
}

impl BinaryExpr {
    pub fn lhs(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    pub fn rhs(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).nth(1)
    }

    pub fn operator(&self) -> Option<SyntaxKind> {
        self.syntax()
            .children_with_tokens()
            .find_map(|element| match element {
                rowan::NodeOrToken::Token(token)
                    if matches!(
                        token.kind(),
                        SyntaxKind::Plus
                            | SyntaxKind::Minus
                            | SyntaxKind::Star
                            | SyntaxKind::Slash
                            | SyntaxKind::Percent
                            | SyntaxKind::EqEq
                            | SyntaxKind::NotEq
                            | SyntaxKind::Lt
                            | SyntaxKind::Gt
                            | SyntaxKind::Le
                            | SyntaxKind::Ge
                            | SyntaxKind::AmpAmp
                            | SyntaxKind::PipePipe
                    ) =>
                {
                    Some(token.kind())
                }
                _ => None,
            })
    }
}

impl RangeExpr {
    pub fn start(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    pub fn end(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).nth(1)
    }

    pub fn inclusive(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::DotDotEq).is_some()
    }
}

impl CallExpr {
    pub fn callee(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    pub fn args(&self) -> impl Iterator<Item = Expr> {
        self.syntax().children().filter_map(Expr::cast).skip(1)
    }
}

impl FieldExpr {
    pub fn receiver(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    pub fn name(&self) -> Option<Name> {
        self.syntax().children().filter_map(Name::cast).next()
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }
}

impl IndexExpr {
    pub fn receiver(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    pub fn index(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).nth(1)
    }
}

impl IfExpr {
    pub fn condition(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    pub fn then_branch(&self) -> Option<BlockExpr> {
        self.syntax().children().filter_map(BlockExpr::cast).next()
    }

    pub fn else_branch(&self) -> Option<Expr> {
        let mut blocks = self.syntax().children().filter_map(BlockExpr::cast);
        let then_branch = blocks.next()?;

        let mut seen_then_branch = false;
        self.syntax()
            .children()
            .filter_map(Expr::cast)
            .find(|expr| {
                if !seen_then_branch && expr.syntax() == then_branch.syntax() {
                    seen_then_branch = true;
                    return false;
                }

                seen_then_branch
            })
    }
}

impl StructExpr {
    pub fn generic_args(&self) -> Option<super::GenericArgList> {
        support::child(self.syntax())
    }
    pub fn path(&self) -> Option<PathExpr> {
        self.syntax().children().filter_map(PathExpr::cast).next()
    }

    pub fn field_list(&self) -> Option<FieldInitList> {
        self.syntax()
            .children()
            .filter_map(FieldInitList::cast)
            .next()
    }
}

impl FieldInitList {
    pub fn fields(&self) -> impl Iterator<Item = FieldInit> {
        self.syntax().children().filter_map(FieldInit::cast)
    }
}

impl FieldInit {
    pub fn name(&self) -> Option<Name> {
        self.syntax().children().filter_map(Name::cast).next()
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn value(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }
}

impl MatchExpr {
    pub fn scrutinee(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    pub fn arms(&self) -> Option<MatchArmList> {
        self.syntax()
            .children()
            .filter_map(MatchArmList::cast)
            .next()
    }
}

impl LoopExpr {
    pub fn body(&self) -> Option<BlockExpr> {
        support::child(self.syntax())
    }
}

impl ClosureExpr {
    pub fn params(&self) -> impl Iterator<Item = ClosureParam> {
        self.syntax()
            .children()
            .filter_map(ClosureParamList::cast)
            .flat_map(|list| {
                list.syntax()
                    .children()
                    .filter_map(ClosureParam::cast)
                    .collect::<Vec<_>>()
            })
    }

    pub fn body(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).last()
    }
}

impl ClosureParam {
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn ty(&self) -> Option<super::TypeRef> {
        support::child(self.syntax())
    }
}

impl MatchArmList {
    pub fn arms(&self) -> impl Iterator<Item = MatchArm> {
        self.syntax().children().filter_map(MatchArm::cast)
    }
}

impl MatchArm {
    pub fn pattern(&self) -> Option<Pattern> {
        self.syntax().children().filter_map(Pattern::cast).next()
    }

    pub fn expr(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).last()
    }

    pub fn guard(&self) -> Option<Expr> {
        if support::token(self.syntax(), SyntaxKind::IfKw).is_some() {
            self.syntax().children().filter_map(Expr::cast).next()
        } else {
            None
        }
    }
}

impl Pattern {
    pub fn elements(&self) -> impl Iterator<Item = Pattern> {
        self.syntax().children().filter_map(Pattern::cast)
    }

    pub fn is_wildcard(&self) -> bool {
        self.path().and_then(|path| path.name_text()).as_deref() == Some("_")
    }

    pub fn path(&self) -> Option<PathExpr> {
        self.syntax().children().filter_map(PathExpr::cast).next()
    }

    pub fn literal(&self) -> Option<Literal> {
        self.syntax().children().filter_map(Literal::cast).next()
    }

    pub fn is_struct(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::LBrace).is_some()
    }

    pub fn is_tuple_struct(&self) -> bool {
        self.path().is_some() && support::token(self.syntax(), SyntaxKind::LParen).is_some()
    }

    pub fn is_tuple(&self) -> bool {
        self.path().is_none() && support::token(self.syntax(), SyntaxKind::LParen).is_some()
    }

    pub fn is_grouped(&self) -> bool {
        self.is_tuple()
            && support::token(self.syntax(), SyntaxKind::Comma).is_none()
            && self.elements().count() == 1
    }

    pub fn fields(&self) -> impl Iterator<Item = PatternField> {
        self.syntax().children().filter_map(PatternField::cast)
    }
}

impl PatternField {
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn pattern(&self) -> Option<Pattern> {
        support::child(self.syntax())
    }
}

impl TupleExpr {
    pub fn elements(&self) -> impl Iterator<Item = Expr> {
        self.syntax().children().filter_map(Expr::cast)
    }
}

impl ArrayExpr {
    pub fn elements(&self) -> impl Iterator<Item = Expr> {
        self.syntax().children().filter_map(Expr::cast)
    }
}
