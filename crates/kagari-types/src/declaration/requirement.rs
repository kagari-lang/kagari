//! Symbolic callable dependencies required by a registered declaration.
use crate::ty::{
    NominalTy, Ty,
    substitution::{TypeSubstitution, TypeTransformError},
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};

mod mapping;
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NativeCallableRequirement {
    pub receiver: Ty,
    pub interface: NominalTy,
    pub member: DefinitionId,
    pub arguments: Vec<Ty>,
}

impl NativeCallableRequirement {
    pub fn apply(
        &self,
        substitution: &TypeSubstitution<'_>,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        Ok(Self {
            receiver: substitution.apply(&self.receiver, cancel)?,
            interface: substitution.apply_nominal(&self.interface, cancel)?,
            member: self.member,
            arguments: self
                .arguments
                .iter()
                .map(|ty| substitution.apply(ty, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}
