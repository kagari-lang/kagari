//! Private serialized Host declarations; decode only into scoped IDs.
use crate::host_interface::{
    HostFunctionDeclaration, HostFunctionEffects, HostInterfaceError, HostParameter,
    HostPassingStyle, decode_limits,
    type_declaration::{
        HostAssociatedTypeBinding, HostFieldDeclaration, HostMethodDeclaration,
        HostReflectionPolicy, HostTraitImplementationDeclaration, HostTraitMethodBinding,
        HostTypeDeclaration, HostTypeOwnership, PathAccess, Visibility,
    },
    value_type::codec::WireHostValueType,
    wire::{Encode, check_cancel, identity_error},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{
        DefinitionId,
        wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(in crate::host_interface) struct WireHostParameter {
    name: String,
    ty: WireHostValueType,
    passing: HostPassingStyle,
}

impl Encode for HostParameter<DefinitionId> {
    type Wire = WireHostParameter;

    fn encode(
        &self,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self::Wire, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(WireHostParameter {
            name: self.name.clone(),
            ty: self.ty.encode(encoding, cancel)?,
            passing: self.passing,
        })
    }
}

impl WireHostParameter {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostParameter<DefinitionId>, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(HostParameter {
            name: self.name,
            ty: self.ty.decode(decoding, cancel)?,
            passing: self.passing,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct WireHostFunctionDeclaration {
    id: PortableDefinitionRef,
    symbol: String,
    #[serde(deserialize_with = "decode_limits::members")]
    params: Vec<WireHostParameter>,
    return_type: WireHostValueType,
    effects: HostFunctionEffects,
    documentation: String,
}

impl Encode for HostFunctionDeclaration<DefinitionId> {
    type Wire = WireHostFunctionDeclaration;

    fn encode(
        &self,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self::Wire, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(WireHostFunctionDeclaration {
            id: encoding.reference(self.id).map_err(identity_error)?,
            symbol: self.symbol.clone(),
            params: self
                .params
                .iter()
                .map(|value| value.encode(encoding, cancel))
                .collect::<Result<_, _>>()?,
            return_type: self.return_type.encode(encoding, cancel)?,
            effects: self.effects,
            documentation: self.documentation.clone(),
        })
    }
}

impl WireHostFunctionDeclaration {
    /// Encode scoped facts with the enclosing file table and cancellation token.
    /// Returns reference, bounded shape or cancellation errors without granting
    /// Host validation or execution evidence.
    pub fn encode(
        value: &HostFunctionDeclaration<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, HostInterfaceError> {
        value.encode(encoding, cancel)
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostFunctionDeclaration<DefinitionId>, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(HostFunctionDeclaration {
            id: decoding.resolve(self.id).map_err(identity_error)?,
            symbol: self.symbol,
            params: self
                .params
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            return_type: self.return_type.decode(decoding, cancel)?,
            effects: self.effects,
            documentation: self.documentation,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(in crate::host_interface) struct WireHostFieldDeclaration {
    id: PortableDefinitionRef,
    name: String,
    ty: WireHostValueType,
    readable: bool,
    writable: bool,
    visibility: Visibility,
    path_access: PathAccess,
    documentation: String,
}

impl Encode for HostFieldDeclaration<DefinitionId> {
    type Wire = WireHostFieldDeclaration;

    fn encode(
        &self,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self::Wire, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(WireHostFieldDeclaration {
            id: encoding.reference(self.id).map_err(identity_error)?,
            name: self.name.clone(),
            ty: self.ty.encode(encoding, cancel)?,
            readable: self.readable,
            writable: self.writable,
            visibility: self.visibility,
            path_access: self.path_access,
            documentation: self.documentation.clone(),
        })
    }
}

impl WireHostFieldDeclaration {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostFieldDeclaration<DefinitionId>, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(HostFieldDeclaration {
            id: decoding.resolve(self.id).map_err(identity_error)?,
            name: self.name,
            ty: self.ty.decode(decoding, cancel)?,
            readable: self.readable,
            writable: self.writable,
            visibility: self.visibility,
            path_access: self.path_access,
            documentation: self.documentation,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(in crate::host_interface) struct WireHostMethodDeclaration {
    id: PortableDefinitionRef,
    name: String,
    receiver: HostPassingStyle,
    #[serde(deserialize_with = "decode_limits::members")]
    params: Vec<WireHostParameter>,
    return_type: WireHostValueType,
    effects: HostFunctionEffects,
    documentation: String,
}

impl Encode for HostMethodDeclaration<DefinitionId> {
    type Wire = WireHostMethodDeclaration;

    fn encode(
        &self,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self::Wire, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(WireHostMethodDeclaration {
            id: encoding.reference(self.id).map_err(identity_error)?,
            name: self.name.clone(),
            receiver: self.receiver,
            params: self
                .params
                .iter()
                .map(|value| value.encode(encoding, cancel))
                .collect::<Result<_, _>>()?,
            return_type: self.return_type.encode(encoding, cancel)?,
            effects: self.effects,
            documentation: self.documentation.clone(),
        })
    }
}

impl WireHostMethodDeclaration {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostMethodDeclaration<DefinitionId>, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(HostMethodDeclaration {
            id: decoding.resolve(self.id).map_err(identity_error)?,
            name: self.name,
            receiver: self.receiver,
            params: self
                .params
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            return_type: self.return_type.decode(decoding, cancel)?,
            effects: self.effects,
            documentation: self.documentation,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(in crate::host_interface) struct WireHostTraitMethodBinding {
    trait_method: PortableDefinitionRef,
    host_method: PortableDefinitionRef,
}

impl Encode for HostTraitMethodBinding<DefinitionId> {
    type Wire = WireHostTraitMethodBinding;

    fn encode(
        &self,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self::Wire, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(WireHostTraitMethodBinding {
            trait_method: encoding
                .reference(self.trait_method)
                .map_err(identity_error)?,
            host_method: encoding
                .reference(self.host_method)
                .map_err(identity_error)?,
        })
    }
}

impl WireHostTraitMethodBinding {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostTraitMethodBinding<DefinitionId>, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(HostTraitMethodBinding {
            trait_method: decoding
                .resolve(self.trait_method)
                .map_err(identity_error)?,
            host_method: decoding.resolve(self.host_method).map_err(identity_error)?,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(in crate::host_interface) struct WireHostAssociatedTypeBinding {
    declaration: PortableDefinitionRef,
    ty: WireHostValueType,
}

impl Encode for HostAssociatedTypeBinding<DefinitionId> {
    type Wire = WireHostAssociatedTypeBinding;

    fn encode(
        &self,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self::Wire, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(WireHostAssociatedTypeBinding {
            declaration: encoding
                .reference(self.declaration)
                .map_err(identity_error)?,
            ty: self.ty.encode(encoding, cancel)?,
        })
    }
}

impl WireHostAssociatedTypeBinding {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostAssociatedTypeBinding<DefinitionId>, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(HostAssociatedTypeBinding {
            declaration: decoding.resolve(self.declaration).map_err(identity_error)?,
            ty: self.ty.decode(decoding, cancel)?,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(in crate::host_interface) struct WireHostTraitImplementationDeclaration {
    trait_id: PortableDefinitionRef,
    #[serde(deserialize_with = "decode_limits::members")]
    trait_arguments: Vec<WireHostValueType>,
    #[serde(deserialize_with = "decode_limits::members")]
    associated_types: Vec<WireHostAssociatedTypeBinding>,
    #[serde(deserialize_with = "decode_limits::members")]
    methods: Vec<WireHostTraitMethodBinding>,
    documentation: String,
}

impl Encode for HostTraitImplementationDeclaration<DefinitionId> {
    type Wire = WireHostTraitImplementationDeclaration;

    fn encode(
        &self,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self::Wire, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(WireHostTraitImplementationDeclaration {
            trait_id: encoding.reference(self.trait_id).map_err(identity_error)?,
            trait_arguments: self
                .trait_arguments
                .iter()
                .map(|value| value.encode(encoding, cancel))
                .collect::<Result<_, _>>()?,
            associated_types: self
                .associated_types
                .iter()
                .map(|value| value.encode(encoding, cancel))
                .collect::<Result<_, _>>()?,
            methods: self
                .methods
                .iter()
                .map(|value| value.encode(encoding, cancel))
                .collect::<Result<_, _>>()?,
            documentation: self.documentation.clone(),
        })
    }
}

impl WireHostTraitImplementationDeclaration {
    pub(super) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostTraitImplementationDeclaration<DefinitionId>, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(HostTraitImplementationDeclaration {
            trait_id: decoding.resolve(self.trait_id).map_err(identity_error)?,
            trait_arguments: self
                .trait_arguments
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            associated_types: self
                .associated_types
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            methods: self
                .methods
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            documentation: self.documentation,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct WireHostTypeDeclaration {
    id: PortableDefinitionRef,
    symbol: String,
    ownership: HostTypeOwnership,
    #[serde(deserialize_with = "decode_limits::members")]
    fields: Vec<WireHostFieldDeclaration>,
    #[serde(deserialize_with = "decode_limits::members")]
    methods: Vec<WireHostMethodDeclaration>,
    #[serde(deserialize_with = "decode_limits::members")]
    trait_implementations: Vec<WireHostTraitImplementationDeclaration>,
    path_access: PathAccess,
    reflection: HostReflectionPolicy,
    documentation: String,
}

impl Encode for HostTypeDeclaration<DefinitionId> {
    type Wire = WireHostTypeDeclaration;

    fn encode(
        &self,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self::Wire, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(WireHostTypeDeclaration {
            id: encoding.reference(self.id).map_err(identity_error)?,
            symbol: self.symbol.clone(),
            ownership: self.ownership,
            fields: self
                .fields
                .iter()
                .map(|value| value.encode(encoding, cancel))
                .collect::<Result<_, _>>()?,
            methods: self
                .methods
                .iter()
                .map(|value| value.encode(encoding, cancel))
                .collect::<Result<_, _>>()?,
            trait_implementations: self
                .trait_implementations
                .iter()
                .map(|value| value.encode(encoding, cancel))
                .collect::<Result<_, _>>()?,
            path_access: self.path_access,
            reflection: self.reflection,
            documentation: self.documentation.clone(),
        })
    }
}

impl WireHostTypeDeclaration {
    /// Encode scoped facts with the enclosing file table and cancellation token.
    /// Returns reference, bounded shape or cancellation errors without granting
    /// Host validation or execution evidence.
    pub fn encode(
        value: &HostTypeDeclaration<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, HostInterfaceError> {
        value.encode(encoding, cancel)
    }

    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostTypeDeclaration<DefinitionId>, HostInterfaceError> {
        check_cancel(cancel)?;
        Ok(HostTypeDeclaration {
            id: decoding.resolve(self.id).map_err(identity_error)?,
            symbol: self.symbol,
            ownership: self.ownership,
            fields: self
                .fields
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            methods: self
                .methods
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            trait_implementations: self
                .trait_implementations
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            path_access: self.path_access,
            reflection: self.reflection,
            documentation: self.documentation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_interface::{
        HostInterface, MAGIC, VERSION, codec,
        wire::{Envelope, HostInterfaceRecord},
    };
    use bincode::Options;
    use kagari_common::identity::map::DefinitionContext;

    #[test]
    fn khi_file_references_must_resolve_before_host_validation() {
        let context = DefinitionContext::new().unwrap();
        let interface = HostInterface {
            types: vec![HostTypeDeclaration::new(&context, "demo.Player").unwrap()],
            ..Default::default()
        };
        let cancel = CancellationToken::default();
        let encoding = context.snapshot().encode([interface.types[0].id]).unwrap();
        let record =
            WireHostTypeDeclaration::encode(&interface.types[0], &encoding, &cancel).unwrap();
        let decoding = encoding.table.clone().decode().unwrap();
        cancel.cancel();
        assert!(matches!(
            WireHostTypeDeclaration::encode(&interface.types[0], &encoding, &cancel),
            Err(HostInterfaceError::Cancelled)
        ));
        assert!(matches!(
            record.decode(&decoding, &cancel),
            Err(HostInterfaceError::Cancelled)
        ));
        let bytes = interface.to_bytes(&context.snapshot()).unwrap();
        let mut envelope: Envelope<HostInterfaceRecord> = codec().deserialize(&bytes[6..]).unwrap();
        envelope.records.types[0].id = codec().deserialize(&u32::MAX.to_le_bytes()).unwrap();
        let mut invalid = MAGIC.to_vec();
        invalid.extend(VERSION.to_le_bytes());
        invalid.extend(codec().serialize(&envelope).unwrap());
        assert!(matches!(
            HostInterface::<DefinitionId>::from_bytes(&invalid),
            Err(HostInterfaceError::InvalidDeclaration)
        ));
    }
}
