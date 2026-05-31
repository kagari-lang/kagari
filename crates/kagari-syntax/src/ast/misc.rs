use crate::{
    ast::{macros::ast_node, support, traits::AstNode, ty::TypeRef},
    kind::SyntaxKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writeability {
    Val,
    Var,
}

ast_node!(Name, Name);
ast_node!(Path, Path);
ast_node!(ParamList, ParamList);
ast_node!(Param, Param);
ast_node!(FieldList, FieldList);
ast_node!(Field, Field);
ast_node!(VariantList, VariantList);
ast_node!(Variant, Variant);

impl Name {
    pub fn text(&self) -> Option<String> {
        self.syntax()
            .children_with_tokens()
            .find_map(|element| match element {
                rowan::NodeOrToken::Token(token)
                    if matches!(
                        token.kind(),
                        SyntaxKind::Ident
                            | SyntaxKind::CrateKw
                            | SyntaxKind::SelfKw
                            | SyntaxKind::SuperKw
                    ) =>
                {
                    Some(token.text().to_string())
                }
                _ => None,
            })
    }
}

impl Path {
    pub fn segments(&self) -> impl Iterator<Item = Name> {
        support::children(self.syntax())
    }

    pub fn text(&self) -> Option<String> {
        let segments = self
            .segments()
            .filter_map(|segment| segment.text())
            .collect::<Vec<_>>();
        (!segments.is_empty()).then(|| segments.join("::"))
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
    pub fn writeability(&self) -> Option<Writeability> {
        if support::token(self.syntax(), SyntaxKind::ValKw).is_some() {
            Some(Writeability::Val)
        } else if support::token(self.syntax(), SyntaxKind::VarKw).is_some() {
            Some(Writeability::Var)
        } else {
            None
        }
    }

    pub fn is_val(&self) -> bool {
        self.writeability() == Some(Writeability::Val)
    }

    pub fn is_var(&self) -> bool {
        self.writeability() == Some(Writeability::Var)
    }

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
