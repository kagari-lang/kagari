use crate::{
    ast::{macros::ast_node, support, traits::AstNode, ty::TypeRef},
    kind::SyntaxKind,
};

ast_node!(Name, Name);
ast_node!(ParamList, ParamList);
ast_node!(Param, Param);
ast_node!(FieldList, FieldList);
ast_node!(Field, Field);
ast_node!(VariantList, VariantList);
ast_node!(Variant, Variant);

impl Name {
    pub fn text(&self) -> Option<String> {
        support::token(self.syntax(), SyntaxKind::Ident).map(|token| token.text().to_string())
    }
}

impl ParamList {
    pub fn params(&self) -> impl Iterator<Item = Param> {
        support::children(self.syntax())
    }
}

impl Param {
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }
}

impl FieldList {
    pub fn fields(&self) -> impl Iterator<Item = Field> {
        support::children(self.syntax())
    }
}

impl Field {
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }
}

impl VariantList {
    pub fn variants(&self) -> impl Iterator<Item = Variant> {
        support::children(self.syntax())
    }
}

impl Variant {
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }
}
