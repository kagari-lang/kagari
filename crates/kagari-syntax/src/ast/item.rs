use crate::{
    ast::{
        expr::{BlockExpr, Expr},
        macros::ast_node,
        misc::{FieldList, Name, ParamList, VariantList},
        stmt::Stmt,
        support,
        traits::AstNode,
        ty::TypeRef,
    },
    kind::SyntaxKind,
    syntax_node::SyntaxNode,
};

ast_node!(SourceFile, SourceFile);
ast_node!(FnDef, FnDef);
ast_node!(ConstDef, ConstDef);
ast_node!(StaticDef, StaticDef);
ast_node!(StructDef, StructDef);
ast_node!(EnumDef, EnumDef);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    FnDef(FnDef),
    ConstDef(ConstDef),
    StaticDef(StaticDef),
    StructDef(StructDef),
    EnumDef(EnumDef),
}

impl AstNode for Item {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::FnDef
                | SyntaxKind::ConstDef
                | SyntaxKind::StaticDef
                | SyntaxKind::StructDef
                | SyntaxKind::EnumDef
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        match syntax.kind() {
            SyntaxKind::FnDef => FnDef::cast(syntax).map(Self::FnDef),
            SyntaxKind::ConstDef => ConstDef::cast(syntax).map(Self::ConstDef),
            SyntaxKind::StaticDef => StaticDef::cast(syntax).map(Self::StaticDef),
            SyntaxKind::StructDef => StructDef::cast(syntax).map(Self::StructDef),
            SyntaxKind::EnumDef => EnumDef::cast(syntax).map(Self::EnumDef),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::FnDef(node) => node.syntax(),
            Self::ConstDef(node) => node.syntax(),
            Self::StaticDef(node) => node.syntax(),
            Self::StructDef(node) => node.syntax(),
            Self::EnumDef(node) => node.syntax(),
        }
    }
}

impl SourceFile {
    pub fn items(&self) -> impl Iterator<Item = Item> {
        support::children(self.syntax())
    }

    pub fn statements(&self) -> impl Iterator<Item = Stmt> {
        support::children(self.syntax())
    }

    pub fn tail_expr(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).last()
    }
}

impl FnDef {
    pub fn is_pub(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::PubKw).is_some()
    }

    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn param_list(&self) -> Option<ParamList> {
        support::child(self.syntax())
    }

    pub fn return_type(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    pub fn body(&self) -> Option<BlockExpr> {
        support::child(self.syntax())
    }
}

impl ConstDef {
    pub fn is_pub(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::PubKw).is_some()
    }

    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn ty(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    pub fn initializer(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }
}

impl StaticDef {
    pub fn is_pub(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::PubKw).is_some()
    }

    pub fn is_mut(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::MutKw).is_some()
    }

    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn ty(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }

    pub fn initializer(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }
}

impl StructDef {
    pub fn is_pub(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::PubKw).is_some()
    }

    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn field_list(&self) -> Option<FieldList> {
        support::child(self.syntax())
    }
}

impl EnumDef {
    pub fn is_pub(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::PubKw).is_some()
    }

    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn variant_list(&self) -> Option<VariantList> {
        support::child(self.syntax())
    }
}
