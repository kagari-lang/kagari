//! Portable callable implementation identities. Signatures belong to the
//! declarations lowered from HIR; these identities never reconstruct a signature.

use crate::standard::{
    StandardIntrinsic,
    bindings::{NativeDefaultMethod, NativeProtocolMethod},
};
use kagari_common::{identity::DefinitionId, integer::IntegerMethod};
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
/// it. Script bodies and provider-qualified Rust bindings are executable targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallableImplementation {
    Required,
    Script,
    Native(NativeBinding),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeBinding {
    Engine(EngineNativeBinding),
    Host(DefinitionId),
}

/// Closed engine operation identity. Integer owner types, generic applications
/// and callback signatures are carried by the checked callable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EngineNativeBinding {
    Intrinsic(StandardIntrinsic),
    Integer(IntegerMethod),
    ParseRadix,
    TraitDefault(NativeDefaultMethod),
    Protocol(NativeProtocolMethod),
}

/// Provider is retained while source contracts become linked import references.
/// The host payload remains the full registration contract or its checked import
/// reference; it never becomes an engine operation merely by sharing a name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeCall<Engine, Host> {
    Engine(Engine),
    Host(Host),
}
