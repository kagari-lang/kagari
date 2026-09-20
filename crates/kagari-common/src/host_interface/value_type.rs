//! Composite declarations use bounded, flat preorder encoding on the wire.
use super::HostInterfaceError;
use crate::identity::DefinitionId;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, SeqAccess, Visitor},
};

const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HostValueType {
    Unit,
    Bool,
    I32,
    I64,
    F32,
    F64,
    String,
    /// An opaque type is identified by its declaration, never a registry slot.
    Opaque(DefinitionId),
    Tuple(Vec<HostValueType>),
    Array(Box<HostValueType>),
    Map {
        key: Box<HostValueType>,
        value: Box<HostValueType>,
    },
    Set(Box<HostValueType>),
    Option(Box<HostValueType>),
    Result {
        ok: Box<HostValueType>,
        error: Box<HostValueType>,
    },
}

impl HostValueType {
    pub fn fingerprint(&self) -> Result<u64, HostInterfaceError> {
        use bincode::Options;
        let bytes = super::codec()
            .serialize(self)
            .map_err(|_| HostInterfaceError::Encoding)?;
        Ok(super::hash(
            b"kagari-host-value-v1\0".iter().copied().chain(bytes),
        ))
    }
    pub fn nominal_references(&self) -> Vec<&DefinitionId> {
        let mut pending = vec![self];
        let mut declarations = Vec::new();
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Opaque(id) => declarations.push(id),
                Self::Tuple(elements) => pending.extend(elements),
                Self::Array(element) | Self::Set(element) | Self::Option(element) => {
                    pending.push(element)
                }
                Self::Map { key, value } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Result { ok, error } => pending.extend([ok.as_ref(), error.as_ref()]),
                _ => {}
            }
        }
        declarations
    }

    fn hash_key(&self) -> bool {
        matches!(self, Self::Bool | Self::I32 | Self::I64 | Self::String)
    }

    pub(super) fn validate(&self) -> Result<(), HostInterfaceError> {
        self.nodes().map(|_| ())
    }

    fn nodes(&self) -> Result<Vec<Node>, HostInterfaceError> {
        let mut pending = vec![(self, 1)];
        let mut nodes = Vec::new();
        while let Some((ty, depth)) = pending.pop() {
            if depth > MAX_DEPTH || nodes.len() >= MAX_NODES {
                return Err(HostInterfaceError::TooLarge);
            }
            nodes.push(match ty {
                Self::Unit => Node::Unit,
                Self::Bool => Node::Bool,
                Self::I32 => Node::I32,
                Self::I64 => Node::I64,
                Self::F32 => Node::F32,
                Self::F64 => Node::F64,
                Self::String => Node::String,
                Self::Opaque(id) => {
                    super::validate_host_type_identity(id)?;
                    Node::Opaque(id.clone())
                }
                Self::Tuple(elements) => {
                    if elements.len() > MAX_NODES {
                        return Err(HostInterfaceError::TooLarge);
                    }
                    pending.extend(elements.iter().rev().map(|ty| (ty, depth + 1)));
                    Node::Tuple(elements.len() as u32)
                }
                Self::Array(element) | Self::Set(element) | Self::Option(element) => {
                    if matches!(ty, Self::Set(_)) && !element.hash_key() {
                        return Err(HostInterfaceError::InvalidDeclaration);
                    }
                    pending.push((element, depth + 1));
                    match ty {
                        Self::Array(_) => Node::Array,
                        Self::Set(_) => Node::Set,
                        _ => Node::Option,
                    }
                }
                Self::Map { key, value } => {
                    if !key.hash_key() {
                        return Err(HostInterfaceError::InvalidDeclaration);
                    }
                    pending.push((value, depth + 1));
                    pending.push((key, depth + 1));
                    Node::Map
                }
                Self::Result { ok, error } => {
                    pending.push((error, depth + 1));
                    pending.push((ok, depth + 1));
                    Node::Result
                }
            });
            if nodes.len() + pending.len() > MAX_NODES {
                return Err(HostInterfaceError::TooLarge);
            }
        }
        Ok(nodes)
    }
}

// Wire nodes contain no recursive type references. Decoding cannot grow the
// Rust call stack until node count and depth have been bounded explicitly.
#[derive(Serialize, Deserialize)]
enum Node {
    Unit,
    Bool,
    I32,
    I64,
    F32,
    F64,
    String,
    Opaque(DefinitionId),
    Tuple(u32),
    Array,
    Map,
    Set,
    Option,
    Result,
}

impl Serialize for HostValueType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.nodes()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for HostValueType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TypeVisitor;
        impl<'de> Visitor<'de> for TypeVisitor {
            type Value = HostValueType;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a bounded preorder host type")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                if sequence.size_hint().is_some_and(|count| count > MAX_NODES) {
                    return Err(de::Error::custom("host type node limit"));
                }
                let mut nodes = Vec::new();
                while let Some(node) = sequence.next_element::<Node>()? {
                    if nodes.len() >= MAX_NODES {
                        return Err(de::Error::custom("host type node limit"));
                    }
                    nodes.push(node);
                }
                let mut nodes = nodes.into_iter();
                let ty = build::<A::Error>(&mut nodes, 1)?;
                if nodes.next().is_some() {
                    return Err(de::Error::custom("trailing host type nodes"));
                }
                ty.validate().map_err(de::Error::custom)?;
                Ok(ty)
            }
        }
        deserializer.deserialize_seq(TypeVisitor)
    }
}

fn build<E: de::Error>(
    nodes: &mut std::vec::IntoIter<Node>,
    depth: usize,
) -> Result<HostValueType, E> {
    if depth > MAX_DEPTH {
        return Err(E::custom("host type depth limit"));
    }
    let ty = match nodes
        .next()
        .ok_or_else(|| E::custom("missing host type node"))?
    {
        Node::Unit => HostValueType::Unit,
        Node::Bool => HostValueType::Bool,
        Node::I32 => HostValueType::I32,
        Node::I64 => HostValueType::I64,
        Node::F32 => HostValueType::F32,
        Node::F64 => HostValueType::F64,
        Node::String => HostValueType::String,
        Node::Opaque(id) => HostValueType::Opaque(id),
        Node::Tuple(count) => {
            if count as usize > nodes.len() {
                return Err(E::custom("missing tuple type nodes"));
            }
            HostValueType::Tuple(
                (0..count)
                    .map(|_| build(nodes, depth + 1))
                    .collect::<Result<_, E>>()?,
            )
        }
        Node::Array => HostValueType::Array(Box::new(build(nodes, depth + 1)?)),
        Node::Set => HostValueType::Set(Box::new(build(nodes, depth + 1)?)),
        Node::Option => HostValueType::Option(Box::new(build(nodes, depth + 1)?)),
        Node::Map => HostValueType::Map {
            key: Box::new(build(nodes, depth + 1)?),
            value: Box::new(build(nodes, depth + 1)?),
        },
        Node::Result => HostValueType::Result {
            ok: Box::new(build(nodes, depth + 1)?),
            error: Box::new(build(nodes, depth + 1)?),
        },
    };
    Ok(ty)
}

#[cfg(test)]
mod tests {
    use super::super::{HostFunctionDeclaration, HostInterface};
    use super::*;

    #[test]
    fn composite_types_round_trip_and_change_binding_fingerprints() {
        let ty = HostValueType::Result {
            ok: Box::new(HostValueType::Tuple(vec![
                HostValueType::Array(Box::new(HostValueType::I32)),
                HostValueType::Option(Box::new(HostValueType::String)),
            ])),
            error: Box::new(HostValueType::Map {
                key: Box::new(HostValueType::String),
                value: Box::new(HostValueType::Set(Box::new(HostValueType::I64))),
            }),
        };
        let a = HostFunctionDeclaration::new("host.make", vec![], ty);
        let interface = HostInterface {
            field_paths: vec![],
            types: Vec::new(),
            functions: vec![a.clone()],
        };
        let mut bytes = interface.to_bytes().unwrap();
        assert_eq!(HostInterface::from_bytes(&bytes).unwrap(), interface);
        let b = HostFunctionDeclaration::new("host.make", vec![], HostValueType::I32);
        assert_ne!(a.fingerprint().unwrap(), b.fingerprint().unwrap());
        bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            HostInterface::from_bytes(&bytes),
            Err(HostInterfaceError::Version)
        );
    }

    #[test]
    fn malformed_wire_types_reject_depth_counts_arity_and_invalid_hash_keys() {
        for nodes in [
            vec![],
            vec![Node::I32, Node::Bool],
            vec![Node::Tuple(u32::MAX)],
            vec![Node::Map, Node::F32, Node::I32],
            (0..MAX_DEPTH)
                .map(|_| Node::Array)
                .chain([Node::I32])
                .collect(),
            (0..=MAX_NODES).map(|_| Node::I32).collect(),
        ] {
            let bytes = bincode::serialize(&nodes).unwrap();
            assert!(bincode::deserialize::<HostValueType>(&bytes).is_err());
        }
        let mut ty = HostValueType::I32;
        for _ in 1..MAX_DEPTH {
            ty = HostValueType::Array(Box::new(ty));
        }
        let bytes = bincode::serialize(&ty).unwrap();
        assert_eq!(bincode::deserialize::<HostValueType>(&bytes).unwrap(), ty);
        assert!(HostValueType::Array(Box::new(ty)).validate().is_err());
        assert!(
            HostValueType::Tuple(vec![HostValueType::I32; MAX_NODES])
                .validate()
                .is_err()
        );
    }
}
