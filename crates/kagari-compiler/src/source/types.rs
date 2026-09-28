//! Encode checked source types at the compiler boundary.
use kagari_abi::numeric::NumericOperation;
use kagari_abi::{
    scalar::BuiltinType,
    types::{AbiType, NominalAbiType},
};
use kagari_common::integer::IntegerOp;
use kagari_hir::{
    hir::BinaryOp,
    types::{GenericParameterType, NominalType, TypeId},
};
pub(crate) fn lower_nominal_type(ty: &NominalType) -> NominalAbiType {
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

pub(crate) fn lower_type(ty: &TypeId) -> AbiType {
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

pub(crate) fn lower_numeric_operation(
    op: BinaryOp,
    input: BuiltinType,
    rhs: BuiltinType,
) -> Option<NumericOperation> {
    input.integer_layout()?;
    let op = match op {
        BinaryOp::Add => IntegerOp::CheckedAdd,
        BinaryOp::Sub => IntegerOp::CheckedSub,
        BinaryOp::Mul => IntegerOp::CheckedMul,
        BinaryOp::Div => IntegerOp::CheckedDiv,
        BinaryOp::Rem => IntegerOp::CheckedRem,
        BinaryOp::BitAnd => IntegerOp::BitAnd,
        BinaryOp::BitOr => IntegerOp::BitOr,
        BinaryOp::BitXor => IntegerOp::BitXor,
        BinaryOp::Shl => IntegerOp::Shl,
        BinaryOp::Shr => IntegerOp::Shr,
        _ => return None,
    };
    Some(NumericOperation {
        op,
        input,
        rhs: Some(rhs),
    })
}

pub(crate) fn raise_nominal_type(ty: &NominalAbiType) -> NominalType {
    NominalType {
        associated_types: ty
            .associated_types
            .iter()
            .map(|(id, ty)| (id.clone(), raise_type(ty)))
            .collect(),
        declaration: ty.declaration.clone(),
        arguments: ty.arguments.iter().map(raise_type).collect(),
    }
}

pub(crate) fn raise_type(ty: &AbiType) -> TypeId {
    match ty {
        AbiType::Projection {
            receiver,
            interface,
            member,
            arguments,
        } => TypeId::Projection {
            arguments: arguments.iter().map(raise_type).collect(),
            receiver: Box::new(raise_type(receiver)),
            interface: Box::new(raise_nominal_type(interface)),
            member: member.clone(),
        },
        AbiType::Host(id) => TypeId::Host(id.clone()),
        AbiType::SelfType(id) => TypeId::SelfType(id.clone()),
        AbiType::Parameter { owner, position } => TypeId::Generic(GenericParameterType {
            owner: owner.clone(),
            position: *position,
            name: String::new(),
        }),
        AbiType::Builtin(ty) => TypeId::Builtin(*ty),
        AbiType::Tuple(types) => TypeId::Tuple(types.iter().map(raise_type).collect()),
        AbiType::Function { params, result } => TypeId::Function {
            params: params.iter().map(raise_type).collect(),
            result: Box::new(raise_type(result)),
        },
        AbiType::Range(ty, kind) => TypeId::Range(Box::new(raise_type(ty)), *kind),
        AbiType::Iter(ty) => TypeId::Iter(Box::new(raise_type(ty))),
        AbiType::Array(ty, access) => TypeId::Array(Box::new(raise_type(ty)), *access),
        AbiType::Map { key, value, access } => TypeId::Map {
            key: Box::new(raise_type(key)),
            value: Box::new(raise_type(value)),
            access: *access,
        },
        AbiType::Set(ty, access) => TypeId::Set(Box::new(raise_type(ty)), *access),
        AbiType::Struct(ty) => TypeId::Struct(raise_nominal_type(ty)),
        AbiType::Enum(ty) => TypeId::Enum(raise_nominal_type(ty)),
        AbiType::Trait(ty) => TypeId::Trait(raise_nominal_type(ty)),
        AbiType::StandardEnum { kind, args } => TypeId::StandardEnum {
            kind: *kind,
            args: args.iter().map(raise_type).collect(),
        },
    }
}
