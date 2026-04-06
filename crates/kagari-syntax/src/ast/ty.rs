use crate::ast::{macros::ast_node, misc::Name, support, traits::AstNode};

ast_node!(TypeRef, TypeRef);
ast_node!(TupleType, TupleType);
ast_node!(ArrayType, ArrayType);

impl TypeRef {
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn tuple_type(&self) -> Option<TupleType> {
        support::child(self.syntax())
    }

    pub fn array_type(&self) -> Option<ArrayType> {
        support::child(self.syntax())
    }
}

impl TupleType {
    pub fn element_types(&self) -> impl Iterator<Item = TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast)
    }
}

impl ArrayType {
    pub fn element_type(&self) -> Option<TypeRef> {
        self.syntax().children().filter_map(TypeRef::cast).next()
    }
}
