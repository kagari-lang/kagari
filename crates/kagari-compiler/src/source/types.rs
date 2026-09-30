//! Encode checked source types at the compiler boundary.
use kagari_abi::{
    numeric::NumericOperation,
    scalar::BuiltinType,
    types::{AbiType, NominalAbiType, native::NativeTypeConstructor},
};
use kagari_common::integer::IntegerOp;
use kagari_hir::{
    hir::BinaryOp,
    native::NativeTypeKind,
    types::{GenericParameterType, NominalType, TypeId},
};

pub(crate) fn lower_native_constructor(kind: NativeTypeKind) -> NativeTypeConstructor {
    match kind {
        NativeTypeKind::String => NativeTypeConstructor::String,
        NativeTypeKind::ArrayList => NativeTypeConstructor::Array,
        NativeTypeKind::LinkedHashMap => NativeTypeConstructor::Map,
        NativeTypeKind::LinkedHashSet => NativeTypeConstructor::Set,
        NativeTypeKind::Iter => NativeTypeConstructor::Iter,
        NativeTypeKind::Range(kind) => NativeTypeConstructor::Range(kind),
        NativeTypeKind::Enum(kind) => NativeTypeConstructor::Enum(kind),
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
