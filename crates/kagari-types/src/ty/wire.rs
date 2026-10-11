//! Bounded flat encoding keeps untrusted ABI type decoding off the Rust call stack.

use crate::{
    collection::CollectionAccess,
    range::RangeKind,
    scalar::BuiltinType,
    ty::{
        NominalTy, Ty,
        identity::IdentityTransformError,
        substitution::{
            MAX_TYPE_DEPTH as MAX_DEPTH, MAX_TYPE_NODES as MAX_NODES, TypeTransformError,
        },
    },
};

use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{
        DefinitionId,
        wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, vec::IntoIter};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum Node<I> {
    Host(I),
    SelfType(I),
    Parameter {
        owner: I,
        position: usize,
    },
    Builtin(BuiltinType),
    Tuple(u32),
    Function(u32),
    Iter,
    Range(RangeKind),
    Array,
    Map(CollectionAccess),
    Set(CollectionAccess),
    Struct(I, u32),
    NativeObject(I, u32),
    Enum(I, u32),
    Trait(
        I,
        u32,
        #[serde(deserialize_with = "crate::decode_limits::nested")] Vec<I>,
    ),
    Projection {
        member_arguments: u32,
        member: I,
        owner: I,
        arguments: u32,
        #[serde(deserialize_with = "crate::decode_limits::nested")]
        bindings: Vec<I>,
    },
}

/// Codec-owned flat records. References are local to the enclosing file table.
/// Deserialization bounds node and binding counts without building a recursive type;
/// decode validates shape, depth, binding order and reference scope before adoption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WireType {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    nodes: Vec<Node<PortableDefinitionRef>>,
}

impl WireType {
    /// Encode scoped types using the artifact's canonical file table.
    /// Returns identity, bounded shape or cancellation errors.
    pub fn encode(
        ty: &Ty,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, IdentityTransformError> {
        Ok(Self {
            nodes: ty.wire_nodes_with(
                &mut |id| encoding.reference(*id).map_err(Into::into),
                Some(cancel),
            )?,
        })
    }

    /// Resolve bounded nodes into the decoded table's scope, without verification.
    /// Returns identity, bounded shape, canonical binding or cancellation errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<Ty, IdentityTransformError> {
        let mut nodes = self.nodes.into_iter();
        let ty = build(
            &mut nodes,
            1,
            &mut |id| decoding.resolve(id).map_err(Into::into),
            Some(cancel),
        )?;
        if nodes.next().is_some() {
            return Err(TypeTransformError::InvalidContract.into());
        }
        Ok(ty)
    }
}

fn ordered_bindings<'a, J: Ord>(
    bindings: &'a BTreeMap<DefinitionId, Ty>,
    convert: &mut impl FnMut(&DefinitionId) -> Result<J, IdentityTransformError>,
) -> Result<Vec<(J, &'a Ty)>, IdentityTransformError> {
    let mut result = bindings
        .iter()
        .map(|(id, ty)| Ok((convert(id)?, ty)))
        .collect::<Result<Vec<_>, IdentityTransformError>>()?;
    result.sort_by(|(left, _), (right, _)| left.cmp(right));
    if result.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(TypeTransformError::InvalidContract.into());
    }
    Ok(result)
}

impl Ty {
    pub fn within_wire_limits(&self) -> bool {
        self.wire_nodes().is_ok()
    }

    fn wire_nodes(&self) -> Result<Vec<Node<DefinitionId>>, IdentityTransformError> {
        self.wire_nodes_with(&mut |id| Ok(*id), None)
    }

    fn wire_nodes_with<J: Ord>(
        &self,
        convert: &mut impl FnMut(&DefinitionId) -> Result<J, IdentityTransformError>,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<Node<J>>, IdentityTransformError> {
        let mut pending = vec![(self, 1usize)];
        let mut nodes = Vec::new();
        while let Some((ty, depth)) = pending.pop() {
            if let Some(cancel) = cancel {
                cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            }
            if depth > MAX_DEPTH || nodes.len() >= MAX_NODES {
                return Err(TypeTransformError::LimitExceeded.into());
            }
            let node = match ty {
                Self::Host(id) => Node::Host(convert(id)?),
                Self::SelfType(id) => Node::SelfType(convert(id)?),
                Self::Parameter { owner, position } => Node::Parameter {
                    owner: convert(owner)?,
                    position: *position,
                },
                Self::Builtin(ty) => Node::Builtin(*ty),
                Self::Tuple(elements) => {
                    if elements.len() > MAX_NODES {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    pending.extend(elements.iter().rev().map(|ty| (ty, depth + 1)));
                    Node::Tuple(elements.len() as u32)
                }
                Self::Function { params, result } => {
                    if params.len() >= MAX_NODES {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    pending.push((result, depth + 1));
                    pending.extend(params.iter().rev().map(|ty| (ty, depth + 1)));
                    Node::Function(params.len() as u32)
                }
                Self::Range(element, kind) => {
                    pending.push((element, depth + 1));
                    Node::Range(*kind)
                }
                Self::Iter(element) => {
                    pending.push((element, depth + 1));
                    Node::Iter
                }
                Self::Array(element) => {
                    pending.push((element, depth + 1));
                    Node::Array
                }
                Self::Map { key, value, access } => {
                    pending.push((value, depth + 1));
                    pending.push((key, depth + 1));
                    Node::Map(*access)
                }
                Self::Set(element, access) => {
                    pending.push((element, depth + 1));
                    Node::Set(*access)
                }
                Self::Struct(nominal) | Self::NativeObject(nominal) => {
                    if !nominal.associated_types.is_empty() {
                        return Err(TypeTransformError::InvalidContract.into());
                    }
                    if nominal.arguments.len() > MAX_NODES {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    pending.extend(nominal.arguments.iter().rev().map(|ty| (ty, depth + 1)));
                    if matches!(ty, Self::NativeObject(_)) {
                        Node::NativeObject(
                            convert(&nominal.declaration)?,
                            nominal.arguments.len() as u32,
                        )
                    } else {
                        Node::Struct(
                            convert(&nominal.declaration)?,
                            nominal.arguments.len() as u32,
                        )
                    }
                }
                Self::Enum(ty) => {
                    if !ty.associated_types.is_empty() {
                        return Err(TypeTransformError::InvalidContract.into());
                    }
                    if ty.arguments.len() > MAX_NODES {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    pending.extend(ty.arguments.iter().rev().map(|ty| (ty, depth + 1)));
                    Node::Enum(convert(&ty.declaration)?, ty.arguments.len() as u32)
                }
                Self::Trait(ty) => {
                    if ty.arguments.len() > MAX_NODES {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    if ty.associated_types.len() > MAX_NODES {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    let bindings = ordered_bindings(&ty.associated_types, convert)?;
                    pending.extend(bindings.iter().rev().map(|(_, ty)| (*ty, depth + 1)));
                    pending.extend(ty.arguments.iter().rev().map(|ty| (ty, depth + 1)));
                    Node::Trait(
                        convert(&ty.declaration)?,
                        ty.arguments.len() as u32,
                        bindings.into_iter().map(|(id, _)| id).collect(),
                    )
                }
                Self::Projection {
                    receiver,
                    interface,
                    member,
                    arguments,
                } => {
                    if arguments.len()
                        + interface.arguments.len()
                        + interface.associated_types.len()
                        > MAX_NODES
                    {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    pending.extend(arguments.iter().rev().map(|ty| (ty, depth + 1)));
                    let bindings = ordered_bindings(&interface.associated_types, convert)?;
                    pending.extend(bindings.iter().rev().map(|(_, ty)| (*ty, depth + 1)));
                    pending.extend(interface.arguments.iter().rev().map(|ty| (ty, depth + 1)));
                    pending.push((receiver, depth + 1));
                    Node::Projection {
                        member_arguments: arguments.len() as u32,
                        member: convert(member)?,
                        owner: convert(&interface.declaration)?,
                        arguments: interface.arguments.len() as u32,
                        bindings: bindings.into_iter().map(|(id, _)| id).collect(),
                    }
                }
            };
            nodes.push(node);
            if nodes.len() + pending.len() > MAX_NODES {
                return Err(TypeTransformError::LimitExceeded.into());
            }
        }
        Ok(nodes)
    }
}

fn build(
    nodes: &mut IntoIter<Node<PortableDefinitionRef>>,
    depth: usize,
    convert: &mut impl FnMut(PortableDefinitionRef) -> Result<DefinitionId, IdentityTransformError>,
    cancel: Option<&CancellationToken>,
) -> Result<Ty, IdentityTransformError> {
    if let Some(cancel) = cancel {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
    }
    if depth > MAX_DEPTH {
        return Err(TypeTransformError::LimitExceeded.into());
    }
    let next = nodes.next().ok_or(TypeTransformError::InvalidContract)?;
    Ok(match next {
        Node::Host(id) => Ty::Host(convert(id)?),
        Node::SelfType(id) => Ty::SelfType(convert(id)?),
        Node::Parameter { owner, position } => Ty::Parameter {
            owner: convert(owner)?,
            position,
        },
        Node::Builtin(ty) => Ty::Builtin(ty),
        Node::Tuple(count) => Ty::Tuple(children(nodes, count, depth, convert, cancel)?),
        Node::Function(count) => Ty::Function {
            params: children(nodes, count, depth, convert, cancel)?,
            result: Box::new(build(nodes, depth + 1, convert, cancel)?),
        },
        Node::Range(kind) => Ty::Range(Box::new(build(nodes, depth + 1, convert, cancel)?), kind),
        Node::Iter => Ty::Iter(Box::new(build(nodes, depth + 1, convert, cancel)?)),
        Node::Array => Ty::Array(Box::new(build(nodes, depth + 1, convert, cancel)?)),
        Node::Map(access) => Ty::Map {
            key: Box::new(build(nodes, depth + 1, convert, cancel)?),
            value: Box::new(build(nodes, depth + 1, convert, cancel)?),
            access,
        },
        Node::Set(access) => Ty::Set(Box::new(build(nodes, depth + 1, convert, cancel)?), access),
        Node::Struct(id, count) => Ty::Struct(NominalTy {
            associated_types: Default::default(),
            declaration: convert(id)?,
            arguments: children(nodes, count, depth, convert, cancel)?,
        }),
        Node::NativeObject(id, count) => Ty::NativeObject(NominalTy {
            associated_types: Default::default(),
            declaration: convert(id)?,
            arguments: children(nodes, count, depth, convert, cancel)?,
        }),
        Node::Enum(id, count) => Ty::Enum(NominalTy {
            associated_types: Default::default(),
            declaration: convert(id)?,
            arguments: children(nodes, count, depth, convert, cancel)?,
        }),
        Node::Trait(id, count, bindings) => {
            let arguments = children(nodes, count, depth, convert, cancel)?;
            let mut associated_types = BTreeMap::new();
            let mut previous = None;
            for id in bindings {
                if previous.as_ref().is_some_and(|previous| previous >= &id) {
                    return Err(TypeTransformError::InvalidContract.into());
                }
                previous = Some(id);
                let id = convert(id)?;
                if associated_types
                    .insert(id, build(nodes, depth + 1, convert, cancel)?)
                    .is_some()
                {
                    return Err(TypeTransformError::InvalidContract.into());
                }
            }
            Ty::Trait(NominalTy {
                declaration: convert(id)?,
                arguments,
                associated_types,
            })
        }
        Node::Projection {
            member_arguments,
            member,
            owner,
            arguments,
            bindings,
        } => {
            let receiver = Box::new(build(nodes, depth + 1, convert, cancel)?);
            let arguments = children(nodes, arguments, depth, convert, cancel)?;
            let mut associated_types = BTreeMap::new();
            let mut previous = None;
            for id in bindings {
                if previous.as_ref().is_some_and(|previous| previous >= &id) {
                    return Err(TypeTransformError::InvalidContract.into());
                }
                previous = Some(id);
                let id = convert(id)?;
                if associated_types
                    .insert(id, build(nodes, depth + 1, convert, cancel)?)
                    .is_some()
                {
                    return Err(TypeTransformError::InvalidContract.into());
                }
            }
            Ty::Projection {
                arguments: children(nodes, member_arguments, depth, convert, cancel)?,
                receiver,
                interface: Box::new(NominalTy {
                    declaration: convert(owner)?,
                    arguments,
                    associated_types,
                }),
                member: convert(member)?,
            }
        }
    })
}

fn children(
    nodes: &mut IntoIter<Node<PortableDefinitionRef>>,
    count: u32,
    depth: usize,
    convert: &mut impl FnMut(PortableDefinitionRef) -> Result<DefinitionId, IdentityTransformError>,
    cancel: Option<&CancellationToken>,
) -> Result<Vec<Ty>, IdentityTransformError> {
    if count as usize > nodes.len() {
        return Err(TypeTransformError::InvalidContract.into());
    }
    (0..count)
        .map(|_| build(nodes, depth + 1, convert, cancel))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::identity::PortableTypes;
    use bincode::Options;
    use kagari_common::identity::table::DefinitionTableBuilder;
    use kagari_common::identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
    };

    fn codec() -> impl Options {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
    }

    #[test]
    fn associated_type_wire_preserves_identity_and_rejects_noncanonical_bindings() {
        let trait_id = DefinitionPath {
            module: ModuleIdentity::single_file("associated.kgr"),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Trait,
                name: "Read".into(),
                occurrence: 0,
            }],
        };
        let member = kagari_common::identity::associated_type_id(&trait_id, "Item");
        let mut definitions = DefinitionTableBuilder::new().unwrap();
        let owner = definitions.intern_path(&trait_id).unwrap();
        let item = definitions.intern_path(&member).unwrap();
        let other = definitions
            .intern_path(&kagari_common::identity::associated_type_id(
                &trait_id, "Other",
            ))
            .unwrap();
        let encoding = definitions.freeze().encode([owner, item, other]).unwrap();
        let scoped_owner = owner;
        let scoped_item = item;
        let owner = encoding.reference(owner).unwrap();
        let item = encoding.reference(item).unwrap();
        let other = encoding.reference(other).unwrap();
        let decoding = encoding.table.clone().decode().unwrap();
        for bindings in [
            vec![item, item],
            vec![other, item],
            vec![item; MAX_NODES + 1],
        ] {
            let nodes = vec![
                Node::Trait(owner, 0, bindings),
                Node::Builtin(BuiltinType::I32),
                Node::Builtin(BuiltinType::Bool),
            ];
            match codec().deserialize::<WireType>(&codec().serialize(&nodes).unwrap()) {
                Err(_) => {}
                Ok(wire) => assert!(wire.decode(&decoding, &Default::default()).is_err()),
            }
        }
        let invalid = Ty::Struct(NominalTy {
            declaration: scoped_owner,
            arguments: Vec::new(),
            associated_types: [(scoped_item, Ty::Builtin(BuiltinType::I32))].into(),
        });
        assert!(WireType::encode(&invalid, &encoding, &Default::default()).is_err());
    }

    #[test]
    fn abi_type_wire_rejects_malformed_and_oversized_nodes() {
        let empty = DefinitionTableBuilder::new().unwrap().freeze();
        let decoding = empty.encode([]).unwrap().table.decode().unwrap();
        let reject = |nodes: Vec<Node<PortableDefinitionRef>>| {
            let bytes = codec().serialize(&nodes).unwrap();
            match codec().deserialize::<WireType>(&bytes) {
                Err(_) => {}
                Ok(wire) => assert!(wire.decode(&decoding, &Default::default()).is_err()),
            }
        };
        for nodes in [
            vec![],
            vec![Node::Array],
            vec![Node::Function(1), Node::Builtin(BuiltinType::I32)],
            vec![Node::Tuple(2), Node::Builtin(BuiltinType::I32)],
            vec![
                Node::Builtin(BuiltinType::I32),
                Node::Builtin(BuiltinType::I64),
            ],
        ] {
            reject(nodes);
        }
        reject(
            (0..MAX_NODES + 1)
                .map(|_| Node::Builtin(BuiltinType::I32))
                .collect(),
        );
        let too_wide: Ty = Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); MAX_NODES]);
        assert!(PortableTypes::encode(&empty, &[too_wide], &Default::default()).is_err());

        let mut deep = Ty::Builtin(BuiltinType::I32);
        for _ in 0..MAX_DEPTH {
            deep = Ty::Array(Box::new(deep));
        }
        assert!(PortableTypes::encode(&empty, &[deep], &Default::default()).is_err());
        let mut nodes = (0..MAX_DEPTH).map(|_| Node::Array).collect::<Vec<_>>();
        nodes.push(Node::Builtin(BuiltinType::I32));
        reject(nodes);
    }
}
