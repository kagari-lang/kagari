//! Portable callable implementation identities. Signatures belong to the
//! declarations lowered from HIR; these identities never reconstruct a signature.

use kagari_common::identity::DefinitionId;
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
    Native(DefinitionId),
}
