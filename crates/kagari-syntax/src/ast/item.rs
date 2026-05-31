use crate::{
    ast::{
        expr::{BlockExpr, Expr},
        macros::ast_node,
        misc::{FieldList, Name, ParamList, Path, VariantList},
        stmt::Stmt,
        support,
        traits::AstNode,
        ty::TypeRef,
    },
    kind::SyntaxKind,
    syntax_node::SyntaxNode,
};

ast_node!(SourceFile, SourceFile);
ast_node!(ModuleDef, ModuleDef);
ast_node!(ModuleBlock, ModuleBlock);
ast_node!(UseDecl, UseDecl);
ast_node!(UseTree, UseTree);
ast_node!(UseTreeList, UseTreeList);
ast_node!(FnDef, FnDef);
ast_node!(ConstDef, ConstDef);
ast_node!(StructDef, StructDef);
ast_node!(EnumDef, EnumDef);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    ModuleDef(ModuleDef),
    UseDecl(UseDecl),
    FnDef(FnDef),
    ConstDef(ConstDef),
    StructDef(StructDef),
    EnumDef(EnumDef),
}

impl AstNode for Item {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::ModuleDef
                | SyntaxKind::UseDecl
                | SyntaxKind::FnDef
                | SyntaxKind::ConstDef
                | SyntaxKind::StructDef
                | SyntaxKind::EnumDef
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        match syntax.kind() {
            SyntaxKind::ModuleDef => ModuleDef::cast(syntax).map(Self::ModuleDef),
            SyntaxKind::UseDecl => UseDecl::cast(syntax).map(Self::UseDecl),
            SyntaxKind::FnDef => FnDef::cast(syntax).map(Self::FnDef),
            SyntaxKind::ConstDef => ConstDef::cast(syntax).map(Self::ConstDef),
            SyntaxKind::StructDef => StructDef::cast(syntax).map(Self::StructDef),
            SyntaxKind::EnumDef => EnumDef::cast(syntax).map(Self::EnumDef),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::ModuleDef(node) => node.syntax(),
            Self::UseDecl(node) => node.syntax(),
            Self::FnDef(node) => node.syntax(),
            Self::ConstDef(node) => node.syntax(),
            Self::StructDef(node) => node.syntax(),
            Self::EnumDef(node) => node.syntax(),
        }
    }
}

impl ModuleDef {
    pub fn is_pub(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::PubKw).is_some()
    }

    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn block(&self) -> Option<ModuleBlock> {
        support::child(self.syntax())
    }
}

impl ModuleBlock {
    pub fn items(&self) -> impl Iterator<Item = Item> {
        support::children(self.syntax())
    }
}

impl UseDecl {
    pub fn is_pub(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::PubKw).is_some()
    }

    pub fn tree(&self) -> Option<UseTree> {
        support::child(self.syntax())
    }
}

impl UseTree {
    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }

    pub fn alias(&self) -> Option<Name> {
        let mut after_as = false;
        self.syntax()
            .children_with_tokens()
            .find_map(|element| match element {
                rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::AsKw => {
                    after_as = true;
                    None
                }
                rowan::NodeOrToken::Node(node) if after_as => Name::cast(node),
                _ => None,
            })
    }

    pub fn nested_trees(&self) -> impl Iterator<Item = UseTree> {
        self.syntax()
            .children()
            .filter_map(UseTreeList::cast)
            .flat_map(|list| list.trees().collect::<Vec<_>>())
    }
}

impl UseTreeList {
    pub fn trees(&self) -> impl Iterator<Item = UseTree> {
        support::children(self.syntax())
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
