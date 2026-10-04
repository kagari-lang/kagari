//! Symbolic callable dependencies required by a registered declaration.
use crate::ty::{
    NominalTy, Ty,
    substitution::{TypeSubstitution, TypeTransformError},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionPath, reference::DefinitionReference},
};
use serde::{Deserialize, Serialize};

mod mapping;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct NativeCallableRequirement<I = DefinitionPath> {
    pub receiver: Ty<I>,
    pub interface: NominalTy<I>,
    pub member: I,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<Ty<I>>,
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
            member: self.member.clone(),
            arguments: self
                .arguments
                .iter()
                .map(|ty| substitution.apply(ty, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}
