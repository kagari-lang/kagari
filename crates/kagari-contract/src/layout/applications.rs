//! Aggregate templates bind only their declaration's own type parameters.
use crate::{
    layout::{EnumLayout, StructLayout},
    types::{GenericParam, Ty, substitution::TypeSubstitution, verify::types_in_scope},
};
use kagari_common::{cancellation::CancellationToken, identity::reference::DefinitionReference};
use std::borrow::Cow;

fn parameters<I: DefinitionReference>(
    owner: &I,
    arguments: &[Ty<I>],
) -> Option<Vec<GenericParam<I>>> {
    if arguments.iter().all(Ty::is_concrete) {
        return Some(vec![]);
    }
    arguments
        .iter()
        .enumerate()
        .map(|(position, ty)| {
            let parameter = GenericParam {
                owner: owner.clone(),
                position,
            };
            (parameter.as_type() == *ty).then_some(parameter)
        })
        .collect()
}

fn accepts<I: DefinitionReference>(owner: &I, template: &[Ty<I>], arguments: &[Ty<I>]) -> bool {
    template == arguments
        || (template.len() == arguments.len()
            && parameters(owner, template).is_some_and(|parameters| !parameters.is_empty()))
}

impl<I: DefinitionReference> StructLayout<I> {
    pub fn accepts(&self, arguments: &[Ty<I>]) -> bool {
        accepts(&self.declaration, &self.arguments, arguments)
    }

    pub fn apply<'a>(
        &'a self,
        arguments: &[Ty<I>],
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

impl<I: DefinitionReference> EnumLayout<I> {
    pub fn accepts(&self, arguments: &[Ty<I>]) -> bool {
        accepts(&self.declaration, &self.arguments, arguments)
    }

    pub fn apply<'a>(
        &'a self,
        arguments: &[Ty<I>],
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
    pub fn types_valid(&self, cancel: &CancellationToken) -> bool {
        parameters(&self.declaration, &self.arguments).is_some_and(|parameters| {
            types_in_scope(
                self.arguments
                    .iter()
                    .chain(self.fields.iter().map(|field| &field.ty)),
                &parameters,
                cancel,
            )
        })
    }
}

impl EnumLayout {
    pub fn types_valid(&self, cancel: &CancellationToken) -> bool {
        parameters(&self.declaration, &self.arguments).is_some_and(|parameters| {
            types_in_scope(
                self.arguments
                    .iter()
                    .chain(self.variants.iter().flat_map(|variant| &variant.payload)),
                &parameters,
                cancel,
            )
        })
    }
}
