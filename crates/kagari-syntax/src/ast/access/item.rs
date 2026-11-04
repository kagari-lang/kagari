//! Declaration/member access retains the known attribute owner and signature fields.

use crate::ast::{
    access::misc::visibility_kind,
    borrow::{Ast, AttributeOwner},
    expr::{BlockExpr, Literal},
    item::{
        AliasTarget, AssociatedType, Attribute, AttributeArg, AttributeArgs, AttributePayload,
        AttributeValue, ConstDef, ConstInitializer, EnumDef, FnDef, FunctionBody, ImplBlock,
        Member, MethodDef, ModuleDef, OpaqueTypeDef, StaticDef, StructDef, TraitDef, TypeAlias,
        UseDecl, UseSuffix, UseTree, UseTreeList, VisibilityKind,
    },
    misc::{
        Field, FieldList, GenericParamList, Name, ParamList, Path, TraitBoundList, TraitRef,
        Variant, VariantList, WhereClause, Writeability,
    },
    ty::TypeSyntax,
    views::{ExprView, ItemView},
};

macro_rules! declaration_fields {
    ($($ty:ty),+ $(,)?) => { $(
        impl AttributeOwner for $ty {
            fn attributes(&self) -> &[Attribute] { self.attributes.as_slice() }
        }
    )+ };
}

declaration_fields!(
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
    OpaqueTypeDef,
    MethodDef,
    AssociatedType,
    Field,
    Variant
);

macro_rules! named_visibility {
    ($($ty:ty),+ $(,)?) => { $(
        impl<'a> Ast<'a, $ty> {
            required_child!(name, Name, name);

            pub fn name_text(self) -> Option<String> { self.name()?.text() }

            pub fn visibility(self) -> VisibilityKind { visibility_kind(self.with(&self.node.visibility)) }
        }
    )+ };
}

named_visibility!(
    ModuleDef,
    FnDef,
    ConstDef,
    StaticDef,
    StructDef,
    EnumDef,
    TraitDef,
    TypeAlias,
    OpaqueTypeDef,
    MethodDef,
    AssociatedType
);

macro_rules! generics {
    ($($ty:ty),+ $(,)?) => { $(
        impl<'a> Ast<'a, $ty> {
            optional_child!(generic_params, GenericParamList, generics);
        }
    )+ };
}

generics!(
    FnDef,
    StructDef,
    EnumDef,
    TraitDef,
    ImplBlock,
    TypeAlias,
    OpaqueTypeDef,
    MethodDef,
    AssociatedType
);

macro_rules! where_clauses {
    ($($ty:ty),+ $(,)?) => { $(
        impl<'a> Ast<'a, $ty> {
            optional_child!(where_clause, WhereClause, where_clause);
        }
    )+ };
}

where_clauses!(FnDef, ImplBlock, OpaqueTypeDef, MethodDef, AssociatedType);

macro_rules! functions {
    ($($ty:ty),+ $(,)?) => { $(
        impl<'a> Ast<'a, $ty> {
            pub fn is_async(self) -> bool { self.node.async_keyword.is_some() }

            pub fn param_list(self) -> Ast<'a, ParamList> { self.with(&self.node.parameters) }

            pub fn return_type(self) -> Option<Ast<'a, TypeSyntax>> { self.node.result.as_ref()?.ty.as_ref().map(|ty| self.with(ty)) }

            pub fn body(self) -> Option<Ast<'a, BlockExpr>> {
                match &self.node.body { FunctionBody::Block(body) => body.as_ref().map(|body| self.with::<BlockExpr>(body)), FunctionBody::Declaration(_) => None }
            }
        }
    )+ };
}

functions!(FnDef, MethodDef);

impl<'a> Ast<'a, UseDecl> {
    required_child!(tree, UseTree, tree);

    pub fn visibility(self) -> VisibilityKind {
        visibility_kind(self.with(&self.node.visibility))
    }
}

impl<'a> Ast<'a, UseTree> {
    optional_child!(path, Path, prefix);

    pub fn alias(self) -> Option<Ast<'a, Name>> {
        match self.node.suffix.as_ref()? {
            UseSuffix::Alias { name, .. } => name.as_ref().map(|name| self.with(name)),
            UseSuffix::Glob { .. } | UseSuffix::Group { .. } => None,
        }
    }

    pub fn is_glob(self) -> bool {
        matches!(self.node.suffix, Some(UseSuffix::Glob { .. }))
    }

    pub fn nested_trees(self) -> impl Iterator<Item = Ast<'a, UseTree>> {
        let entries = match &self.node.suffix {
            Some(UseSuffix::Group { group, .. }) => group.trees.entries.as_ref(),
            _ => &[],
        };
        entries
            .iter()
            .filter_map(move |entry| entry.element.as_ref().map(|tree| self.with(tree)))
    }
}

impl<'a> Ast<'a, UseTreeList> {
    pub fn trees(self) -> impl Iterator<Item = Ast<'a, UseTree>> {
        self.list(&self.node.trees)
    }
}

impl<'a> Ast<'a, ConstDef> {
    pub fn ty(self) -> Option<Ast<'a, TypeSyntax>> {
        self.node
            .annotation
            .as_ref()?
            .ty
            .as_ref()
            .map(|ty| self.with(ty))
    }

    pub fn initializer(self) -> Option<ExprView<'a>> {
        match &self.node.initializer {
            ConstInitializer::Definition { value, .. } => {
                value.as_ref().map(|expr| self.with(expr).view())
            }
            ConstInitializer::Declaration => None,
        }
    }
}

impl<'a> Ast<'a, StaticDef> {
    required_child!(ty, TypeSyntax, ty);
    required_expression!(initializer, initializer);

    pub fn writeability(self) -> Option<Writeability> {
        Some(self.node.binding.as_ref()?.writeability)
    }

    pub fn is_val(self) -> bool {
        self.writeability() == Some(Writeability::Val)
    }

    pub fn is_var(self) -> bool {
        self.writeability() == Some(Writeability::Var)
    }
}

impl<'a> Ast<'a, StructDef> {
    required_child!(field_list, FieldList, fields);
}

impl<'a> Ast<'a, EnumDef> {
    required_child!(variant_list, VariantList, variants);
}

impl<'a> Ast<'a, TypeAlias> {
    pub fn ty(self) -> Option<Ast<'a, TypeSyntax>> {
        let (AliasTarget::LeadingWhere { ty, .. } | AliasTarget::TrailingWhere { ty, .. }) =
            &self.node.target;
        ty.as_ref().map(|ty| self.with(ty))
    }

    pub fn where_clause(self) -> Option<Ast<'a, WhereClause>> {
        match &self.node.target {
            AliasTarget::LeadingWhere { clause, .. } => Some(self.with::<WhereClause>(clause)),
            AliasTarget::TrailingWhere { clause, .. } => clause
                .as_ref()
                .map(|clause| self.with::<WhereClause>(clause)),
        }
    }
}

impl<'a> Ast<'a, AssociatedType> {
    pub fn bounds(self) -> Option<Ast<'a, TraitBoundList>> {
        self.node
            .bounds
            .as_ref()?
            .bounds
            .as_ref()
            .map(|bounds| self.with(bounds))
    }

    pub fn ty(self) -> Option<Ast<'a, TypeSyntax>> {
        self.node
            .default
            .as_ref()?
            .ty
            .as_ref()
            .map(|ty| self.with(ty))
    }
}

impl<'a> Ast<'a, OpaqueTypeDef> {
    pub fn bounds(self) -> Option<Ast<'a, TraitBoundList>> {
        self.node
            .bounds
            .as_ref()?
            .bounds
            .as_ref()
            .map(|bounds| self.with(bounds))
    }
}

macro_rules! members {
    ($($ty:ty),+ $(,)?) => { $(
        impl<'a> Ast<'a, $ty> {
            pub fn methods(self) -> impl Iterator<Item = Ast<'a, MethodDef>> {
                self.node.members.iter().filter_map(move |member| match member { Member::MethodDef(method) => Some(self.with::<MethodDef>(method)), _ => None })
            }

            pub fn associated_types(self) -> impl Iterator<Item = Ast<'a, AssociatedType>> {
                self.node.members.iter().filter_map(move |member| match member { Member::AssociatedType(ty) => Some(self.with::<AssociatedType>(ty)), _ => None })
            }

            pub fn associated_consts(self) -> impl Iterator<Item = Ast<'a, ConstDef>> {
                self.node.members.iter().filter_map(move |member| match member { Member::ConstDef(constant) => Some(self.with::<ConstDef>(constant)), _ => None })
            }
        }
    )+ };
}

members!(TraitDef, ImplBlock);

impl<'a> Ast<'a, TraitDef> {
    pub fn supertraits(self) -> Option<Ast<'a, TraitBoundList>> {
        self.node
            .supertraits
            .as_ref()?
            .bounds
            .as_ref()
            .map(|bounds| self.with(bounds))
    }
}

impl<'a> Ast<'a, ImplBlock> {
    required_child!(target_type, TypeSyntax, target);

    pub fn trait_ref(self) -> Option<Ast<'a, TraitRef>> {
        self.node
            .trait_clause
            .as_ref()?
            .trait_ref
            .as_ref()
            .map(|trait_ref| self.with(trait_ref))
    }
}

impl<'a> Ast<'a, Attribute> {
    required_child!(path, Path, path);

    pub fn name_text(self) -> Option<String> {
        self.path()?.text()
    }

    pub fn value(self) -> Option<Ast<'a, AttributeValue>> {
        match self.node.payload.as_ref()? {
            AttributePayload::Value { value, .. } => value.as_ref().map(|value| self.with(value)),
            AttributePayload::Arguments(_) => None,
        }
    }

    pub fn args(self) -> Option<Ast<'a, AttributeArgs>> {
        match self.node.payload.as_ref()? {
            AttributePayload::Arguments(args) => Some(self.with(args)),
            AttributePayload::Value { .. } => None,
        }
    }
}

impl<'a> Ast<'a, AttributeArgs> {
    pub fn arguments(self) -> impl Iterator<Item = Ast<'a, AttributeArg>> {
        self.list(&self.node.arguments)
    }
}

impl<'a> Ast<'a, AttributeArg> {
    required_child!(value, AttributeValue, value);

    pub fn name_text(self) -> Option<String> {
        self.node
            .key
            .as_ref()
            .and_then(|key| self.with(&key.name).text())
    }
}

impl<'a> Ast<'a, AttributeValue> {
    pub fn literal(self) -> Option<Ast<'a, Literal>> {
        match self.node {
            AttributeValue::Literal(value) => Some(self.with(value)),
            _ => None,
        }
    }

    pub fn path(self) -> Option<Ast<'a, Path>> {
        match self.node {
            AttributeValue::Path(path) => Some(self.with(path)),
            _ => None,
        }
    }

    pub fn elements(self) -> impl Iterator<Item = Ast<'a, AttributeArg>> {
        let entries = match self.node {
            AttributeValue::List(list) => list.elements.entries.as_ref(),
            _ => &[],
        };
        entries
            .iter()
            .filter_map(move |entry| entry.element.as_ref().map(|argument| self.with(argument)))
    }
}

impl<'a> ItemView<'a> {
    pub fn attributes(self) -> impl Iterator<Item = Ast<'a, Attribute>> {
        let (owner, attributes) = match self {
            Self::ModuleDef(node) => (node.owner, node.node.attributes.as_slice()),
            Self::UseDecl(node) => (node.owner, node.node.attributes.as_slice()),
            Self::FnDef(node) => (node.owner, node.node.attributes.as_slice()),
            Self::ConstDef(node) => (node.owner, node.node.attributes.as_slice()),
            Self::StaticDef(node) => (node.owner, node.node.attributes.as_slice()),
            Self::StructDef(node) => (node.owner, node.node.attributes.as_slice()),
            Self::EnumDef(node) => (node.owner, node.node.attributes.as_slice()),
            Self::TraitDef(node) => (node.owner, node.node.attributes.as_slice()),
            Self::ImplBlock(node) => (node.owner, node.node.attributes.as_slice()),
            Self::TypeAlias(node) => (node.owner, node.node.attributes.as_slice()),
            Self::OpaqueTypeDef(node) => (node.owner, node.node.attributes.as_slice()),
            Self::Error(node) => (node.owner, &[][..]),
        };
        attributes.iter().map(move |node| Ast { owner, node })
    }
}
