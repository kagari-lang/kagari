//! Each type alternative exposes its own stored fields; HIR matches `TypeView`.

use crate::ast::{
    borrow::Ast,
    misc::{GenericArgList, Name, Path, TraitRef},
    ty::{AppliedType, ArrayType, FunctionType, GroupedType, QualifiedType, TupleType, TypeSyntax},
};

impl<'a> Ast<'a, AppliedType> {
    required_child!(path, Path, path);

    pub fn arguments(self) -> Ast<'a, GenericArgList> {
        self.with(&self.node.arguments)
    }
}

impl<'a> Ast<'a, GroupedType> {
    required_child!(ty, TypeSyntax, ty);
}

impl<'a> Ast<'a, TupleType> {
    pub fn element_types(self) -> impl Iterator<Item = Ast<'a, TypeSyntax>> {
        self.list(&self.node.elements)
    }
}

impl<'a> Ast<'a, ArrayType> {
    required_child!(element_type, TypeSyntax, element);
}

impl<'a> Ast<'a, FunctionType> {
    required_child!(result, TypeSyntax, result);

    pub fn params(self) -> impl Iterator<Item = Ast<'a, TypeSyntax>> {
        self.list(&self.node.parameters)
    }
}

impl<'a> Ast<'a, QualifiedType> {
    required_child!(receiver, TypeSyntax, receiver);
    required_child!(trait_ref, TraitRef, trait_ref);
    required_child!(member, Name, member);
    optional_child!(generic_args, GenericArgList, arguments);
}
