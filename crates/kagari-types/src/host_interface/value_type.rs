//! Composite declarations use bounded, flat preorder encoding on the wire.
use super::HostInterfaceError;
use crate::{collection::CollectionAccess, language::binding};
use bincode::Options;
use kagari_common::identity::{DefinitionPath, reference::DefinitionReference};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer, de,
    de::{Error as DecodeError, SeqAccess, Visitor},
    ser::Error as EncodeError,
};
use std::{fmt, marker::PhantomData, vec::IntoIter};

const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HostValueType<I = DefinitionPath> {
    Unit,
    Bool,
    I32,
    I64,
    F32,
    F64,
    String,
    /// An opaque type is identified by its declaration, never a registry slot.
    Opaque(I),
    Tuple(Vec<HostValueType<I>>),
    Array(Box<HostValueType<I>>, CollectionAccess),
    Map {
        key: Box<HostValueType<I>>,
        value: Box<HostValueType<I>>,
        access: CollectionAccess,
    },
    Set(Box<HostValueType<I>>, CollectionAccess),
    Option(I, Box<HostValueType<I>>),
    Result {
        declaration: I,
        ok: Box<HostValueType<I>>,
        error: Box<HostValueType<I>>,
    },
}

impl<I: DefinitionReference> HostValueType<I> {
    pub fn nominal_references(&self) -> Vec<&I> {
        let mut pending = vec![self];
        let mut declarations = Vec::new();
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Opaque(id) => declarations.push(id),
                Self::Tuple(elements) => pending.extend(elements),
                Self::Array(element, _) | Self::Set(element, _) => pending.push(element),
                Self::Option(_, element) => pending.push(element),
                Self::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Result { ok, error, .. } => pending.extend([ok.as_ref(), error.as_ref()]),
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

    fn nodes(&self) -> Result<Vec<Node<I>>, HostInterfaceError> {
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
                    if !id.within_path_limit() {
                        return Err(HostInterfaceError::TooLarge);
                    }
                    if let Some(path) = id.authoring_path() {
                        super::validate_host_type_identity(path)?;
                    }
                    Node::Opaque(id.clone())
                }
                Self::Tuple(elements) => {
                    if elements.len() > MAX_NODES {
                        return Err(HostInterfaceError::TooLarge);
                    }
                    pending.extend(elements.iter().rev().map(|ty| (ty, depth + 1)));
                    Node::Tuple(elements.len() as u32)
                }
                Self::Array(element, _) | Self::Set(element, _) => {
                    if matches!(ty, Self::Set(_, _)) && !element.hash_key() {
                        return Err(HostInterfaceError::InvalidDeclaration);
                    }
                    pending.push((element, depth + 1));
                    match ty {
                        Self::Array(_, access) => Node::Array(*access),
                        Self::Set(_, access) => Node::Set(*access),
                        _ => unreachable!(),
                    }
                }
                Self::Option(declaration, element) => {
                    if declaration
                        .authoring_path()
                        .is_some_and(|path| path != &binding::option_declaration())
                    {
                        return Err(HostInterfaceError::InvalidDeclaration);
                    }
                    if !declaration.within_path_limit() {
                        return Err(HostInterfaceError::TooLarge);
                    }
                    pending.push((element, depth + 1));
                    Node::Option(declaration.clone())
                }
                Self::Map { key, value, access } => {
                    if !key.hash_key() {
                        return Err(HostInterfaceError::InvalidDeclaration);
                    }
                    pending.push((value, depth + 1));
                    pending.push((key, depth + 1));
                    Node::Map(*access)
                }
                Self::Result {
                    declaration,
                    ok,
                    error,
                } => {
                    if declaration
                        .authoring_path()
                        .is_some_and(|path| path != &binding::result_declaration())
                    {
                        return Err(HostInterfaceError::InvalidDeclaration);
                    }
                    if !declaration.within_path_limit() {
                        return Err(HostInterfaceError::TooLarge);
                    }
                    pending.push((error, depth + 1));
                    pending.push((ok, depth + 1));
                    Node::Result(declaration.clone())
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
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
enum Node<I = DefinitionPath> {
    Unit,
    Bool,
    I32,
    I64,
    F32,
    F64,
    String,
    Opaque(I),
    Tuple(u32),
    Array(CollectionAccess),
    Map(CollectionAccess),
    Set(CollectionAccess),
    Option(I),
    Result(I),
}

impl<I: DefinitionReference + Serialize> Serialize for HostValueType<I> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.nodes()
            .map_err(EncodeError::custom)?
            .serialize(serializer)
    }
}

impl<'de, I: DefinitionReference + Deserialize<'de>> Deserialize<'de> for HostValueType<I> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TypeVisitor<I>(PhantomData<I>);

        impl<'de, I: DefinitionReference + Deserialize<'de>> Visitor<'de> for TypeVisitor<I> {
            type Value = HostValueType<I>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a bounded preorder host type")
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                if sequence.size_hint().is_some_and(|count| count > MAX_NODES) {
                    return Err(DecodeError::custom("host type node limit"));
                }
                let mut nodes = Vec::new();
                while let Some(node) = sequence.next_element::<Node<I>>()? {
                    if nodes.len() >= MAX_NODES {
                        return Err(DecodeError::custom("host type node limit"));
                    }
                    nodes.push(node);
                }
                let mut nodes = nodes.into_iter();
                let ty = build::<I, A::Error>(&mut nodes, 1)?;
                if nodes.next().is_some() {
                    return Err(DecodeError::custom("trailing host type nodes"));
                }
                ty.validate().map_err(DecodeError::custom)?;
                Ok(ty)
            }
        }
        deserializer.deserialize_seq(TypeVisitor(PhantomData))
    }
}

fn build<I: DefinitionReference, E: de::Error>(
    nodes: &mut IntoIter<Node<I>>,
    depth: usize,
) -> Result<HostValueType<I>, E> {
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
        Node::Array(access) => HostValueType::Array(Box::new(build(nodes, depth + 1)?), access),
        Node::Set(access) => HostValueType::Set(Box::new(build(nodes, depth + 1)?), access),
        Node::Option(declaration) => {
            HostValueType::Option(declaration, Box::new(build(nodes, depth + 1)?))
        }
        Node::Map(access) => HostValueType::Map {
            key: Box::new(build(nodes, depth + 1)?),
            value: Box::new(build(nodes, depth + 1)?),
            access,
        },
        Node::Result(declaration) => HostValueType::Result {
            declaration,
            ok: Box::new(build(nodes, depth + 1)?),
            error: Box::new(build(nodes, depth + 1)?),
        },
    };
    Ok(ty)
}

impl HostValueType {
    /// Host Option carriers reference the installed nominal core declaration.
    pub fn option(element: Self) -> Self {
        Self::Option(binding::option_declaration(), Box::new(element))
    }

    /// Host Result carriers reference the installed nominal core declaration.
    pub fn result(ok: Self, error: Self) -> Self {
        Self::Result {
            declaration: binding::result_declaration(),
            ok: Box::new(ok),
            error: Box::new(error),
        }
    }

    pub fn fingerprint(&self) -> Result<u64, HostInterfaceError> {
        let bytes = super::codec()
            .serialize(self)
            .map_err(|_| HostInterfaceError::Encoding)?;
        Ok(super::hash(
            b"kagari-host-value-v1\0".iter().copied().chain(bytes),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_interface::{HostFunctionDeclaration, HostInterface};

    #[test]
    fn collection_access_round_trips_and_changes_host_binding_fingerprints() {
        use CollectionAccess::{Mutable, ReadOnly};
        for (writable, readable) in [
            (
                HostValueType::Array(Box::new(HostValueType::I32), Mutable),
                HostValueType::Array(Box::new(HostValueType::I32), ReadOnly),
            ),
            (
                HostValueType::Set(Box::new(HostValueType::String), Mutable),
                HostValueType::Set(Box::new(HostValueType::String), ReadOnly),
            ),
            (
                HostValueType::Map {
                    key: Box::new(HostValueType::String),
                    value: Box::new(HostValueType::Array(Box::new(HostValueType::I32), ReadOnly)),
                    access: Mutable,
                },
                HostValueType::Map {
                    key: Box::new(HostValueType::String),
                    value: Box::new(HostValueType::Array(Box::new(HostValueType::I32), ReadOnly)),
                    access: ReadOnly,
                },
            ),
        ] {
            let before = HostFunctionDeclaration::new("host.read", vec![], writable.clone());
            let after = HostFunctionDeclaration::new("host.read", vec![], readable.clone());
            assert_ne!(before.fingerprint().unwrap(), after.fingerprint().unwrap());
            assert!(!before.matches_binding(&after));
            assert_ne!(
                writable.fingerprint().unwrap(),
                readable.fingerprint().unwrap()
            );
            for ty in [writable, readable] {
                let bytes = bincode::serialize(&ty).unwrap();
                assert_eq!(bincode::deserialize::<HostValueType>(&bytes).unwrap(), ty);
            }
        }
        let mut legacy = HostInterface::default().to_bytes().unwrap();
        legacy[4..6].copy_from_slice(&11u16.to_le_bytes());
        assert_eq!(
            HostInterface::from_bytes(&legacy),
            Err(HostInterfaceError::Version)
        );
    }

    #[test]
    fn composite_types_round_trip_and_change_binding_fingerprints() {
        let ty = HostValueType::Result {
            declaration: binding::result_declaration(),
            ok: Box::new(HostValueType::Tuple(vec![
                HostValueType::Array(Box::new(HostValueType::I32), CollectionAccess::Mutable),
                HostValueType::option(HostValueType::String),
            ])),
            error: Box::new(HostValueType::Map {
                key: Box::new(HostValueType::String),
                value: Box::new(HostValueType::Set(
                    Box::new(HostValueType::I64),
                    CollectionAccess::Mutable,
                )),
                access: CollectionAccess::Mutable,
            }),
        };
        let a = HostFunctionDeclaration::new("host.make", vec![], ty);
        let interface = HostInterface {
            paths: vec![],
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
            vec![Node::<DefinitionPath>::I32, Node::Bool],
            vec![Node::Tuple(u32::MAX)],
            vec![Node::Map(CollectionAccess::Mutable), Node::F32, Node::I32],
            (0..MAX_DEPTH)
                .map(|_| Node::Array(CollectionAccess::Mutable))
                .chain([Node::I32])
                .collect(),
            (0..=MAX_NODES).map(|_| Node::I32).collect(),
        ] {
            let bytes = bincode::serialize(&nodes).unwrap();
            assert!(bincode::deserialize::<HostValueType>(&bytes).is_err());
        }
        let mut ty = HostValueType::I32;
        for _ in 1..MAX_DEPTH {
            ty = HostValueType::Array(Box::new(ty), CollectionAccess::Mutable);
        }
        let bytes = bincode::serialize(&ty).unwrap();
        assert_eq!(bincode::deserialize::<HostValueType>(&bytes).unwrap(), ty);
        assert!(
            HostValueType::Array(Box::new(ty), CollectionAccess::Mutable)
                .validate()
                .is_err()
        );
        assert!(
            HostValueType::Tuple(vec![HostValueType::<DefinitionPath>::I32; MAX_NODES])
                .validate()
                .is_err()
        );
    }
}

mod mapping;
