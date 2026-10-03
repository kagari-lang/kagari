//! Encode checked HIR types into portable semantic facts.
use crate::types::{NominalType, TypeId};
use kagari_abi::types::{AbiType, NominalAbiType};
use kagari_common::identity::reference::DefinitionReference;

pub fn lower_nominal_type<I: DefinitionReference>(ty: &NominalType<I>) -> NominalAbiType<I> {
    NominalAbiType {
        associated_types: ty
            .associated_types
            .iter()
            .map(|(id, ty)| (id.clone(), lower_type(ty)))
            .collect(),
        declaration: ty.declaration.clone(),
        arguments: ty.arguments.iter().map(lower_type).collect(),
    }
}

pub fn lower_type<I: DefinitionReference>(ty: &TypeId<I>) -> AbiType<I> {
    match ty {
        TypeId::Projection {
            receiver,
            interface,
            member,
            arguments,
        } => AbiType::Projection {
            arguments: arguments.iter().map(lower_type).collect(),
            receiver: Box::new(lower_type(receiver)),
            interface: Box::new(lower_nominal_type(interface)),
            member: member.clone(),
        },
        TypeId::Host(id) => AbiType::Host(id.clone()),
        TypeId::Builtin(ty) => AbiType::Builtin(*ty),
        TypeId::Tuple(elements) => AbiType::Tuple(elements.iter().map(lower_type).collect()),
        TypeId::Function { params, result } => AbiType::Function {
            params: params.iter().map(lower_type).collect(),
            result: Box::new(lower_type(result)),
        },
        TypeId::Range(element, kind) => AbiType::Range(Box::new(lower_type(element)), *kind),
        TypeId::Iter(element) => AbiType::Iter(Box::new(lower_type(element))),
        TypeId::Array(element, access) => AbiType::Array(Box::new(lower_type(element)), *access),
        TypeId::Map { key, value, access } => AbiType::Map {
            key: Box::new(lower_type(key)),
            value: Box::new(lower_type(value)),
            access: *access,
        },
        TypeId::Set(element, access) => AbiType::Set(Box::new(lower_type(element)), *access),
        TypeId::NativeObject(ty) => AbiType::NativeObject(lower_nominal_type(ty)),
        TypeId::Struct(ty) => AbiType::Struct(lower_nominal_type(ty)),
        TypeId::Enum(ty) => AbiType::Enum(lower_nominal_type(ty)),
        TypeId::Trait(ty) => AbiType::Trait(lower_nominal_type(ty)),
        TypeId::StandardEnum { kind, args } => AbiType::StandardEnum {
            kind: *kind,
            args: args.iter().map(lower_type).collect(),
        },
        TypeId::Generic(parameter) => AbiType::Parameter {
            owner: parameter.owner.clone(),
            position: parameter.position,
        },
        TypeId::SelfType(owner) => AbiType::SelfType(owner.clone()),
        TypeId::Inference(_) | TypeId::Unknown | TypeId::Error => {
            unreachable!("non-concrete type reached concrete ABI encoding")
        }
    }
}
