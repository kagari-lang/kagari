//! Names, lists and signature access with retained source context.

use std::iter;

use kagari_common::span::Span;

use crate::{
    ast::{
        borrow::{Ast, NodeData},
        item::{Visibility, VisibilityKind},
        misc::{
            AssociatedTypeBinding, CallableTrait, Field, FieldList, GenericArgList,
            GenericArgument, GenericParam, GenericParamList, Name, Param, ParamList, ParameterType,
            Path, TraitApplication, TraitBoundList, TraitRef, TuplePayload, Variant, VariantList,
            WhereClause, WherePredicate, Writeability,
        },
        site::Required,
        tokens::Token,
        ty::TypeSyntax,
        views::TypeView,
    },
    token::TokenKind,
};

impl<'a, K> Ast<'a, Token<K>> {
    pub fn text(self) -> &'a str {
        self.source_text()
    }

    pub fn kind(self) -> TokenKind {
        self.owner.lexical.kind(self.node.index() as usize)
    }
}

impl Ast<'_, Name> {
    /// Copy this name's exact spelling. Use `source_text()` for a borrowed slice.
    pub fn text(self) -> Option<String> {
        Some(self.source_text().to_owned())
    }
}

impl<'a> Ast<'a, Path> {
    /// Present path names in authored order, skipping any missing segment names.
    pub fn segments(self) -> impl Iterator<Item = Ast<'a, Name>> {
        let (first, rest) = match self.node {
            Path::Single(name) => (name, &[][..]),
            Path::Segmented(path) => (&path.first, path.rest.as_slice()),
        };
        iter::once(self.with(first)).chain(
            rest.iter()
                .filter_map(move |segment| segment.name.as_ref().map(|name| self.with(name))),
        )
    }

    /// Join present segment names with `::`, allocating normalized path text.
    /// This omits source trivia and missing segments; `source_text()` preserves
    /// exact spelling and `data()` retains malformed segment roles.
    pub fn text(self) -> Option<String> {
        Some(
            self.segments()
                .filter_map(|name| name.text())
                .collect::<Vec<_>>()
                .join("::"),
        )
    }
}

impl<'a> Ast<'a, GenericParamList> {
    pub fn params(self) -> impl Iterator<Item = Ast<'a, GenericParam>> {
        self.list(&self.node.parameters)
    }
}

impl<'a> Ast<'a, GenericParam> {
    required_child!(name, Name, name);

    pub fn name_text(self) -> Option<String> {
        self.name()?.text()
    }

    pub fn bounds(self) -> Option<Ast<'a, TraitBoundList>> {
        self.node
            .bounds
            .as_ref()?
            .bounds
            .as_ref()
            .map(|bounds| self.with(bounds))
    }
}

impl<'a> Ast<'a, GenericArgList> {
    pub fn args(self) -> impl Iterator<Item = Ast<'a, TypeSyntax>> {
        self.list(&self.node.arguments)
            .filter_map(|argument| match argument.node {
                GenericArgument::Type(ty) => Some(argument.with(ty)),
                GenericArgument::Binding(_) => None,
            })
    }

    pub fn bindings(self) -> impl Iterator<Item = Ast<'a, AssociatedTypeBinding>> {
        self.list(&self.node.arguments)
            .filter_map(|argument| match argument.node {
                GenericArgument::Binding(binding) => Some(argument.with(binding)),
                GenericArgument::Type(_) => None,
            })
    }

    pub fn positional_after_binding(self) -> bool {
        let mut binding = false;
        for argument in self.list(&self.node.arguments) {
            match argument.node {
                GenericArgument::Binding(_) => binding = true,
                GenericArgument::Type(_) if binding => return true,
                GenericArgument::Type(_) => {}
            }
        }
        false
    }
}

impl<'a> Ast<'a, AssociatedTypeBinding> {
    required_child!(name, Name, name);
    required_child!(ty, TypeSyntax, ty);

    pub fn name_text(self) -> Option<String> {
        self.name()?.text()
    }
}

impl<'a> Ast<'a, WhereClause> {
    pub fn predicates(self) -> impl Iterator<Item = Ast<'a, WherePredicate>> {
        self.list(&self.node.predicates)
    }
}

impl<'a> Ast<'a, WherePredicate> {
    required_child!(target_type, TypeSyntax, target);
    required_child!(bounds, TraitBoundList, bounds);

    pub fn name_text(self) -> Option<String> {
        match self.target_type()?.view() {
            TypeView::Named(path) => path.text(),
            TypeView::Applied(applied) => applied.path()?.text(),
            TypeView::Never(_) => Some("!".into()),
            _ => None,
        }
    }

    pub fn name(self) -> Option<Ast<'a, Name>> {
        match self.target_type()?.view() {
            TypeView::Named(path) => path.segments().next(),
            TypeView::Applied(applied) => applied.path()?.segments().next(),
            _ => None,
        }
    }
}

impl<'a> Ast<'a, TraitBoundList> {
    pub fn bounds(self) -> impl Iterator<Item = Ast<'a, TraitRef>> {
        self.node
            .first
            .as_ref()
            .into_iter()
            .chain(
                self.node
                    .rest
                    .iter()
                    .filter_map(|bound| bound.trait_ref.as_ref()),
            )
            .map(move |node| self.with(node))
    }
}

impl<'a> Ast<'a, TraitRef> {
    required_child!(path, Path, path);

    pub fn path_text(self) -> Option<String> {
        self.path()?.text()
    }

    pub fn generic_args(self) -> Option<Ast<'a, GenericArgList>> {
        match self.node.application.as_deref()? {
            TraitApplication::Generic(args) => Some(self.with(args)),
            TraitApplication::Callable(_) => None,
        }
    }

    pub fn callable(self) -> Option<Ast<'a, CallableTrait>> {
        match self.node.application.as_deref()? {
            TraitApplication::Callable(callable) => Some(self.with(callable)),
            TraitApplication::Generic(_) => None,
        }
    }
}

impl<'a> Ast<'a, CallableTrait> {
    pub fn inputs(self) -> impl Iterator<Item = Ast<'a, TypeSyntax>> {
        self.list(&self.node.inputs)
    }

    pub fn output(self) -> Option<Ast<'a, TypeSyntax>> {
        self.node
            .output
            .as_ref()?
            .ty
            .as_ref()
            .map(|ty| self.with(ty))
    }

    pub fn input_span(self) -> Span {
        let entries = &self.node.inputs.entries;
        let Some(first) = entries.first() else {
            let start = self.owner.lexical.span(self.node.open.index() as usize).end;
            let end = self.node.close.as_ref().map_or(start, |close| {
                self.owner.lexical.span(close.index() as usize).start
            });
            return Span::new(start, end);
        };
        let start = match &first.element {
            Required::Present(ty) => self.owner.body_span(ty.site()).start,
            Required::Missing { at } => *at as usize,
        };
        let last = entries.last().expect("nonempty input list");
        let end = if let Some(separator) = last.separator.as_ref().and_then(Required::as_ref) {
            self.owner.lexical.span(separator.index() as usize).end
        } else {
            match &last.element {
                Required::Present(ty) => self.owner.body_span(ty.site()).end,
                Required::Missing { at } => *at as usize,
            }
        };
        Span::new(start, end)
    }
}

impl<'a> Ast<'a, ParamList> {
    pub fn params(self) -> impl Iterator<Item = Ast<'a, Param>> {
        self.list(&self.node.parameters)
    }
}

impl<'a> Ast<'a, Param> {
    required_child!(name, Name, name);

    pub fn name_text(self) -> Option<String> {
        self.name()?.text()
    }

    pub fn ty(self) -> Option<Ast<'a, TypeSyntax>> {
        match &self.node.annotation {
            ParameterType::Explicit { ty, .. } => ty.as_ref().map(|ty| self.with(ty)),
            ParameterType::Receiver => None,
        }
    }
}

impl<'a> Ast<'a, FieldList> {
    pub fn fields(self) -> impl Iterator<Item = Ast<'a, Field>> {
        self.list(&self.node.fields)
    }
}

pub(crate) fn visibility_kind(view: Ast<'_, Visibility>) -> VisibilityKind {
    match view.node {
        Visibility::Private => VisibilityKind::Private,
        Visibility::Public {
            restriction: Some(restriction),
            ..
        } => match restriction
            .scope
            .as_ref()
            .map(|token| view.with(token).kind())
        {
            Some(TokenKind::CrateKw) => VisibilityKind::PublicCrate,
            Some(TokenKind::SuperKw) => VisibilityKind::PublicSuper,
            _ => VisibilityKind::Public,
        },
        Visibility::Public { .. } => VisibilityKind::Public,
    }
}

impl<'a> Ast<'a, Field> {
    required_child!(name, Name, name);
    required_child!(ty, TypeSyntax, ty);

    pub fn name_text(self) -> Option<String> {
        self.name()?.text()
    }

    pub fn visibility(self) -> VisibilityKind {
        visibility_kind(self.with(&self.node.visibility))
    }

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

impl<'a> Ast<'a, VariantList> {
    pub fn variants(self) -> impl Iterator<Item = Ast<'a, Variant>> {
        self.list(&self.node.variants)
    }
}

impl<'a> Ast<'a, Variant> {
    required_child!(name, Name, name);
    optional_child!(payload_types, TuplePayload, payload);

    pub fn name_text(self) -> Option<String> {
        self.name()?.text()
    }
}

impl<'a> Ast<'a, TuplePayload> {
    /// Present payload types, omitting missing slots. Use [`Self::type_entries`]
    /// when tuple positions or recovery anchors must be preserved.
    pub fn types(self) -> impl Iterator<Item = Ast<'a, TypeSyntax>> {
        self.list(&self.node.types)
    }

    /// Payload types in authored slot order, including required missing entries.
    /// For `Hole(, Point)`, this yields `Missing` at the comma followed by
    /// `Present(Point)`. Punctuation remains in `data().types.entries`.
    pub fn type_entries(self) -> impl ExactSizeIterator<Item = Required<Ast<'a, TypeSyntax>>> {
        self.node
            .types
            .entries
            .iter()
            .map(move |entry| match &entry.element {
                Required::Present(ty) => Required::Present(self.with(ty)),
                Required::Missing { at } => Required::Missing { at: *at },
            })
    }
}
