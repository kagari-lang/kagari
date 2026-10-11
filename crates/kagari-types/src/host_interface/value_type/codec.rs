//! Flat file-local Host values share the bounded shape builder with the model.
use crate::host_interface::{
    HostInterfaceError,
    value_type::{HostValueType, Node, build, read_nodes},
    wire::{Encode, check_cancel, identity_error},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
};
use serde::{Deserialize, Serialize, de::value::Error as DecodeError};

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::host_interface) struct WireHostValueType {
    #[serde(deserialize_with = "read_nodes")]
    nodes: Vec<Node<PortableDefinitionRef>>,
}

impl Encode for HostValueType {
    type Wire = WireHostValueType;

    fn encode(
        &self,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self::Wire, HostInterfaceError> {
        check_cancel(cancel)?;
        let nodes = self
            .nodes()?
            .into_iter()
            .map(|node| {
                check_cancel(cancel)?;
                node.map_reference(&mut |id| encoding.reference(id).map_err(identity_error))
            })
            .collect::<Result<_, _>>()?;
        Ok(WireHostValueType { nodes })
    }
}

impl WireHostValueType {
    pub(in crate::host_interface) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<HostValueType, HostInterfaceError> {
        check_cancel(cancel)?;
        let nodes = self
            .nodes
            .into_iter()
            .map(|node| {
                check_cancel(cancel)?;
                node.map_reference(&mut |id| decoding.resolve(id).map_err(identity_error))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut nodes = nodes.into_iter();
        let ty = build::<DecodeError>(&mut nodes, 1).map_err(|_| HostInterfaceError::Encoding)?;
        if nodes.next().is_some() {
            return Err(HostInterfaceError::Encoding);
        }
        ty.validate().map_err(|_| HostInterfaceError::Encoding)?;
        Ok(ty)
    }
}

impl<I> Node<I> {
    fn map_reference<J>(
        self,
        convert: &mut impl FnMut(I) -> Result<J, HostInterfaceError>,
    ) -> Result<Node<J>, HostInterfaceError> {
        Ok(match self {
            Self::Unit => Node::Unit,
            Self::Bool => Node::Bool,
            Self::I32 => Node::I32,
            Self::I64 => Node::I64,
            Self::F32 => Node::F32,
            Self::F64 => Node::F64,
            Self::String => Node::String,
            Self::Array => Node::Array,
            Self::Opaque(id) => Node::Opaque(convert(id)?),
            Self::Vec(id) => Node::Vec(convert(id)?),
            Self::List(id) => Node::List(convert(id)?),
            Self::Option(id) => Node::Option(convert(id)?),
            Self::Result(id) => Node::Result(convert(id)?),
            Self::Tuple(value) => Node::Tuple(value),
            Self::Map(value) => Node::Map(value),
            Self::Set(value) => Node::Set(value),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        collection::CollectionAccess,
        host_interface::value_type::{MAX_DEPTH, MAX_NODES},
    };
    use kagari_common::identity::map::DefinitionContext;

    #[test]
    fn malformed_wire_types_reject_depth_counts_arity_and_invalid_hash_keys() {
        let context = DefinitionContext::new().unwrap();
        let encoding = context.snapshot().encode([]).unwrap();
        let decoding = encoding.table.decode().unwrap();
        for nodes in [
            vec![],
            vec![Node::<PortableDefinitionRef>::I32, Node::Bool],
            vec![Node::Tuple(u32::MAX)],
            vec![Node::Map(CollectionAccess::Mutable), Node::F32, Node::I32],
            (0..MAX_DEPTH)
                .map(|_| Node::Array)
                .chain([Node::I32])
                .collect(),
            (0..=MAX_NODES).map(|_| Node::I32).collect(),
        ] {
            let bytes = bincode::serialize(&nodes).unwrap();
            let result = bincode::deserialize::<WireHostValueType>(&bytes)
                .map_err(|_| HostInterfaceError::Encoding)
                .and_then(|wire| wire.decode(&decoding, &CancellationToken::default()));
            assert!(result.is_err());
        }
        let mut ty = HostValueType::I32;
        for _ in 1..MAX_DEPTH {
            ty = HostValueType::Array(Box::new(ty));
        }
        let bytes =
            bincode::serialize(&ty.encode(&encoding, &CancellationToken::default()).unwrap())
                .unwrap();
        assert_eq!(
            bincode::deserialize::<WireHostValueType>(&bytes)
                .unwrap()
                .decode(&decoding, &CancellationToken::default())
                .unwrap(),
            ty
        );
        assert!(
            HostValueType::Array(Box::new(ty))
                .encode(&encoding, &CancellationToken::default())
                .is_err()
        );
        assert!(
            HostValueType::Tuple(vec![HostValueType::I32; MAX_NODES])
                .encode(&encoding, &CancellationToken::default())
                .is_err()
        );
    }
}
