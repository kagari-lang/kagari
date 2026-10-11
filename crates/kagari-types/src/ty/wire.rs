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
    identity::{
        reference::DefinitionReference,
        table::{
            DefinitionId,
            wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
        },
    },
};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error as DeError, SeqAccess, Visitor},
    ser::Error,
};
use std::{collections::BTreeMap, fmt, fmt::Formatter, marker::PhantomData, vec::IntoIter};

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
pub(crate) struct WireType {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    nodes: Vec<Node<PortableDefinitionRef>>,
}

impl WireType {
    pub(crate) fn encode(
        ty: &Ty<DefinitionId>,
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

    pub(crate) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<Ty<DefinitionId>, IdentityTransformError> {
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

fn ordered_bindings<'a, I, J: Ord>(
    bindings: &'a BTreeMap<I, Ty<I>>,
    convert: &mut impl FnMut(&I) -> Result<J, IdentityTransformError>,
) -> Result<Vec<(J, &'a Ty<I>)>, IdentityTransformError> {
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

impl<I: DefinitionReference> Ty<I> {
    pub fn within_wire_limits(&self) -> bool {
        self.wire_nodes().is_ok()
    }

    fn wire_nodes(&self) -> Result<Vec<Node<I>>, IdentityTransformError> {
        self.wire_nodes_with(&mut |id| Ok(id.clone()), None)
    }

    fn wire_nodes_with<J: Ord>(
        &self,
        convert: &mut impl FnMut(&I) -> Result<J, IdentityTransformError>,
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
                Self::Host(id) => {
                    if !id.within_path_limit() {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    Node::Host(convert(id)?)
                }
                Self::SelfType(id) => {
                    if !id.within_path_limit() {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    Node::SelfType(convert(id)?)
                }
                Self::Parameter { owner, position } => {
                    if !owner.within_path_limit() {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    Node::Parameter {
                        owner: convert(owner)?,
                        position: *position,
                    }
                }
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
                    if !nominal.declaration.within_path_limit() {
                        return Err(TypeTransformError::LimitExceeded.into());
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
                    if !ty.declaration.within_path_limit() {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    if ty.arguments.len() > MAX_NODES {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    pending.extend(ty.arguments.iter().rev().map(|ty| (ty, depth + 1)));
                    Node::Enum(convert(&ty.declaration)?, ty.arguments.len() as u32)
                }
                Self::Trait(ty) => {
                    if !ty.declaration.within_path_limit() {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    if ty.arguments.len() > MAX_NODES {
                        return Err(TypeTransformError::LimitExceeded.into());
                    }
                    if ty.associated_types.len() > MAX_NODES
                        || ty.associated_types.keys().any(|id| !id.within_path_limit())
                    {
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
                    if !member.within_path_limit()
                        || !interface.declaration.within_path_limit()
                        || arguments.len()
                            + interface.arguments.len()
                            + interface.associated_types.len()
                            > MAX_NODES
                        || interface
                            .associated_types
                            .keys()
                            .any(|id| !id.within_path_limit())
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

impl<I: DefinitionReference + Serialize> Serialize for Ty<I> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.wire_nodes()
            .map_err(Error::custom)?
            .serialize(serializer)
    }
}

impl<'de, I: DefinitionReference + Deserialize<'de>> Deserialize<'de> for Ty<I> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TypeVisitor<I>(PhantomData<I>);

        impl<'de, I: DefinitionReference + Deserialize<'de>> Visitor<'de> for TypeVisitor<I> {
            type Value = Ty<I>;

            fn expecting(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                formatter.write_str("a bounded preorder ABI type")
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                if sequence.size_hint().is_some_and(|count| count > MAX_NODES) {
                    return Err(DeError::custom("ABI type node limit exceeded"));
                }
                let mut nodes = Vec::new();
                while let Some(node) = sequence.next_element::<Node<I>>()? {
                    if nodes.len() >= MAX_NODES {
                        return Err(DeError::custom("ABI type node limit exceeded"));
                    }
                    nodes.push(node);
                }
                let mut nodes = nodes.into_iter();
                let ty = build(&mut nodes, 1, &mut |id| Ok(id), None).map_err(DeError::custom)?;
                if nodes.next().is_some() {
                    return Err(DeError::custom("trailing ABI type nodes"));
                }
                Ok(ty)
            }
        }
        deserializer.deserialize_seq(TypeVisitor(PhantomData))
    }
}

fn build<I: Clone + Ord, J: DefinitionReference>(
    nodes: &mut IntoIter<Node<I>>,
    depth: usize,
    convert: &mut impl FnMut(I) -> Result<J, IdentityTransformError>,
    cancel: Option<&CancellationToken>,
) -> Result<Ty<J>, IdentityTransformError> {
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
                previous = Some(id.clone());
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
                previous = Some(id.clone());
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

fn children<I: Clone + Ord, J: DefinitionReference>(
    nodes: &mut IntoIter<Node<I>>,
    count: u32,
    depth: usize,
    convert: &mut impl FnMut(I) -> Result<J, IdentityTransformError>,
    cancel: Option<&CancellationToken>,
) -> Result<Vec<Ty<J>>, IdentityTransformError> {
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
    fn abi_type_wire_round_trips_all_composite_shapes() {
        let id = DefinitionPath {
            module: ModuleIdentity::single_file("types.kgr"),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Struct,
                name: "Box".into(),
                occurrence: 0,
            }],
        };
        let nominal = NominalTy {
            associated_types: Default::default(),
            declaration: id.clone(),
            arguments: vec![Ty::Builtin(BuiltinType::I32)],
        };
        let types = vec![
            Ty::Host(id.clone()),
            Ty::SelfType(id.clone()),
            Ty::Parameter {
                owner: id,
                position: 0,
            },
            Ty::Tuple(vec![
                Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
                Ty::Map {
                    key: Box::new(Ty::Builtin(BuiltinType::String)),
                    value: Box::new(Ty::Set(
                        Box::new(Ty::Builtin(BuiltinType::I64)),
                        CollectionAccess::Mutable,
                    )),
                    access: CollectionAccess::Mutable,
                },
            ]),
            Ty::Function {
                params: vec![Ty::Builtin(BuiltinType::I32)],
                result: Box::new(Ty::Builtin(BuiltinType::Bool)),
            },
            Ty::Struct(nominal.clone()),
            Ty::Enum(nominal.clone()),
            Ty::Trait(nominal),
            Ty::Enum(crate::ty::NominalTy {
                declaration: crate::language::binding::option_declaration(),
                arguments: vec![Ty::Builtin(BuiltinType::Bool)],
                associated_types: Default::default(),
            }),
        ];
        for ty in types {
            let bytes = codec().serialize(&ty).unwrap();
            let decoded: Ty = codec().deserialize(&bytes).unwrap();
            assert_eq!(decoded, ty);
        }
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
        let interface = NominalTy {
            declaration: trait_id.clone(),
            arguments: vec![Ty::Builtin(BuiltinType::Bool)],
            associated_types: [(
                member.clone(),
                Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
            )]
            .into(),
        };
        for ty in [
            Ty::Trait(interface.clone()),
            Ty::Projection {
                receiver: Box::new(Ty::SelfType(trait_id.clone())),
                interface: Box::new(interface),
                member: member.clone(),
                arguments: vec![Ty::Builtin(BuiltinType::I32)],
            },
        ] {
            let encoded = codec().serialize(&ty).unwrap();
            assert_eq!(codec().deserialize::<Ty>(&encoded).unwrap(), ty);
        }
        let mut definitions = DefinitionTableBuilder::new().unwrap();
        let owner = definitions.intern_path(&trait_id).unwrap();
        let item = definitions.intern_path(&member).unwrap();
        let other = definitions
            .intern_path(&kagari_common::identity::associated_type_id(
                &trait_id, "Other",
            ))
            .unwrap();
        let encoding = definitions.freeze().encode([owner, item, other]).unwrap();
        let owner = encoding.reference(owner).unwrap();
        let item = encoding.reference(item).unwrap();
        let other = encoding.reference(other).unwrap();
        let decoding = encoding.table.decode().unwrap();
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
            declaration: trait_id,
            arguments: Vec::new(),
            associated_types: [(member, Ty::Builtin(BuiltinType::I32))].into(),
        });
        assert!(codec().serialize(&invalid).is_err());
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
        let too_wide: Ty<DefinitionId> = Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); MAX_NODES]);
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
