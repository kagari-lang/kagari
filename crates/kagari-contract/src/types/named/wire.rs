//! Portable named contracts; decoding grants no declaration or executable authority.

use crate::types::{
    InterfaceBridge,
    named::{
        AliasContract, FunctionOrigin, ImplementationContract, NamedContract, NamedDeclaration,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use kagari_types::{
    declaration::wire::{
        declarations::ConstDefRecord,
        signature::{FnDeclRecord, GenericBoundRecord, GenericParamRecord},
    },
    ty::wire::WireType,
    visibility::Visibility,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct FunctionOriginRecord {
    placement: PortableDefinitionRef,
    authored: PortableDefinitionRef,
}

impl FunctionOriginRecord {
    pub fn encode(
        value: &FunctionOrigin,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            placement: encoding.reference(value.placement)?,
            authored: encoding.reference(value.authored)?,
        })
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<FunctionOrigin, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(FunctionOrigin {
            placement: decoding.resolve(self.placement)?,
            authored: decoding.resolve(self.authored)?,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct NamedDeclarationRecord {
    declaration: PortableDefinitionRef,
    visibility: Visibility,
    ambiguous_local_body: Option<String>,
    contract: ContractRecord,
}

#[derive(Serialize, Deserialize)]
enum ContractRecord {
    Function(FnDeclRecord),
    Constant(ConstDefRecord),
    Alias {
        name: String,
        binders: Binders,
        target: WireType,
    },
    Implementation {
        bridge: Option<InterfaceBridge>,
        binders: Binders,
        receiver: WireType,
        trait_type: Option<WireType>,
    },
}

#[derive(Serialize, Deserialize)]
struct Binders {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    parameters: Vec<GenericParamRecord>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    bounds: Vec<GenericBoundRecord>,
}

impl NamedDeclarationRecord {
    /// Encode using the enclosing artifact's explicit definition table.
    pub fn encode(
        value: &NamedDeclaration,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        let contract = match &value.contract {
            NamedContract::Function(value) => {
                ContractRecord::Function(FnDeclRecord::encode(value, encoding, cancel)?)
            }
            NamedContract::Constant(value) => {
                ContractRecord::Constant(ConstDefRecord::encode(value, encoding, cancel)?)
            }
            NamedContract::Alias(value) => ContractRecord::Alias {
                name: value.name.clone(),
                binders: Binders {
                    parameters: value
                        .generic_params
                        .iter()
                        .map(|value| GenericParamRecord::encode(value, encoding, cancel))
                        .collect::<Result<_, _>>()?,
                    bounds: value
                        .bounds
                        .iter()
                        .map(|value| GenericBoundRecord::encode(value, encoding, cancel))
                        .collect::<Result<_, _>>()?,
                },
                target: WireType::encode(&value.target, encoding, cancel)?,
            },
            NamedContract::Implementation(value) => ContractRecord::Implementation {
                bridge: value.bridge,
                binders: Binders {
                    parameters: value
                        .generic_params
                        .iter()
                        .map(|value| GenericParamRecord::encode(value, encoding, cancel))
                        .collect::<Result<_, _>>()?,
                    bounds: value
                        .bounds
                        .iter()
                        .map(|value| GenericBoundRecord::encode(value, encoding, cancel))
                        .collect::<Result<_, _>>()?,
                },
                receiver: WireType::encode(&value.receiver, encoding, cancel)?,
                trait_type: value
                    .trait_type
                    .as_ref()
                    .map(|ty| WireType::encode(ty, encoding, cancel))
                    .transpose()?,
            },
        };
        Ok(Self {
            declaration: encoding.reference(value.declaration)?,
            visibility: value.visibility,
            ambiguous_local_body: value.ambiguous_local_body.clone(),
            contract,
        })
    }

    /// Resolve file references; callers must perform semantic and executable checks.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<NamedDeclaration, DefinitionMappingError> {
        check_cancel(cancel)?;
        let contract = match self.contract {
            ContractRecord::Function(value) => {
                NamedContract::Function(value.decode(decoding, cancel)?)
            }
            ContractRecord::Constant(value) => {
                NamedContract::Constant(value.decode(decoding, cancel)?)
            }
            ContractRecord::Alias {
                name,
                binders,
                target,
            } => NamedContract::Alias(AliasContract {
                name,
                generic_params: binders
                    .parameters
                    .into_iter()
                    .map(|value| value.decode(decoding, cancel))
                    .collect::<Result<_, _>>()?,
                bounds: binders
                    .bounds
                    .into_iter()
                    .map(|value| value.decode(decoding, cancel))
                    .collect::<Result<_, _>>()?,
                target: target.decode(decoding, cancel)?,
            }),
            ContractRecord::Implementation {
                bridge,
                binders,
                receiver,
                trait_type,
            } => NamedContract::Implementation(ImplementationContract {
                bridge,
                generic_params: binders
                    .parameters
                    .into_iter()
                    .map(|value| value.decode(decoding, cancel))
                    .collect::<Result<_, _>>()?,
                bounds: binders
                    .bounds
                    .into_iter()
                    .map(|value| value.decode(decoding, cancel))
                    .collect::<Result<_, _>>()?,
                receiver: receiver.decode(decoding, cancel)?,
                trait_type: trait_type
                    .map(|ty| ty.decode(decoding, cancel))
                    .transpose()?,
            }),
        };
        Ok(NamedDeclaration {
            declaration: decoding.resolve(self.declaration)?,
            visibility: self.visibility,
            ambiguous_local_body: self.ambiguous_local_body,
            contract,
        })
    }
}
