//! Declared native dependencies and their concrete checked trait-member targets.
use crate::{
    effects::EffectSet,
    types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, reference::DefinitionReference, table::DefinitionId,
    },
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::requirement::NativeCallableRequirement,
    language::Protocol,
    ty::{Ty, substitution::TypeTransformError},
};
use serde::{Deserialize, Serialize};

pub fn normalize_requirement(
    requirement: &NativeCallableRequirement<DefinitionId>,
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<NativeCallableRequirement<DefinitionId>, TypeTransformError> {
    let definitions = catalog.definitions();
    let Ty::Trait(mut interface) =
        catalog.normalize(&Ty::Trait(requirement.interface.clone()), cancel)?
    else {
        return Err(TypeTransformError::InvalidContract);
    };
    let receiver = catalog.normalize(&requirement.receiver, cancel)?;
    if matches!(receiver, Ty::Trait(_))
        && Protocol::from_reference(&interface.declaration, Some(definitions))
            .is_some_and(Protocol::iteration)
    {
        for name in ["Item", "Iter"] {
            if name == "Iter"
                && Protocol::from_reference(&interface.declaration, Some(definitions))
                    != Some(Protocol::Iterable)
            {
                continue;
            }
            let member = definitions
                .lookup_child(
                    interface.declaration,
                    DefinitionKind::AssociatedType,
                    name,
                    0,
                )
                .ok_or(TypeTransformError::InvalidContract)?;
            if !interface.associated_types.contains_key(&member) {
                let output = catalog.normalize(
                    &Ty::Projection {
                        receiver: Box::new(receiver.clone()),
                        interface: Box::new(interface.clone()),
                        member,
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
        member: requirement.member,
        arguments: requirement
            .arguments
            .iter()
            .map(|ty| catalog.normalize(ty, cancel))
            .collect::<Result<_, _>>()?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NativeCallableOrigin {
    Implementation,
    /// A checked language protocol is materialized as an ordinary function.
    ProtocolAdapter,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct NativeCallableApplication<I = DefinitionPath> {
    pub origin: NativeCallableOrigin,
    pub requirement: NativeCallableRequirement<I>,
    pub instance: ConcreteFunctionIdentity<I>,
    pub implementation: CallableImplementation<I>,
    pub signature: Signature<I>,
    /// A common conservative callback boundary, checked independently of claims.
    pub effects: EffectSet,
}

mod mapping;
