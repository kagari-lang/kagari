//! Bounded flat encoding keeps untrusted ABI type decoding off the Rust call stack.

use crate::{
    scalar::BuiltinType,
    standard::surface::StandardEnum,
    types::{
        NominalTy, Ty,
        substitution::{MAX_TYPE_DEPTH as MAX_DEPTH, MAX_TYPE_NODES as MAX_NODES},
    },
};

use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionPath, reference::DefinitionReference},
    range::RangeKind,
};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, Error as DeError, SeqAccess, Visitor},
    ser::Error,
};

use std::{
    collections::BTreeMap,
    fmt::{self, Formatter},
    marker::PhantomData,
    vec::IntoIter,
};

#[derive(Serialize, Deserialize)]
enum Node<I = DefinitionPath> {
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
    Array(CollectionAccess),
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
    StandardEnum(StandardEnum, u32),
}

impl<I: DefinitionReference> Ty<I> {
    pub fn within_wire_limits(&self) -> bool {
        self.wire_nodes().is_ok()
    }

    fn wire_nodes(&self) -> Result<Vec<Node<I>>, &'static str> {
        let mut pending = vec![(self, 1usize)];
        let mut nodes = Vec::new();
        while let Some((ty, depth)) = pending.pop() {
            if depth > MAX_DEPTH || nodes.len() >= MAX_NODES {
                return Err("ABI type depth or node limit exceeded");
            }
            let node = match ty {
                Self::Host(id) => {
                    if !id.within_path_limit() {
                        return Err("ABI identity path limit exceeded");
                    }
                    Node::Host(id.clone())
                }
                Self::SelfType(id) => {
                    if !id.within_path_limit() {
                        return Err("ABI identity path limit exceeded");
                    }
                    Node::SelfType(id.clone())
                }
                Self::Parameter { owner, position } => {
                    if !owner.within_path_limit() {
                        return Err("ABI identity path limit exceeded");
                    }
                    Node::Parameter {
                        owner: owner.clone(),
                        position: *position,
                    }
                }
                Self::Builtin(ty) => Node::Builtin(*ty),
                Self::Tuple(elements) => {
                    if elements.len() > MAX_NODES {
                        return Err("ABI type node limit exceeded");
                    }
                    pending.extend(elements.iter().rev().map(|ty| (ty, depth + 1)));
                    Node::Tuple(elements.len() as u32)
                }
                Self::Function { params, result } => {
                    if params.len() >= MAX_NODES {
                        return Err("ABI type node limit exceeded");
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
                Self::Array(element, access) => {
                    pending.push((element, depth + 1));
                    Node::Array(*access)
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
                        return Err("associated bindings require a trait");
                    }
                    if !nominal.declaration.within_path_limit() {
                        return Err("ABI identity path limit exceeded");
                    }
                    if nominal.arguments.len() > MAX_NODES {
                        return Err("ABI type node limit exceeded");
                    }
                    pending.extend(nominal.arguments.iter().rev().map(|ty| (ty, depth + 1)));
                    if matches!(ty, Self::NativeObject(_)) {
                        Node::NativeObject(
                            nominal.declaration.clone(),
                            nominal.arguments.len() as u32,
                        )
                    } else {
                        Node::Struct(nominal.declaration.clone(), nominal.arguments.len() as u32)
                    }
                }
                Self::Enum(ty) => {
                    if !ty.associated_types.is_empty() {
                        return Err("associated bindings require a trait");
                    }
                    if !ty.declaration.within_path_limit() {
                        return Err("ABI identity path limit exceeded");
                    }
                    if ty.arguments.len() > MAX_NODES {
                        return Err("ABI type node limit exceeded");
                    }
                    pending.extend(ty.arguments.iter().rev().map(|ty| (ty, depth + 1)));
                    Node::Enum(ty.declaration.clone(), ty.arguments.len() as u32)
                }
                Self::Trait(ty) => {
                    if !ty.declaration.within_path_limit() {
                        return Err("ABI identity path limit exceeded");
                    }
                    if ty.arguments.len() > MAX_NODES {
                        return Err("ABI type node limit exceeded");
                    }
                    if ty.associated_types.len() > MAX_NODES
                        || ty.associated_types.keys().any(|id| !id.within_path_limit())
                    {
                        return Err("ABI associated type limit exceeded");
                    }
                    pending.extend(ty.associated_types.values().rev().map(|ty| (ty, depth + 1)));
                    pending.extend(ty.arguments.iter().rev().map(|ty| (ty, depth + 1)));
                    Node::Trait(
                        ty.declaration.clone(),
                        ty.arguments.len() as u32,
                        ty.associated_types.keys().cloned().collect(),
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
                        return Err("ABI projection limit exceeded");
                    }
                    pending.extend(arguments.iter().rev().map(|ty| (ty, depth + 1)));
                    pending.extend(
                        interface
                            .associated_types
                            .values()
                            .rev()
                            .map(|ty| (ty, depth + 1)),
                    );
                    pending.extend(interface.arguments.iter().rev().map(|ty| (ty, depth + 1)));
                    pending.push((receiver, depth + 1));
                    Node::Projection {
                        member_arguments: arguments.len() as u32,
                        member: member.clone(),
                        owner: interface.declaration.clone(),
                        arguments: interface.arguments.len() as u32,
                        bindings: interface.associated_types.keys().cloned().collect(),
                    }
                }
                Self::StandardEnum { kind, args } => {
                    if args.len() > MAX_NODES {
                        return Err("ABI type node limit exceeded");
                    }
                    pending.extend(args.iter().rev().map(|ty| (ty, depth + 1)));
                    Node::StandardEnum(*kind, args.len() as u32)
                }
            };
            nodes.push(node);
            if nodes.len() + pending.len() > MAX_NODES {
                return Err("ABI type node limit exceeded");
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
                let ty = build::<I, A::Error>(&mut nodes, 1)?;
                if nodes.next().is_some() {
                    return Err(DeError::custom("trailing ABI type nodes"));
                }
                Ok(ty)
            }
        }
        deserializer.deserialize_seq(TypeVisitor(PhantomData))
    }
}

fn build<I: DefinitionReference, E: de::Error>(
    nodes: &mut IntoIter<Node<I>>,
    depth: usize,
) -> Result<Ty<I>, E> {
    if depth > MAX_DEPTH {
        return Err(E::custom("ABI type depth limit exceeded"));
    }
    let next = nodes
        .next()
        .ok_or_else(|| E::custom("missing ABI type node"))?;
    let children = |count: u32, nodes: &mut IntoIter<Node<I>>| {
        if count as usize > nodes.len() {
            return Err(E::custom("missing ABI child nodes"));
        }
        (0..count)
            .map(|_| build(nodes, depth + 1))
            .collect::<Result<Vec<_>, E>>()
    };
    Ok(match next {
        Node::Host(id) => Ty::Host(id),
        Node::SelfType(id) => Ty::SelfType(id),
        Node::Parameter { owner, position } => Ty::Parameter { owner, position },
        Node::Builtin(ty) => Ty::Builtin(ty),
        Node::Tuple(count) => Ty::Tuple(children(count, nodes)?),
        Node::Function(count) => Ty::Function {
            params: children(count, nodes)?,
            result: Box::new(build(nodes, depth + 1)?),
        },
        Node::Range(kind) => Ty::Range(Box::new(build(nodes, depth + 1)?), kind),
        Node::Iter => Ty::Iter(Box::new(build(nodes, depth + 1)?)),
        Node::Array(access) => Ty::Array(Box::new(build(nodes, depth + 1)?), access),
        Node::Map(access) => Ty::Map {
            key: Box::new(build(nodes, depth + 1)?),
            value: Box::new(build(nodes, depth + 1)?),
            access,
        },
        Node::Set(access) => Ty::Set(Box::new(build(nodes, depth + 1)?), access),
        Node::Struct(id, count) => Ty::Struct(NominalTy {
            associated_types: Default::default(),
            declaration: id,
            arguments: children(count, nodes)?,
        }),
        Node::NativeObject(id, count) => Ty::NativeObject(NominalTy {
            associated_types: Default::default(),
            declaration: id,
            arguments: children(count, nodes)?,
        }),
        Node::Enum(id, count) => Ty::Enum(NominalTy {
            associated_types: Default::default(),
            declaration: id,
            arguments: children(count, nodes)?,
        }),
        Node::Trait(id, count, bindings) => {
            let arguments = children(count, nodes)?;
            let mut associated_types = BTreeMap::new();
            for id in bindings {
                if associated_types
                    .last_key_value()
                    .is_some_and(|(previous, _)| previous >= &id)
                {
                    return Err(E::custom("noncanonical associated type bindings"));
                }
                associated_types.insert(id, build(nodes, depth + 1)?);
            }
            Ty::Trait(NominalTy {
                declaration: id,
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
            let receiver = Box::new(build(nodes, depth + 1)?);
            let arguments = children(arguments, nodes)?;
            let mut associated_types = BTreeMap::new();
            for id in bindings {
                if associated_types
                    .last_key_value()
                    .is_some_and(|(previous, _)| previous >= &id)
                {
                    return Err(E::custom("noncanonical associated type bindings"));
                }
                associated_types.insert(id, build(nodes, depth + 1)?);
            }
            Ty::Projection {
                arguments: children(member_arguments, nodes)?,
                receiver,
                interface: Box::new(NominalTy {
                    declaration: owner,
                    arguments,
                    associated_types,
                }),
                member,
            }
        }
        Node::StandardEnum(kind, count) => Ty::StandardEnum {
            kind,
            args: children(count, nodes)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use bincode::Options;
    use kagari_common::identity::{DefinitionKind, DefinitionPathSegment, ModuleIdentity};

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
                Ty::Array(
                    Box::new(Ty::Builtin(BuiltinType::I32)),
                    CollectionAccess::Mutable,
                ),
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
            Ty::StandardEnum {
                kind: StandardEnum::Option,
                args: vec![Ty::Builtin(BuiltinType::Bool)],
            },
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
                Ty::Array(
                    Box::new(Ty::Builtin(BuiltinType::I32)),
                    CollectionAccess::Mutable,
                ),
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
        for bindings in [
            vec![member.clone(), member.clone()],
            vec![member.clone(); MAX_NODES + 1],
        ] {
            let nodes = vec![
                Node::Trait(trait_id.clone(), 0, bindings),
                Node::Builtin(BuiltinType::I32),
                Node::Builtin(BuiltinType::Bool),
            ];
            assert!(
                codec()
                    .deserialize::<Ty>(&codec().serialize(&nodes).unwrap())
                    .is_err()
            );
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
        for nodes in [
            vec![],
            vec![Node::<DefinitionPath>::Array(CollectionAccess::Mutable)],
            vec![Node::Function(1), Node::Builtin(BuiltinType::I32)],
            vec![Node::Tuple(2), Node::Builtin(BuiltinType::I32)],
            vec![
                Node::Builtin(BuiltinType::I32),
                Node::Builtin(BuiltinType::I64),
            ],
        ] {
            let bytes = codec().serialize(&nodes).unwrap();
            assert!(codec().deserialize::<Ty>(&bytes).is_err());
        }
        let nodes = (0..MAX_NODES + 1)
            .map(|_| Node::<DefinitionPath>::Builtin(BuiltinType::I32))
            .collect::<Vec<_>>();
        let bytes = codec().serialize(&nodes).unwrap();
        assert!(codec().deserialize::<Ty>(&bytes).is_err());
        let too_wide: Ty = Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); MAX_NODES]);
        assert!(codec().serialize(&too_wide).is_err());

        let mut deep: Ty = Ty::Builtin(BuiltinType::I32);
        for _ in 0..MAX_DEPTH {
            deep = Ty::Array(Box::new(deep), CollectionAccess::Mutable);
        }
        assert!(codec().serialize(&deep).is_err());
        let mut nodes = (0..MAX_DEPTH)
            .map(|_| Node::<DefinitionPath>::Array(CollectionAccess::Mutable))
            .collect::<Vec<_>>();
        nodes.push(Node::Builtin(BuiltinType::I32));
        let bytes = codec().serialize(&nodes).unwrap();
        assert!(codec().deserialize::<Ty>(&bytes).is_err());
    }
}
