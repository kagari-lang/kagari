//! Bounded flat encoding keeps untrusted ABI type decoding off the Rust call stack.
use super::{AbiType, NominalAbiType};
use kagari_common::identity::DefinitionId;
use kagari_hir::{builtin::surface::StandardEnum, types::BuiltinType};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, SeqAccess, Visitor},
};

const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 4_096;

#[derive(Serialize, Deserialize)]
enum Node {
    Host(DefinitionId),
    SelfType(DefinitionId),
    Parameter {
        owner: DefinitionId,
        position: usize,
    },
    Builtin(BuiltinType),
    Tuple(u32),
    Function(u32),
    Cursor,
    Array,
    Map,
    Set,
    Struct(DefinitionId, u32),
    Enum(DefinitionId, u32),
    Trait(
        DefinitionId,
        u32,
        #[serde(deserialize_with = "crate::decode_limits::nested")] Vec<DefinitionId>,
    ),
    Projection {
        member_arguments: u32,
        member: DefinitionId,
        owner: DefinitionId,
        arguments: u32,
        #[serde(deserialize_with = "crate::decode_limits::nested")]
        bindings: Vec<DefinitionId>,
    },
    StandardEnum(StandardEnum, u32),
}

impl AbiType {
    pub(crate) fn within_wire_limits(&self) -> bool {
        self.wire_nodes().is_ok()
    }

    fn wire_nodes(&self) -> Result<Vec<Node>, &'static str> {
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
                Self::Cursor(element) => {
                    pending.push((element, depth + 1));
                    Node::Cursor
                }
                Self::Array(element) => {
                    pending.push((element, depth + 1));
                    Node::Array
                }
                Self::Map { key, value } => {
                    pending.push((value, depth + 1));
                    pending.push((key, depth + 1));
                    Node::Map
                }
                Self::Set(element) => {
                    pending.push((element, depth + 1));
                    Node::Set
                }
                Self::Struct(ty) => {
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
                    Node::Struct(ty.declaration.clone(), ty.arguments.len() as u32)
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

impl Serialize for AbiType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.wire_nodes()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AbiType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TypeVisitor;
        impl<'de> Visitor<'de> for TypeVisitor {
            type Value = AbiType;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a bounded preorder ABI type")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                if sequence.size_hint().is_some_and(|count| count > MAX_NODES) {
                    return Err(de::Error::custom("ABI type node limit exceeded"));
                }
                let mut nodes = Vec::new();
                while let Some(node) = sequence.next_element::<Node>()? {
                    if nodes.len() >= MAX_NODES {
                        return Err(de::Error::custom("ABI type node limit exceeded"));
                    }
                    nodes.push(node);
                }
                let mut nodes = nodes.into_iter();
                let ty = build::<A::Error>(&mut nodes, 1)?;
                if nodes.next().is_some() {
                    return Err(de::Error::custom("trailing ABI type nodes"));
                }
                Ok(ty)
            }
        }
        deserializer.deserialize_seq(TypeVisitor)
    }
}

fn build<E: de::Error>(nodes: &mut std::vec::IntoIter<Node>, depth: usize) -> Result<AbiType, E> {
    if depth > MAX_DEPTH {
        return Err(E::custom("ABI type depth limit exceeded"));
    }
    let next = nodes
        .next()
        .ok_or_else(|| E::custom("missing ABI type node"))?;
    let children = |count: u32, nodes: &mut std::vec::IntoIter<Node>| {
        if count as usize > nodes.len() {
            return Err(E::custom("missing ABI child nodes"));
        }
        (0..count)
            .map(|_| build(nodes, depth + 1))
            .collect::<Result<Vec<_>, E>>()
    };
    Ok(match next {
        Node::Host(id) => AbiType::Host(id),
        Node::SelfType(id) => AbiType::SelfType(id),
        Node::Parameter { owner, position } => AbiType::Parameter { owner, position },
        Node::Builtin(ty) => AbiType::Builtin(ty),
        Node::Tuple(count) => AbiType::Tuple(children(count, nodes)?),
        Node::Function(count) => AbiType::Function {
            params: children(count, nodes)?,
            result: Box::new(build(nodes, depth + 1)?),
        },
        Node::Cursor => AbiType::Cursor(Box::new(build(nodes, depth + 1)?)),
        Node::Array => AbiType::Array(Box::new(build(nodes, depth + 1)?)),
        Node::Map => AbiType::Map {
            key: Box::new(build(nodes, depth + 1)?),
            value: Box::new(build(nodes, depth + 1)?),
        },
        Node::Set => AbiType::Set(Box::new(build(nodes, depth + 1)?)),
        Node::Struct(id, count) => AbiType::Struct(NominalAbiType {
            associated_types: Default::default(),
            declaration: id,
            arguments: children(count, nodes)?,
        }),
        Node::Enum(id, count) => AbiType::Enum(NominalAbiType {
            associated_types: Default::default(),
            declaration: id,
            arguments: children(count, nodes)?,
        }),
        Node::Trait(id, count, bindings) => {
            let arguments = children(count, nodes)?;
            let mut associated_types = std::collections::BTreeMap::new();
            for id in bindings {
                if associated_types
                    .last_key_value()
                    .is_some_and(|(previous, _)| previous >= &id)
                {
                    return Err(E::custom("noncanonical associated type bindings"));
                }
                associated_types.insert(id, build(nodes, depth + 1)?);
            }
            AbiType::Trait(NominalAbiType {
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
            let mut associated_types = std::collections::BTreeMap::new();
            for id in bindings {
                if associated_types
                    .last_key_value()
                    .is_some_and(|(previous, _)| previous >= &id)
                {
                    return Err(E::custom("noncanonical associated type bindings"));
                }
                associated_types.insert(id, build(nodes, depth + 1)?);
            }
            AbiType::Projection {
                arguments: children(member_arguments, nodes)?,
                receiver,
                interface: Box::new(NominalAbiType {
                    declaration: owner,
                    arguments,
                    associated_types,
                }),
                member,
            }
        }
        Node::StandardEnum(kind, count) => AbiType::StandardEnum {
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
        let id = DefinitionId {
            module: ModuleIdentity::single_file("types.kgr"),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Struct,
                name: "Box".into(),
                occurrence: 0,
            }],
        };
        let nominal = NominalAbiType {
            associated_types: Default::default(),
            declaration: id.clone(),
            arguments: vec![AbiType::Builtin(BuiltinType::I32)],
        };
        let types = vec![
            AbiType::Host(id.clone()),
            AbiType::SelfType(id.clone()),
            AbiType::Parameter {
                owner: id,
                position: 0,
            },
            AbiType::Tuple(vec![
                AbiType::Array(Box::new(AbiType::Builtin(BuiltinType::I32))),
                AbiType::Map {
                    key: Box::new(AbiType::Builtin(BuiltinType::String)),
                    value: Box::new(AbiType::Set(Box::new(AbiType::Builtin(BuiltinType::I64)))),
                },
            ]),
            AbiType::Function {
                params: vec![AbiType::Builtin(BuiltinType::I32)],
                result: Box::new(AbiType::Builtin(BuiltinType::Bool)),
            },
            AbiType::Struct(nominal.clone()),
            AbiType::Enum(nominal.clone()),
            AbiType::Trait(nominal),
            AbiType::StandardEnum {
                kind: StandardEnum::Option,
                args: vec![AbiType::Builtin(BuiltinType::Bool)],
            },
        ];
        for ty in types {
            let bytes = codec().serialize(&ty).unwrap();
            let decoded: AbiType = codec().deserialize(&bytes).unwrap();
            assert_eq!(decoded, ty);
        }
    }

    #[test]
    fn associated_type_wire_preserves_identity_and_rejects_noncanonical_bindings() {
        let trait_id = DefinitionId {
            module: ModuleIdentity::single_file("associated.kgr"),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Trait,
                name: "Read".into(),
                occurrence: 0,
            }],
        };
        let member = kagari_hir::types::associated_type_id(&trait_id, "Item");
        let interface = NominalAbiType {
            declaration: trait_id.clone(),
            arguments: vec![AbiType::Builtin(BuiltinType::Bool)],
            associated_types: [(
                member.clone(),
                AbiType::Array(Box::new(AbiType::Builtin(BuiltinType::I32))),
            )]
            .into(),
        };
        for ty in [
            AbiType::Trait(interface.clone()),
            AbiType::Projection {
                receiver: Box::new(AbiType::SelfType(trait_id.clone())),
                interface: Box::new(interface),
                member: member.clone(),
                arguments: vec![AbiType::Builtin(BuiltinType::I32)],
            },
        ] {
            let encoded = codec().serialize(&ty).unwrap();
            assert_eq!(codec().deserialize::<AbiType>(&encoded).unwrap(), ty);
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
                    .deserialize::<AbiType>(&codec().serialize(&nodes).unwrap())
                    .is_err()
            );
        }
        let invalid = AbiType::Struct(NominalAbiType {
            declaration: trait_id,
            arguments: Vec::new(),
            associated_types: [(member, AbiType::Builtin(BuiltinType::I32))].into(),
        });
        assert!(codec().serialize(&invalid).is_err());
    }

    #[test]
    fn abi_type_wire_rejects_malformed_and_oversized_nodes() {
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
            let bytes = codec().serialize(&nodes).unwrap();
            assert!(codec().deserialize::<AbiType>(&bytes).is_err());
        }
        let nodes = (0..MAX_NODES + 1)
            .map(|_| Node::Builtin(BuiltinType::I32))
            .collect::<Vec<_>>();
        let bytes = codec().serialize(&nodes).unwrap();
        assert!(codec().deserialize::<AbiType>(&bytes).is_err());
        let too_wide = AbiType::Tuple(vec![AbiType::Builtin(BuiltinType::I32); MAX_NODES]);
        assert!(codec().serialize(&too_wide).is_err());

        let mut deep = AbiType::Builtin(BuiltinType::I32);
        for _ in 0..MAX_DEPTH {
            deep = AbiType::Array(Box::new(deep));
        }
        assert!(codec().serialize(&deep).is_err());
        let mut nodes = (0..MAX_DEPTH).map(|_| Node::Array).collect::<Vec<_>>();
        nodes.push(Node::Builtin(BuiltinType::I32));
        let bytes = codec().serialize(&nodes).unwrap();
        assert!(codec().deserialize::<AbiType>(&bytes).is_err());
    }
}
