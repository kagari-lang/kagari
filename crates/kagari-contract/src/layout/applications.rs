//! Aggregate templates bind only their declaration's own type parameters.
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{DefinitionId, DefinitionTable},
};

use crate::layout::{EnumLayout, StructLayout};
use kagari_types::{
    declaration::verify::types_in_scope,
    ty::{GenericParam, Ty, substitution::TypeSubstitution},
};
use std::borrow::Cow;

fn parameters(owner: &DefinitionId, arguments: &[Ty]) -> Option<Vec<GenericParam>> {
    if arguments.iter().all(Ty::is_concrete) {
        return Some(vec![]);
    }
    arguments
        .iter()
        .enumerate()
        .map(|(position, ty)| {
            let parameter = GenericParam {
                owner: *owner,
                position,
            };
            (parameter.as_type() == *ty).then_some(parameter)
        })
        .collect()
}

fn accepts(owner: &DefinitionId, template: &[Ty], arguments: &[Ty]) -> bool {
    template == arguments
        || (template.len() == arguments.len()
            && parameters(owner, template).is_some_and(|parameters| !parameters.is_empty()))
}

impl StructLayout {
    pub fn accepts(&self, arguments: &[Ty]) -> bool {
        accepts(&self.declaration, &self.arguments, arguments)
    }

    pub fn apply<'a>(
        &'a self,
        arguments: &[Ty],
        cancel: &CancellationToken,
    ) -> Option<Cow<'a, Self>> {
        if self.arguments == arguments {
            return Some(Cow::Borrowed(self));
        }
        if !self.accepts(arguments) {
            return None;
        }
        let substitution = TypeSubstitution::for_owner(&self.declaration, arguments);
        let mut applied = self.clone();
        applied.arguments = arguments.to_vec();
        for field in &mut applied.fields {
            field.ty = substitution.apply(&field.ty, cancel).ok()?;
        }
        Some(Cow::Owned(applied))
    }
}

impl EnumLayout {
    pub fn accepts(&self, arguments: &[Ty]) -> bool {
        accepts(&self.declaration, &self.arguments, arguments)
    }

    pub fn apply<'a>(
        &'a self,
        arguments: &[Ty],
        cancel: &CancellationToken,
    ) -> Option<Cow<'a, Self>> {
        if self.arguments == arguments {
            return Some(Cow::Borrowed(self));
        }
        if !self.accepts(arguments) {
            return None;
        }
        let substitution = TypeSubstitution::for_owner(&self.declaration, arguments);
        let mut applied = self.clone();
        applied.arguments = arguments.to_vec();
        for variant in &mut applied.variants {
            for ty in &mut variant.payload {
                *ty = substitution.apply(ty, cancel).ok()?;
            }
        }
        Some(Cow::Owned(applied))
    }
}

impl StructLayout {
    pub fn types_valid(&self, cancel: &CancellationToken, definitions: &DefinitionTable) -> bool {
        parameters(&self.declaration, &self.arguments).is_some_and(|parameters| {
            types_in_scope(
                self.arguments
                    .iter()
                    .chain(self.fields.iter().map(|field| &field.ty)),
                &parameters,
                cancel,
                definitions,
            )
        })
    }
}

impl EnumLayout {
    pub fn types_valid(&self, cancel: &CancellationToken, definitions: &DefinitionTable) -> bool {
        parameters(&self.declaration, &self.arguments).is_some_and(|parameters| {
            types_in_scope(
                self.arguments
                    .iter()
                    .chain(self.variants.iter().flat_map(|variant| &variant.payload)),
                &parameters,
                cancel,
                definitions,
            )
        })
    }
}
