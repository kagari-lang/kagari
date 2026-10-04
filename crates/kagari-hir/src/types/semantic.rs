//! Encode checked HIR types into portable semantic facts.
use crate::types::{NominalType, TypeId};
use kagari_common::identity::reference::DefinitionReference;
use kagari_types::ty::{NominalTy, Ty};

pub fn lower_nominal_type<I: DefinitionReference>(ty: &NominalType<I>) -> NominalTy<I> {
    NominalTy {
        associated_types: ty
            .associated_types
            .iter()
            .map(|(id, ty)| (id.clone(), lower_type(ty)))
            .collect(),
        declaration: ty.declaration.clone(),
        arguments: ty.arguments.iter().map(lower_type).collect(),
    }
}

pub fn lower_type<I: DefinitionReference>(ty: &TypeId<I>) -> Ty<I> {
    match ty {
        TypeId::Projection {
            receiver,
            interface,
            member,
            arguments,
        } => Ty::Projection {
            arguments: arguments.iter().map(lower_type).collect(),
            receiver: Box::new(lower_type(receiver)),
            interface: Box::new(lower_nominal_type(interface)),
            member: member.clone(),
        },
        TypeId::Host(id) => Ty::Host(id.clone()),
        TypeId::Builtin(ty) => Ty::Builtin(*ty),
        TypeId::Tuple(elements) => Ty::Tuple(elements.iter().map(lower_type).collect()),
        TypeId::Function { params, result } => Ty::Function {
            params: params.iter().map(lower_type).collect(),
            result: Box::new(lower_type(result)),
        },
        TypeId::Range(element, kind) => Ty::Range(Box::new(lower_type(element)), *kind),
        TypeId::Iter(element) => Ty::Iter(Box::new(lower_type(element))),
        TypeId::Array(element, access) => Ty::Array(Box::new(lower_type(element)), *access),
        TypeId::Map { key, value, access } => Ty::Map {
            key: Box::new(lower_type(key)),
            value: Box::new(lower_type(value)),
            access: *access,
        },
        TypeId::Set(element, access) => Ty::Set(Box::new(lower_type(element)), *access),
        TypeId::NativeObject(ty) => Ty::NativeObject(lower_nominal_type(ty)),
        TypeId::Struct(ty) => Ty::Struct(lower_nominal_type(ty)),
        TypeId::Enum(ty) => Ty::Enum(lower_nominal_type(ty)),
        TypeId::Trait(ty) => Ty::Trait(lower_nominal_type(ty)),
        TypeId::StandardEnum { kind, args } => Ty::StandardEnum {
            kind: *kind,
            args: args.iter().map(lower_type).collect(),
        },
        TypeId::Generic(parameter) => Ty::Parameter {
            owner: parameter.owner.clone(),
            position: parameter.position,
        },
        TypeId::SelfType(owner) => Ty::SelfType(owner.clone()),
        TypeId::Inference(_) | TypeId::Unknown | TypeId::Error => {
            unreachable!("non-concrete type reached concrete ABI encoding")
        }
    }
}
