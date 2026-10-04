//! Declared native dependencies and their concrete checked trait-member targets.
use crate::{
    effects::EffectSet,
    native_import::NativeSignature,
    types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionPath, associated_type_id, reference::DefinitionReference},
};
use kagari_types::declaration::requirement::NativeCallableRequirement;
use kagari_types::{
    callable::CallableImplementation,
    language::Protocol,
    ty::{Ty, substitution::TypeTransformError},
};
use serde::{Deserialize, Serialize};

pub fn normalize_requirement(
    requirement: &NativeCallableRequirement,
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<NativeCallableRequirement, TypeTransformError> {
    let Ty::Trait(mut interface) =
        catalog.normalize(&Ty::Trait(requirement.interface.clone()), cancel)?
    else {
        return Err(TypeTransformError::InvalidContract);
    };
    let receiver = catalog.normalize(&requirement.receiver, cancel)?;
    if matches!(receiver, Ty::Trait(_))
        && Protocol::from_id(&interface.declaration).is_some_and(Protocol::iteration)
    {
        for name in ["Item", "Iter"] {
            if name == "Iter"
                && Protocol::from_id(&interface.declaration) != Some(Protocol::Iterable)
            {
                continue;
            }
            let member = associated_type_id(&interface.declaration, name);
            if !interface.associated_types.contains_key(&member) {
                let output = catalog.normalize(
                    &Ty::Projection {
                        receiver: Box::new(receiver.clone()),
                        interface: Box::new(interface.clone()),
                        member: member.clone(),
                        arguments: vec![],
                    },
                    cancel,
                )?;
                interface.associated_types.insert(member, output);
            }
        }
    }
    Ok(NativeCallableRequirement {
        receiver,
        interface,
        member: requirement.member.clone(),
        arguments: requirement
            .arguments
            .iter()
            .map(|ty| catalog.normalize(ty, cancel))
            .collect::<Result<_, _>>()?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeCallableOrigin {
    Implementation,
    /// A checked language protocol is materialized as an ordinary function.
    ProtocolAdapter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct NativeCallableApplication<I = DefinitionPath> {
    pub origin: NativeCallableOrigin,
    pub requirement: NativeCallableRequirement<I>,
    pub instance: ConcreteFunctionIdentity<I>,
    pub implementation: CallableImplementation<I>,
    pub signature: NativeSignature<I>,
    /// A common conservative callback boundary, checked independently of claims.
    pub effects: EffectSet,
}

mod mapping;
