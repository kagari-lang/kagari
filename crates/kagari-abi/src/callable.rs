//! Portable callable implementation identities. Signatures belong to the
//! declarations lowered from HIR; these identities never reconstruct a signature.

pub mod generic;
pub mod interface;
pub mod shared;
pub mod witness;

use crate::types::{
    AbiType,
    substitution::{TypeSubstitution, TypeTransformError},
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionPath};
use serde::{Deserialize, Serialize};

/// Declaration policy is independent of the implementation's provider or entry.
/// Required methods must permit an implementation; defaults may forbid replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodPolicy {
    pub override_allowed: bool,
}

impl Default for MethodPolicy {
    fn default() -> Self {
        Self {
            override_allowed: true,
        }
    }
}

/// A requirement has no executable entry until implementation selection resolves
/// it. Script bodies and registered Rust entries are executable targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallableImplementation {
    Required,
    Script,
    Native(DefinitionPath),
    /// A trait default applies an ordinary registered function template. Its
    /// arguments belong to the enclosing trait/method binder, including Self.
    NativeDefault(NativeDefaultApplication),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeDefaultApplication {
    pub declaration: DefinitionPath,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<AbiType>,
}

impl NativeDefaultApplication {
    pub fn apply(
        &self,
        substitution: &TypeSubstitution<'_>,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if self.arguments.len() > 4096 {
            return Err(TypeTransformError::LimitExceeded);
        }
        Ok(Self {
            declaration: self.declaration.clone(),
            arguments: self
                .arguments
                .iter()
                .map(|argument| substitution.apply(argument, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

impl CallableImplementation {
    pub fn apply(
        &self,
        substitution: &TypeSubstitution<'_>,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        Ok(match self {
            Self::NativeDefault(application) => {
                Self::NativeDefault(application.apply(substitution, cancel)?)
            }
            _ => self.clone(),
        })
    }
}
