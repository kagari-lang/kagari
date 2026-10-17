//! Callable implementation selectors with scoped definition identities. Signatures belong to the
//! declarations lowered from HIR; these identities never reconstruct a signature.
use crate::{
    reference::ReferenceAccess,
    ty::{
        Ty,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};
use serde::{Deserialize, Serialize};

/// Declaration policy is independent of the implementation's provider or entry.
/// Required methods must permit an implementation; defaults may forbid replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodPolicy {
    pub override_allowed: bool,
    /// A short Rust payload loan required by an explicitly registered member.
    /// This does not change the nominal receiver's storage or expose Rust traits.
    pub receiver_access: Option<ReferenceAccess>,
}

impl Default for MethodPolicy {
    fn default() -> Self {
        Self {
            override_allowed: true,
            receiver_access: None,
        }
    }
}

/// A requirement has no executable entry until implementation selection resolves
/// it. Script bodies and registered Rust entries are executable targets.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CallableImplementation {
    Required,
    Script,
    Native(DefinitionId),
    /// A trait default applies an ordinary registered function template. Its
    /// arguments belong to the enclosing trait/method binder, including Self.
    NativeDefault(NativeDefaultApplication),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NativeDefaultApplication {
    pub declaration: DefinitionId,
    pub arguments: Vec<Ty>,
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
            declaration: self.declaration,
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

mod mapping;

/// Parameter and result semantics, independent of an executable target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Signature {
    pub params: Vec<Ty>,
    pub result: Ty,
}
