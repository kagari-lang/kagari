use crate::{
    ast::{
        expr::BlockExpr,
        macros::ast_node,
        misc::{FieldList, Name, ParamList, VariantList},
        support,
        traits::AstNode,
        ty::TypeRef,
    },
    kind::SyntaxKind,
    syntax_node::SyntaxNode,
};

ast_node!(SourceFile, SourceFile);
ast_node!(FnDef, FnDef);
ast_node!(StructDef, StructDef);
ast_node!(EnumDef, EnumDef);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    FnDef(FnDef),
    StructDef(StructDef),
    EnumDef(EnumDef),
}

impl AstNode for Item {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::FnDef | SyntaxKind::StructDef | SyntaxKind::EnumDef
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        match syntax.kind() {
            SyntaxKind::FnDef => FnDef::cast(syntax).map(Self::FnDef),
            SyntaxKind::StructDef => StructDef::cast(syntax).map(Self::StructDef),
            SyntaxKind::EnumDef => EnumDef::cast(syntax).map(Self::EnumDef),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::FnDef(node) => node.syntax(),
            Self::StructDef(node) => node.syntax(),
            Self::EnumDef(node) => node.syntax(),
        }
    }
}

impl SourceFile {
    pub fn items(&self) -> impl Iterator<Item = Item> {
        support::children(self.syntax())
    }
}

impl FnDef {
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

impl StructDef {
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
